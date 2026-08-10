use futures::StreamExt;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use taurine_core::engine::{ActiveWindowInfo, EngineState};
use tracing::{debug, error, info};
use zbus::Connection;

static JOIN_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

const KWIN_SCRIPT: &str = r#"
function emitActive(client) {
    if (client) {
        callDBus("com.taurine.WindowTracker", "/WindowTracker", "com.taurine.WindowTracker", "ActiveWindowChanged", 
                 client.caption || "", 
                 client.resourceClass || "", 
                 client.fullScreen ? true : false);
    }
}
workspace.windowActivated.connect(emitActive);
if (workspace.activeWindow) {
    emitActive(workspace.activeWindow);
} else if (workspace.activeClient) {
    emitActive(workspace.activeClient);
}
"#;

pub fn start_listener(state: Arc<EngineState>, active_window_store: Arc<Mutex<Option<String>>>) {
    let spawn_result = std::thread::Builder::new()
        .name("tau-lnx-kwin".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    error!(error = %error, "Failed to initialize KWin runtime");
                    return;
                }
            };

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

                let temp_dir = taurine_core::system::paths::ensure_temp_dir()
                    .join(format!("kwin_script_{}", uuid::Uuid::new_v4()));
                let _ = fs::create_dir_all(&temp_dir);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&temp_dir, fs::Permissions::from_mode(0o700));
                }

                let mut code_dir = temp_dir.clone();
                code_dir.push("contents");
                code_dir.push("code");

                let _ = fs::create_dir_all(&code_dir);

                let mut meta_path = temp_dir.clone();
                meta_path.push("metadata.json");
                let meta_json = r#"{
    "KPlugin": {
        "Id": "taurine_window_tracker",
        "Name": "Taurine Window Tracker",
        "Description": "Taurine Window Tracker"
    }
}"#;
                let _ = fs::write(&meta_path, meta_json);

                let mut js_path = code_dir.clone();
                js_path.push("main.js");
                let _ = fs::write(&js_path, KWIN_SCRIPT);

                let mut loaded_script_id = None;

                let proxy = zbus::Proxy::new(
                    &conn,
                    "org.kde.KWin",
                    "/Scripting",
                    "org.kde.kwin.Scripting",
                )
                .await;

                if let Ok(proxy) = proxy {
                    let script_path = temp_dir.to_string_lossy().to_string();
                    let plugin_name = "taurine_window_tracker".to_string();

                    let reply: Result<i32, _> =
                        proxy.call("loadScript", &(script_path, plugin_name)).await;
                    if let Ok(id) = reply {
                        loaded_script_id = Some(id);
                        let path = format!("/Scripting/Script{}", id);
                        if let Ok(start_proxy) = zbus::Proxy::new(
                            &conn,
                            "org.kde.KWin",
                            path.as_str(),
                            "org.kde.kwin.Script",
                        )
                        .await
                        {
                            let _ = start_proxy.call::<_, _, ()>("run", &()).await;
                        }
                    }
                }

                let match_rule_builder = match zbus::MatchRule::builder()
                    .interface("com.taurine.WindowTracker")
                    .and_then(|b| b.member("ActiveWindowChanged"))
                {
                    Ok(builder) => builder.build(),
                    Err(e) => {
                        error!("Failed to build KWin match rule: {:?}", e);
                        return;
                    }
                };

                let mut stream = match zbus::MessageStream::for_match_rule(
                    match_rule_builder,
                    &conn,
                    None,
                )
                .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to add match rule: {}", e);
                        return;
                    }
                };

                info!("KWin D-Bus toplevel listener started");
                SHUTDOWN.store(false, Ordering::Relaxed);

                while !SHUTDOWN.load(Ordering::Relaxed) {
                    if let Ok(Some(Ok(msg))) =
                        tokio::time::timeout(std::time::Duration::from_millis(500), stream.next())
                            .await
                    {
                        let header = msg.header();
                        if header.interface().map(|i| i.as_str())
                            == Some("com.taurine.WindowTracker")
                            && let Ok(body) = msg.body().deserialize::<(String, String, bool)>()
                        {
                            let (title, class, is_fullscreen) = body;

                            state
                                .is_os_fullscreen
                                .store(is_fullscreen, Ordering::Relaxed);

                            if let Ok(mut lock) = active_window_store.lock() {
                                let info = ActiveWindowInfo {
                                    title: if title.is_empty() { None } else { Some(title) },
                                    class: if class.is_empty() {
                                        None
                                    } else {
                                        Some(class.clone())
                                    },
                                    exec_name: if class.is_empty() { None } else { Some(class) },
                                    exec_path: None,
                                };
                                *lock = serde_json::to_string(&info).ok();
                            }
                        }
                    }
                }

                if let Some(id) = loaded_script_id {
                    let path = format!("/Scripting/Script{}", id);
                    if let Ok(start_proxy) = zbus::Proxy::new(
                        &conn,
                        "org.kde.KWin",
                        path.as_str(),
                        "org.kde.kwin.Script",
                    )
                    .await
                    {
                        let _ = start_proxy.call::<_, _, ()>("stop", &()).await;
                    }
                }
                let _ = fs::remove_dir_all(&temp_dir);

                debug!("KWin D-Bus toplevel listener shutdown");
            });
        });

    match spawn_result {
        Ok(handle) => {
            if let Ok(mut lock) = JOIN_HANDLE.lock() {
                *lock = Some(handle);
            }
        }
        Err(error) => {
            error!(error = %error, "Failed to spawn Linux KWin listener thread");
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
