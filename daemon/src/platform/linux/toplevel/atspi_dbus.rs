use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use taurine_core::engine::{ActiveWindowInfo, EngineState};
use tracing::{debug, error, info};
use zbus::{Connection, MatchRule};

static JOIN_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

pub fn start_listener(state: Arc<EngineState>, active_window_store: Arc<Mutex<Option<String>>>) {
    let handle = std::thread::Builder::new()
        .name("tau-lnx-atspi".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

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

                let a11y_conn = match zbus::connection::Builder::address(&a11y_address) {
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
                let match_rule = MatchRule::builder()
                    .interface("org.a11y.atspi.Event.Object")
                    .unwrap()
                    .member("StateChanged")
                    .unwrap()
                    .build();

                if let Err(e) = a11y_conn.add_match_rule(match_rule).await {
                    error!("Failed to add AT-SPI2 match rule: {:?}", e);
                    return;
                }

                let mut stream = zbus::MessageStream::from(a11y_conn.clone());

                info!("AT-SPI2 toplevel listener started");
                SHUTDOWN.store(false, Ordering::Relaxed);

                while !SHUTDOWN.load(Ordering::Relaxed) {
                    if let Ok(Some(msg)) =
                        tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
                            .await
                    {
                        if let Ok(msg) = msg {
                            if let Ok(body) = msg
                                .body()
                                .deserialize::<(String, i32, i32, zbus::zvariant::Value)>()
                            {
                                let (state_name, detail1, _detail2, _any_data) = body;

                                if state_name == "focused" && detail1 == 1 {
                                    // A window or widget just got focus.
                                    // Query the name of the focused object
                                    if let (Ok(sender), Ok(path)) = (
                                        msg.header().and_then(|h| h.sender().map(|s| s.clone())),
                                        msg.header().and_then(|h| h.path().map(|p| p.clone())),
                                    ) {
                                        let sender_str = sender.as_str().to_string();
                                        let path_str = path.as_str().to_string();
                                        let a11y_conn_clone = a11y_conn.clone();
                                        let store_clone = active_window_store.clone();

                                        // Spawn a quick task to query the name without blocking the event loop
                                        tokio::spawn(async move {
                                            if let Ok(node_proxy) = zbus::Proxy::new(
                                                &a11y_conn_clone,
                                                sender_str,
                                                path_str,
                                                "org.a11y.atspi.Accessible",
                                            )
                                            .await
                                            {
                                                if let Ok(name) =
                                                    node_proxy.get_property::<String>("Name").await
                                                {
                                                    // Simple heuristic: if name is non-empty, use it as title
                                                    if !name.is_empty() {
                                                        if let Ok(mut lock) = store_clone.lock() {
                                                            let info = ActiveWindowInfo {
                                                                title: Some(name),
                                                                class: None, // Getting class from AT-SPI2 is complex (requires traversing up to Application role)
                                                                exec_name: None,
                                                                exec_path: None,
                                                            };
                                                            *lock =
                                                                serde_json::to_string(&info).ok();
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                debug!("AT-SPI2 toplevel listener shutdown");
            });
        })
        .expect("Failed to spawn Linux AT-SPI2 listener thread");

    if let Ok(mut lock) = JOIN_HANDLE.lock() {
        *lock = Some(handle);
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
