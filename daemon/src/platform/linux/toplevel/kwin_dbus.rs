use futures::StreamExt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use taurine_core::engine::{ActiveWindowInfo, EngineState};
use tracing::{debug, error, info};
use zbus::{Connection, MessageStream};

static JOIN_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

const KWIN_SCRIPT: &str = r#"
workspace.windowActivated.connect(function(client) {
    if (client) {
        callDBus("com.taurine.WindowTracker", "/WindowTracker", "com.taurine.WindowTracker", "ActiveWindowChanged", 
                 client.caption || "", 
                 client.resourceClass || "", 
                 client.fullScreen ? true : false);
    }
});
"#;

pub fn start_listener(state: Arc<EngineState>, active_window_store: Arc<Mutex<Option<String>>>) {
    let handle = std::thread::Builder::new()
        .name("tau-lnx-kwin".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                let conn = match Connection::session().await {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to connect to D-Bus for KWin backend: {:?}", e);
                        return;
                    }
                };

                // Request a name so the script can send signals to us, or we just listen to signals globally
                // Actually, the script broadcasts via callDBus. We can just listen to the signal.

                // Write script to temp file
                let mut temp_dir = std::env::temp_dir();
                temp_dir.push("taurine_kwin_script.js");
                if let Err(e) = fs::write(&temp_dir, KWIN_SCRIPT) {
                    error!("Failed to write KWin script: {}", e);
                    return;
                }

                // load script
                let proxy = zbus::Proxy::new(
                    &conn,
                    "org.kde.KWin",
                    "/Scripting",
                    "org.kde.kwin.Scripting",
                )
                .await;

                if let Ok(proxy) = proxy {
                    // Call loadScript(path, name)
                    let script_path = temp_dir.to_string_lossy().to_string();
                    let plugin_name = "taurine_window_tracker".to_string();

                    let reply: Result<i32, _> =
                        proxy.call("loadScript", &(script_path, plugin_name)).await;
                    if let Ok(id) = reply {
                        // KWin 5 returns an ID, KWin 6 might be different, but let's assume it loads.
                        // We also need to start it.
                        let start_proxy = zbus::Proxy::new(
                            &conn,
                            "org.kde.KWin",
                            &format!("/Scripting/Script{}", id),
                            "org.kde.kwin.Script",
                        )
                        .await;

                        if let Ok(start_proxy) = start_proxy {
                            let _ = start_proxy.call::<_, ()>("run", &()).await;
                        }
                    }
                }

                // Listen for our custom signal
                let mut stream = zbus::MessageStream::from(conn.clone());

                // Setup match rule for the signal
                let match_rule = zbus::MatchRule::builder()
                    .interface("com.taurine.WindowTracker")
                    .unwrap()
                    .member("ActiveWindowChanged")
                    .unwrap()
                    .build();

                if let Err(e) = conn.add_match_rule(match_rule).await {
                    error!("Failed to add match rule: {}", e);
                }

                info!("KWin D-Bus toplevel listener started");
                SHUTDOWN.store(false, Ordering::Relaxed);

                while !SHUTDOWN.load(Ordering::Relaxed) {
                    if let Ok(Some(msg)) =
                        tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
                            .await
                    {
                        if let Ok(msg) = msg {
                            if let Ok(header) = msg.header() {
                                if header.interface().map(|i| i.as_str())
                                    == Some("com.taurine.WindowTracker")
                                {
                                    if let Ok(body) =
                                        msg.body().deserialize::<(String, String, bool)>()
                                    {
                                        let (title, class, is_fullscreen) = body;

                                        state
                                            .is_os_fullscreen
                                            .store(is_fullscreen, Ordering::Relaxed);

                                        if let Ok(mut lock) = active_window_store.lock() {
                                            let info = ActiveWindowInfo {
                                                title: if title.is_empty() {
                                                    None
                                                } else {
                                                    Some(title)
                                                },
                                                class: if class.is_empty() {
                                                    None
                                                } else {
                                                    Some(class.clone())
                                                },
                                                exec_name: if class.is_empty() {
                                                    None
                                                } else {
                                                    Some(class)
                                                },
                                                exec_path: None,
                                            };
                                            *lock = serde_json::to_string(&info).ok();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                debug!("KWin D-Bus toplevel listener shutdown");
            });
        })
        .expect("Failed to spawn Linux KWin listener thread");

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
