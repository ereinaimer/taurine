use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use taurine_core::engine::{ActiveWindowInfo, EngineState};
use tracing::{debug, error, info};
use zbus::{Connection, MatchRule};

static JOIN_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

pub fn start_listener(state: Arc<EngineState>, active_window_store: Arc<Mutex<Option<String>>>) {
    let spawn_result = std::thread::Builder::new()
        .name("tau-lnx-atspi".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    error!(error = %error, "Failed to initialize AT-SPI2 runtime");
                    return;
                }
            };

            rt.block_on(async {
                let session_conn = match Connection::session().await {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to connect to session bus for AT-SPI2: {:?}", e);
                        return;
                    }
                };

                // Query a11y bus address
                let proxy = match zbus::Proxy::new(
                    &session_conn,
                    "org.a11y.Bus",
                    "/org/a11y/bus",
                    "org.a11y.Bus",
                )
                .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        error!("Failed to create proxy for org.a11y.Bus: {:?}", e);
                        return;
                    }
                };

                let a11y_address: String = match proxy.call("GetAddress", &()).await {
                    Ok(addr) => addr,
                    Err(e) => {
                        error!("Failed to get AT-SPI2 bus address: {:?}", e);
                        return;
                    }
                };

                let a11y_conn = match zbus::connection::Builder::address(a11y_address.as_str()) {
                    Ok(builder) => match builder.build().await {
                        Ok(c) => c,
                        Err(e) => {
                            error!("Failed to connect to AT-SPI2 bus: {:?}", e);
                            return;
                        }
                    },
                    Err(e) => {
                        error!("Invalid AT-SPI2 bus address {}: {:?}", a11y_address, e);
                        return;
                    }
                };

                // Listen to window focus events
                let match_rule_builder = match MatchRule::builder()
                    .interface("org.a11y.atspi.Event.Window")
                    .and_then(|b| b.member("Activate"))
                {
                    Ok(builder) => builder.build(),
                    Err(e) => {
                        error!("Failed to build AT-SPI2 match rule: {:?}", e);
                        return;
                    }
                };

                let mut stream =
                    match zbus::MessageStream::for_match_rule(match_rule_builder, &a11y_conn, None)
                        .await
                    {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Failed to create stream for AT-SPI2 match rule: {:?}", e);
                            return;
                        }
                    };

                info!("AT-SPI2 toplevel listener started");
                SHUTDOWN.store(false, Ordering::Relaxed);

                while !SHUTDOWN.load(Ordering::Relaxed) {
                    if let Ok(msg_opt) =
                        tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
                            .await
                    {
                        match msg_opt {
                            Some(Ok(msg)) => {
                                let header = msg.header();
                                if let (Some(sender), Some(path)) = (header.sender(), header.path())
                                {
                                    let sender_str = sender.as_str().to_string();
                                    let path_str = path.as_str().to_string();
                                    let a11y_conn_clone = a11y_conn.clone();
                                    let store_clone = active_window_store.clone();
                                    let state_clone = state.clone();

                                    // Spawn a quick task to query the name without blocking the event loop
                                    tokio::spawn(async move {
                                        if let Ok(node_proxy) = zbus::Proxy::new(
                                            &a11y_conn_clone,
                                            sender_str.as_str(),
                                            path_str.as_str(),
                                            "org.a11y.atspi.Accessible",
                                        )
                                        .await
                                        {
                                            let title = node_proxy
                                                .get_property::<String>("Name")
                                                .await
                                                .ok();

                                            let mut class = None;
                                            let app_ref_result: zbus::Result<(
                                                String,
                                                zbus::zvariant::OwnedObjectPath,
                                            )> = node_proxy.call("GetApplication", &()).await;
                                            if let Ok(app_ref) = app_ref_result
                                                && let Ok(app_proxy) = zbus::Proxy::new(
                                                    &a11y_conn_clone,
                                                    app_ref.0.as_str(),
                                                    &app_ref.1,
                                                    "org.a11y.atspi.Accessible",
                                                )
                                                .await
                                            {
                                                class = app_proxy
                                                    .get_property::<String>("Name")
                                                    .await
                                                    .ok();
                                            }

                                            // We don't have a reliable way to get AT-SPI2 fullscreen state
                                            // without importing the entire atspi state table, so we gracefully
                                            // default to false to allow macros to run.
                                            state_clone
                                                .is_os_fullscreen
                                                .store(false, Ordering::Relaxed);

                                            if let Ok(mut lock) = store_clone.lock() {
                                                let info = ActiveWindowInfo {
                                                    title: title.clone().filter(|t| !t.is_empty()),
                                                    class: class.clone().filter(|c| !c.is_empty()),
                                                    exec_name: class.filter(|c| !c.is_empty()),
                                                    exec_path: None,
                                                };
                                                *lock = serde_json::to_string(&info).ok();
                                            }
                                        }
                                    });
                                }
                            }
                            Some(Err(e)) => {
                                error!("AT-SPI2 stream error: {:?}", e);
                                break;
                            }
                            None => {
                                error!("AT-SPI2 bus disconnected");
                                break;
                            }
                        }
                    }
                }

                debug!("AT-SPI2 toplevel listener shutdown");
            });
        });

    match spawn_result {
        Ok(handle) => {
            if let Ok(mut lock) = JOIN_HANDLE.lock() {
                *lock = Some(handle);
            }
        }
        Err(error) => {
            error!(error = %error, "Failed to spawn Linux AT-SPI2 listener thread");
        }
    }
}

pub fn stop_listener() {
    SHUTDOWN.store(true, Ordering::Relaxed);
    let handle = if let Ok(mut lock) = JOIN_HANDLE.lock() {
        lock.take()
    } else {
        None
    };

    if let Some(h) = handle {
        let _ = h.join();
    }
}
