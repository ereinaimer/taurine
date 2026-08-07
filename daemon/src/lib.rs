// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use taurine_core::db::init;
use taurine_core::rpc::daemon_control_server::DaemonControlServer;
use tokio::sync::mpsc;
use tonic::transport::Server;
use tracing::{debug, error, info};

mod engine;
mod hook;
mod injector;
mod input;
pub mod platform;
mod services;

pub use services::server::DaemonService;

static FILE_LOG_GUARD: std::sync::OnceLock<Option<tracing_appender::non_blocking::WorkerGuard>> =
    std::sync::OnceLock::new();

pub(crate) static TOKIO_HANDLE: std::sync::OnceLock<tokio::runtime::Handle> =
    std::sync::OnceLock::new();

pub fn start() -> taurine_core::error::Result<()> {
    let conn = init::setup()?;

    // Initialize injection thread pool (replaces per-expansion thread spawning)
    crate::injector::init_injection_pool();

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = crate::platform::linux::init() {
            error!("Linux platform initialization failed: {}", e);
            return Err(taurine_core::error::Error::Service(e));
        }
    }

    debug!("Service initialization complete!");

    // Instantiate the Core Engine State
    use std::sync::{Arc, Mutex, RwLock};
    use taurine_core::db::crud::{
        get_all_active_hotkey_triggers, get_all_active_regex_triggers, get_all_active_triggers,
    };
    use taurine_core::engine::{EngineState, Evaluator};
    use taurine_core::settings::SettingsManager;

    let settings_manager = SettingsManager::new(&conn);
    let settings = settings_manager.load_all();

    taurine_core::settings::set_cached_wpm(settings.wpm);
    taurine_core::settings::set_cached_clipboard_restore_delay(settings.clipboard_restore_delay_ms);
    taurine_core::settings::set_cached_script_timeout(settings.script_timeout);
    taurine_core::settings::set_cached_clipboard_history_enabled(
        settings.clipboard_history_enabled,
    );
    taurine_core::settings::set_cached_clipboard_history_retention_secs(
        settings.clipboard_history_retention_secs,
    );
    taurine_core::settings::set_cached_inline_emoji_enabled(settings.inline_emoji_enabled);
    taurine_core::settings::set_cached_inline_emoji_trigger_char(
        settings.inline_emoji_trigger_char,
    );
    taurine_core::settings::set_cached_inline_datetime_enabled(settings.inline_datetime_enabled);
    taurine_core::settings::set_cached_inline_datetime_date_format(
        settings.inline_datetime_date_format.clone(),
    );
    taurine_core::settings::set_cached_inline_datetime_time_format(
        settings.inline_datetime_time_format.clone(),
    );
    taurine_core::settings::set_cached_inline_datetime_datetime_format(
        settings.inline_datetime_datetime_format.clone(),
    );
    taurine_core::settings::set_cached_inline_datetime_dialect(
        settings.inline_datetime_dialect.clone(),
    );
    taurine_core::settings::set_cached_inline_currency_to_words_enabled(
        settings.inline_currency_to_words_enabled,
    );
    taurine_core::settings::set_cached_scripts_enabled(settings.scripts_enabled);

    let state = Arc::new(EngineState::new());
    state
        .inline_tab_completion_enabled
        .store(settings.inline_tab_completion_enabled, Ordering::Relaxed);
    state
        .instant_expand
        .store(settings.instant_expand, Ordering::Relaxed);
    state
        .ignore_fullscreen_enabled
        .store(settings.ignore_fullscreen, Ordering::Relaxed);
    state
        .inline_datetime_enabled
        .store(settings.inline_datetime_enabled, Ordering::Relaxed);
    state
        .inline_currency_to_words_enabled
        .store(settings.inline_currency_to_words_enabled, Ordering::Relaxed);
    state.set_inline_datetime_date_format(settings.inline_datetime_date_format.clone());
    state.set_inline_datetime_time_format(settings.inline_datetime_time_format.clone());
    state.set_inline_datetime_datetime_format(settings.inline_datetime_datetime_format.clone());
    state.set_inline_datetime_dialect(settings.inline_datetime_dialect.clone());
    state.set_inline_ai_trigger_mode(settings.inline_ai_trigger_mode);

    state.set_action_key(settings.action_key);

    // Global pause toggle hotkey (display + parse).
    let pause_hotkey = Arc::new(RwLock::new(settings.pause_hotkey.clone()));

    let pause_hotkey_spec = Arc::new(RwLock::new(
        input::hotkey::parse_pause_hotkey_setting(&settings.pause_hotkey).unwrap_or_else(|| {
            // Fall back to strict default if DB is malformed or unsupported.
            tracing::warn!("Configured pause hotkey is invalid; using default Alt + `");
            input::hotkey::HotkeySpec {
                hotkey: taurine_core::keys::taurine_pause_hotkey(),
            }
        }),
    ));

    let pause_notifications_enabled = settings.pause_notifications_enabled;
    let spinner_style = Arc::new(RwLock::new(settings.spinner_style));

    let evaluator = Arc::new(Mutex::new(Evaluator::new(state.clone())));

    let paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pause_notifications_enabled = Arc::new(std::sync::atomic::AtomicBool::new(
        pause_notifications_enabled,
    ));
    let pause_audio_enabled = Arc::new(std::sync::atomic::AtomicBool::new(
        settings.pause_audio_enabled,
    ));
    let system_tray_enabled = Arc::new(std::sync::atomic::AtomicBool::new(
        settings.system_tray_enabled,
    ));
    let hook_health = input::hook_health::HookHealth::new();

    let (audio_tx, audio_rx) = services::audio::create_channel();
    let (pause_transition_tx, mut pause_transition_rx) = tokio::sync::mpsc::channel::<bool>(8);

    // Fire up listener in OS thread
    let eval_clone = evaluator.clone();
    let state_clone = state.clone();
    let paused_clone = paused.clone();
    let pause_notifications_enabled_clone = pause_notifications_enabled.clone();
    let pause_hotkey_spec_clone = pause_hotkey_spec.clone();
    let spinner_style_clone = spinner_style.clone();
    let pause_audio_enabled_clone = pause_audio_enabled.clone();
    let audio_tx_clone = audio_tx.clone();
    let pause_transition_tx_clone = pause_transition_tx.clone();
    #[cfg(windows)]
    let supervisor_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>> =
        Arc::new(Mutex::new(None));
    #[cfg(windows)]
    let supervisor_handle_clone = supervisor_handle.clone();

    #[cfg(windows)]
    let hook_health_clone = hook_health.clone();
    let hook_thread = std::thread::Builder::new()
        .name("tau-hook".to_string())
        .spawn(move || {
            #[cfg(windows)]
            {
                info!("Starting supervised Windows keyboard hook listener...");
                let handle = hook::start_windows_supervisor(
                    eval_clone,
                    state_clone,
                    paused_clone,
                    pause_notifications_enabled_clone,
                    pause_hotkey_spec_clone,
                    spinner_style_clone,
                    pause_audio_enabled_clone,
                    audio_tx_clone,
                    pause_transition_tx_clone,
                    hook_health_clone,
                );
                if let Ok(mut lock) = supervisor_handle_clone.lock() {
                    *lock = handle;
                }
            }

            #[cfg(not(windows))]
            {
                info!("Starting OS keyboard hook listener...");
                hook::start_listener(
                    eval_clone,
                    state_clone,
                    paused_clone,
                    pause_notifications_enabled_clone,
                    pause_hotkey_spec_clone,
                    spinner_style_clone,
                    pause_audio_enabled_clone,
                    audio_tx_clone,
                    pause_transition_tx_clone,
                );
            }
        })?;

    // Start deferred heavy initializations

    // 1. Load snippets efficiently in background
    let state_for_bg = state.clone();
    std::thread::Builder::new()
        .name("tau-db-load".to_string())
        .spawn(move || {
            if let Ok(conn) = taurine_core::db::init::setup() {
                if let Ok(active) = get_all_active_triggers(&conn) {
                    state_for_bg.load_actions(active);
                }
                if let Ok(active_hotkeys) = get_all_active_hotkey_triggers(&conn) {
                    state_for_bg.load_hotkey_actions(active_hotkeys);
                }
                if let Ok(active_regex) = get_all_active_regex_triggers(&conn) {
                    state_for_bg.load_regex_actions(active_regex);
                }
            }
        })?;

    // 2. Start clipboard history listener
    let clipboard_thread = std::thread::Builder::new()
        .name("tau-clip".to_string())
        .spawn(|| {
            info!("Starting clipboard history listener...");
            services::clipboard_history::start_listener();
        })?;

    // 3. Start fullscreen listeners
    #[cfg(windows)]
    crate::platform::windows::fullscreen::start_listener(state.clone());
    #[cfg(target_os = "linux")]
    crate::platform::linux::toplevel::start_listener(state.clone());
    #[cfg(target_os = "macos")]
    crate::platform::macos::fullscreen::start_listener(state.clone());

    // 4. Start audio worker
    services::audio::start_worker(audio_rx);

    // 5. Start system tray icon
    crate::services::tray::spawn(paused.clone(), system_tray_enabled.clone());

    // Activate daemon file logging immediately after hook thread starts capturing
    let guard = taurine_core::logs::activate_file_logging();
    let _ = FILE_LOG_GUARD.set(guard);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()?;
    let _ = TOKIO_HANDLE.set(rt.handle().clone());

    let run_result = rt.block_on(async move {
        let shutdown_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let rpc_reload_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let evaluator_for_coordinator = evaluator.clone();
        #[cfg(any(windows, target_os = "linux"))]
        let state_for_coordinator = state.clone();
        let pause_notifications_enabled_for_coordinator = pause_notifications_enabled.clone();
        let pause_audio_enabled_for_coordinator = pause_audio_enabled.clone();
        let audio_tx_for_coordinator = audio_tx.clone();
        tokio::spawn(async move {
            while let Some(is_paused) = pause_transition_rx.recv().await {
                if is_paused {
                    info!("Taurine paused: entering deep idle state...");
                    // 1. Stop fullscreen detection
                    #[cfg(windows)]
                    crate::platform::windows::fullscreen::stop_listener();
                    #[cfg(target_os = "linux")]
                    crate::platform::linux::toplevel::stop_listener();

                    // 2. Suspend clipboard listener
                    crate::services::clipboard_history::suspend_listener();

                    // 3. Clear transient evaluator state
                    if let Ok(mut lock) = evaluator_for_coordinator.lock() {
                        lock.reset();
                    }
                } else {
                    info!("Taurine resumed: restoring subsystems...");
                    // 1. Resume clipboard listener
                    crate::services::clipboard_history::resume_listener();

                    // 2. Restart fullscreen detection
                    #[cfg(windows)]
                    crate::platform::windows::fullscreen::start_listener(
                        state_for_coordinator.clone(),
                    );
                    #[cfg(target_os = "linux")]
                    crate::platform::linux::toplevel::start_listener(state_for_coordinator.clone());
                }

                if pause_notifications_enabled_for_coordinator.load(Ordering::Relaxed) {
                    services::notify::notify_pause_toggled(is_paused);
                }
                if pause_audio_enabled_for_coordinator.load(Ordering::Relaxed) {
                    let _ = audio_tx_for_coordinator.try_send(is_paused);
                }
            }
        });

        let (mut shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        let (mut rpc_reload_tx, mut rpc_reload_rx) = mpsc::channel(1);

        let active_rpc_settings = std::sync::Arc::new(std::sync::RwLock::new(
            services::server::RpcServerSettings {
                rpc_mode: settings.rpc_mode,
                rpc_host: settings.rpc_host.clone(),
                rpc_port: settings.rpc_port,
                rpc_token: settings.rpc_token.clone(),
            },
        ));

        loop {
            let shutdown_requested_clone = shutdown_requested.clone();
            let rpc_reload_requested_clone = rpc_reload_requested.clone();

            let watcher_task = {
                let shutdown_requested_clone = shutdown_requested_clone.clone();
                let rpc_reload_requested_clone = rpc_reload_requested_clone.clone();
                tokio::spawn(async move {
                    #[cfg(unix)]
                    let mut sigterm = match tokio::signal::unix::signal(
                        tokio::signal::unix::SignalKind::terminate(),
                    ) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            error!("Failed to register SIGTERM handler: {}", e);
                            None
                        }
                    };

                    #[cfg(unix)]
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            shutdown_requested_clone.store(true, Ordering::Relaxed);
                        }
                        _ = rpc_reload_rx.recv() => {
                            rpc_reload_requested_clone.store(true, Ordering::Relaxed);
                        }
                        _ = tokio::signal::ctrl_c() => {
                            info!("System Ctrl+C received, initiating shutdown...");
                            shutdown_requested_clone.store(true, Ordering::Relaxed);
                        }
                        _ = async {
                            if let Some(ref mut sig) = sigterm {
                                sig.recv().await;
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => {
                            info!("System SIGTERM received, initiating shutdown...");
                            shutdown_requested_clone.store(true, Ordering::Relaxed);
                        }
                    }

                    #[cfg(not(unix))]
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            shutdown_requested_clone.store(true, Ordering::Relaxed);
                        }
                        _ = rpc_reload_rx.recv() => {
                            rpc_reload_requested_clone.store(true, Ordering::Relaxed);
                        }
                        _ = tokio::signal::ctrl_c() => {
                            info!("System Ctrl+C received, initiating shutdown...");
                            shutdown_requested_clone.store(true, Ordering::Relaxed);
                        }
                    }
                })
            };

            let daemon_service = DaemonService::builder()
                .shutdown_sender(shutdown_tx.clone())
                .state(state.clone())
                .paused(paused.clone())
                .pause_notifications_enabled(pause_notifications_enabled.clone())
                .pause_hotkey_spec(pause_hotkey_spec.clone())
                .pause_hotkey_display(pause_hotkey.clone())
                .spinner_style(spinner_style.clone())
                .pause_audio_enabled(pause_audio_enabled.clone())
                .system_tray_enabled(system_tray_enabled.clone())
                .hook_health(hook_health.clone())
                .active_rpc_settings(active_rpc_settings.clone())
                .rpc_reload_sender(rpc_reload_tx.clone())
                .pause_transition_tx(pause_transition_tx.clone())
                .build()
                .map_err(taurine_core::error::Error::Config)?;

            let current_rpc = {
                let lock = match active_rpc_settings.read() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        tracing::warn!("active_rpc_settings lock poisoned; recovering");
                        poisoned.into_inner()
                    }
                };
                lock.clone()
            };

            let token = current_rpc.rpc_token.clone();
            let use_tcp = current_rpc.rpc_mode == taurine_core::settings::RpcMode::Tcp;

            let shutdown_requested_for_signal = shutdown_requested.clone();
            let rpc_reload_requested_for_signal = rpc_reload_requested.clone();
            let shutdown_signal = async move {
                loop {
                    if shutdown_requested_for_signal.load(Ordering::Relaxed) {
                        debug!("Shutdown signal received, initiating gRPC server shutdown...");
                        break;
                    }
                    if rpc_reload_requested_for_signal.load(Ordering::Relaxed) {
                        debug!("RPC settings changed, reloading gRPC server...");
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            };

            let auth_token = token.clone();
            let auth_interceptor =
                move |req: tonic::Request<()>| -> Result<tonic::Request<()>, tonic::Status> {
                    if let Some(auth_val) = req.metadata().get("authorization")
                        && let Ok(auth_str) = auth_val.to_str()
                        && auth_str.starts_with("Bearer ")
                        && &auth_str[7..] == auth_token.as_str()
                    {
                        return Ok(req);
                    }
                    Err(tonic::Status::unauthenticated(
                        "Invalid or missing RPC token",
                    ))
                };

            if use_tcp {
                let addr_parsed = current_rpc
                    .rpc_host
                    .parse::<std::net::IpAddr>()
                    .unwrap_or_else(|_| [127, 0, 0, 1].into());
                let addr = SocketAddr::new(addr_parsed, current_rpc.rpc_port);

                info!("Starting authenticated gRPC server on {}", addr);
                let server_future = Server::builder()
                    .add_service(DaemonControlServer::with_interceptor(
                        daemon_service,
                        auth_interceptor,
                    ))
                    .serve_with_shutdown(addr, shutdown_signal);

                if let Err(e) = server_future.await {
                    error!("gRPC server failed: {}", e);
                    return Err(taurine_core::error::Error::Transport(Box::new(e)));
                }
            } else {
                #[cfg(all(unix, not(target_os = "android")))]
                {
                    use tokio::net::UnixListener;
                    use tokio_stream::wrappers::UnixListenerStream;

                    let socket_path = taurine_core::paths::get_data_dir().join("taurine.sock");
                    if socket_path.exists()
                        && std::os::unix::net::UnixStream::connect(&socket_path).is_ok()
                    {
                        error!(
                            "Another service instance is already listening on UDS socket: {}",
                            socket_path.display()
                        );
                        return Err(taurine_core::error::Error::Service(format!(
                            "Another service instance is already listening on UDS socket: {}",
                            socket_path.display()
                        )));
                    }
                    let _ = std::fs::remove_file(&socket_path);

                    match UnixListener::bind(&socket_path) {
                        Ok(uds) => {
                            use std::os::unix::fs::PermissionsExt;
                            if let Ok(metadata) = std::fs::metadata(&socket_path) {
                                let mut perms = metadata.permissions();
                                perms.set_mode(0o600);
                                let _ = std::fs::set_permissions(&socket_path, perms);
                            }

                            let stream = UnixListenerStream::new(uds);
                            debug!(
                                "Starting gRPC server on UDS socket: {}",
                                socket_path.display()
                            );
                            let server_future = Server::builder()
                                .add_service(DaemonControlServer::with_interceptor(
                                    daemon_service,
                                    auth_interceptor.clone(),
                                ))
                                .serve_with_incoming_shutdown(stream, shutdown_signal);

                            if let Err(e) = server_future.await {
                                error!("gRPC server failed: {}", e);
                                return Err(taurine_core::error::Error::Transport(Box::new(e)));
                            }

                            let _ = std::fs::remove_file(&socket_path);
                        }
                        Err(e) => {
                            error!("Failed to bind to UDS socket: {}", e);
                            return Err(taurine_core::error::Error::Io(e));
                        }
                    }
                }

                #[cfg(target_os = "windows")]
                {
                    use tokio::net::windows::named_pipe::ServerOptions;

                    let pipe_path = r"\\.\pipe\taurine";

                    let first_server = match ServerOptions::new()
                        .first_pipe_instance(true)
                        .create(pipe_path)
                    {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Failed to create first named pipe instance: {}", e);
                            return Err(taurine_core::error::Error::Io(e));
                        }
                    };

                    debug!(
                        "Starting gRPC server on Named Pipe UDS equivalent: {}",
                        pipe_path
                    );

                    let (connection_tx, connection_rx) =
                        tokio::sync::mpsc::channel::<Result<NamedPipeConn, std::io::Error>>(16);

                    let accept_task = {
                        let pipe_path = pipe_path.to_string();
                        tokio::spawn(async move {
                            let mut next_server = Some(first_server);
                            loop {
                                let server_instance = match next_server.take() {
                                    Some(s) => s,
                                    None => match ServerOptions::new()
                                        .first_pipe_instance(false)
                                        .create(&pipe_path)
                                    {
                                        Ok(s) => s,
                                        Err(e) => {
                                            error!("Failed to create named pipe instance: {}", e);
                                            tokio::time::sleep(std::time::Duration::from_millis(
                                                100,
                                            ))
                                            .await;
                                            continue;
                                        }
                                    },
                                };

                                if server_instance.connect().await.is_ok()
                                    && connection_tx
                                        .send(Ok(NamedPipeConn(server_instance)))
                                        .await
                                        .is_err()
                                {
                                    break;
                                }
                            }
                        })
                    };
                    let accept_task_abort = accept_task.abort_handle();

                    let stream = tokio_stream::wrappers::ReceiverStream::new(connection_rx);
                    let server_future = Server::builder()
                        .add_service(DaemonControlServer::with_interceptor(
                            daemon_service,
                            auth_interceptor.clone(),
                        ))
                        .serve_with_incoming_shutdown(stream, shutdown_signal);

                    if let Err(e) = server_future.await {
                        error!("gRPC server failed: {}", e);
                        accept_task_abort.abort();
                        return Err(taurine_core::error::Error::Transport(Box::new(e)));
                    }
                    accept_task_abort.abort();
                }

                #[cfg(not(any(all(unix, not(target_os = "android")), target_os = "windows")))]
                {
                    let addr = SocketAddr::from(([127, 0, 0, 1], current_rpc.rpc_port));
                    info!("Starting fallback gRPC server on {}", addr);
                    let server_future = Server::builder()
                        .add_service(DaemonControlServer::with_interceptor(
                            daemon_service,
                            auth_interceptor.clone(),
                        ))
                        .serve_with_shutdown(addr, shutdown_signal);
                    if let Err(e) = server_future.await {
                        error!("gRPC server failed: {}", e);
                        return Err(taurine_core::error::Error::Transport(Box::new(e)));
                    }
                }
            }

            let _ = watcher_task.await;

            if shutdown_requested.load(Ordering::Relaxed) {
                break;
            }

            // Reset reload request flag
            rpc_reload_requested.store(false, Ordering::Relaxed);

            // Re-create channels for next iteration
            let (new_shutdown_tx, new_shutdown_rx) = mpsc::channel(1);
            let (new_rpc_reload_tx, new_rpc_reload_rx) = mpsc::channel(1);
            shutdown_tx = new_shutdown_tx;
            shutdown_rx = new_shutdown_rx;
            rpc_reload_tx = new_rpc_reload_tx;
            rpc_reload_rx = new_rpc_reload_rx;

            // Load the new settings from the database and update Arc<RwLock<RpcServerSettings>>
            if let Ok(conn) = taurine_core::db::init::setup() {
                let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
                if let Ok(mut lock) = active_rpc_settings.write() {
                    *lock = services::server::RpcServerSettings {
                        rpc_mode: settings.rpc_mode,
                        rpc_host: settings.rpc_host.clone(),
                        rpc_port: settings.rpc_port,
                        rpc_token: settings.rpc_token.clone(),
                    };
                }
            }
        }
        Ok(())
    });

    debug!("Initiating clean shutdown of all background threads...");

    // 1. Signal shutdown to all hook listeners/supervisors
    #[cfg(windows)]
    {
        hook::stop_windows_supervisor();
        // Join the supervisor thread
        let handle = match supervisor_handle.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(h) = handle {
            let res = h.join();
            if let Err(e) = res {
                error!("Error joining supervisor thread: {:?}", e);
            }
        }
    }
    #[cfg(not(windows))]
    {
        hook::stop_listener();
    }

    // 2. Join the hook thread (either the supervisor spawner on Windows, or the hook listener on Unix/macOS)
    let res = hook_thread.join();
    if let Err(e) = res {
        error!("Error joining hook thread: {:?}", e);
    }

    // 3. Stop the clipboard history listener and join its thread
    services::clipboard_history::stop_listener();
    let res = clipboard_thread.join();
    if let Err(e) = res {
        error!("Error joining clipboard thread: {:?}", e);
    }

    // 4. Stop the fullscreen listener (on Windows, join the thread)
    #[cfg(windows)]
    {
        crate::platform::windows::fullscreen::stop_listener();
    }
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::toplevel::stop_listener();
    }

    info!("Service stopped cleanly. Exiting.");
    run_result
}

#[cfg(windows)]
struct NamedPipeConn(tokio::net::windows::named_pipe::NamedPipeServer);

#[cfg(windows)]
impl tokio::io::AsyncRead for NamedPipeConn {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

#[cfg(windows)]
impl tokio::io::AsyncWrite for NamedPipeConn {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

#[cfg(windows)]
impl tonic::transport::server::Connected for NamedPipeConn {
    type ConnectInfo = ();
    fn connect_info(&self) -> Self::ConnectInfo {}
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_tray_module_exists() {
        let paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let enabled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));

        crate::services::tray::spawn(paused, enabled);
    }
}
