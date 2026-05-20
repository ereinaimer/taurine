// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use taurine_core::db::init;
use taurine_core::rpc::daemon_control_server::DaemonControlServer;
use tokio::sync::mpsc;
use tonic::transport::Server;
use tracing::{debug, error, info};

mod audio;
mod clipboard_history;
mod engine;
mod hook;
mod hook_health;
mod hotkey;
mod hotkey_evaluator;
mod injector;
mod notify;
pub mod platform;
mod server;

pub use server::DaemonService;

pub fn start() -> taurine_core::error::Result<()> {
    let conn = init::setup()?;

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = crate::platform::linux::init() {
            error!("Linux platform initialization failed: {}", e);
            return Err(taurine_core::error::Error::Service(e));
        }
    }

    debug!("Daemon initialization complete!");

    // Instantiate the Core Engine State
    use std::sync::{Arc, Mutex, RwLock};
    use taurine_core::db::crud::{
        get_active_word_trigger_history, get_all_active_automations,
        get_all_active_hotkey_automations,
    };
    use taurine_core::engine::{EngineState, Evaluator};
    use taurine_core::settings::SettingsManager;

    let settings_manager = SettingsManager::new(&conn);
    let settings = settings_manager.load_all();

    let trigger_char = settings.trigger_char;
    let state = Arc::new(EngineState::new(trigger_char));
    state
        .inline_tab_completion_enabled
        .store(settings.inline_tab_completion_enabled, Ordering::Relaxed);
    state
        .inline_history_enabled
        .store(settings.inline_history_enabled, Ordering::Relaxed);

    // Global pause toggle hotkey (display + parse).
    let pause_hotkey = Arc::new(RwLock::new(settings.pause_hotkey.clone()));

    let pause_hotkey_spec = Arc::new(RwLock::new(
        hotkey::parse_pause_hotkey_setting(&settings.pause_hotkey).unwrap_or_else(|| {
            // Fall back to strict default if DB is malformed or unsupported.
            hotkey::parse_pause_hotkey_setting("Alt + `").expect("default pause hotkey parses")
        }),
    ));

    let pause_notifications_enabled = settings.pause_notifications_enabled;
    let spinner_style = Arc::new(RwLock::new(settings.spinner_style));

    // Load snippets efficiently!
    if let Ok(active) = get_all_active_automations(&conn) {
        state.load_actions(active);
    }
    if let Ok(history) = get_active_word_trigger_history(&conn) {
        state.load_word_trigger_history(history);
    }
    if let Ok(active_hotkeys) = get_all_active_hotkey_automations(&conn) {
        state.load_hotkey_actions(active_hotkeys);
    }

    // Load AI presets!
    if let Ok(presets) = taurine_core::db::crud::ai_presets::list_presets(&conn) {
        state.load_ai_presets(presets.into_iter().map(|p| (p.name, p.prompt)));
    }

    let evaluator = Arc::new(Mutex::new(Evaluator::new(state.clone())));

    let paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pause_notifications_enabled = Arc::new(std::sync::atomic::AtomicBool::new(
        pause_notifications_enabled,
    ));
    let pause_audio_enabled = Arc::new(std::sync::atomic::AtomicBool::new(
        settings.pause_audio_enabled,
    ));
    let hook_health = hook_health::HookHealth::new();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let runtime_handle = rt.handle().clone();

    let audio_tx = audio::init_audio_system();

    std::thread::spawn(|| {
        info!("Starting clipboard history listener...");
        clipboard_history::start_listener();
    });

    // Fire up listener in OS thread
    let eval_clone = evaluator.clone();
    let state_clone = state.clone();
    let paused_clone = paused.clone();
    let pause_notifications_enabled_clone = pause_notifications_enabled.clone();
    let pause_hotkey_spec_clone = pause_hotkey_spec.clone();
    let spinner_style_clone = spinner_style.clone();
    let runtime_handle_clone = runtime_handle.clone();
    let pause_audio_enabled_clone = pause_audio_enabled.clone();
    let audio_tx_clone = audio_tx.clone();
    #[cfg(windows)]
    let hook_health_clone = hook_health.clone();
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            info!("Starting supervised Windows keyboard hook listener...");
            hook::start_windows_supervisor(
                eval_clone,
                state_clone,
                paused_clone,
                pause_notifications_enabled_clone,
                pause_hotkey_spec_clone,
                spinner_style_clone,
                runtime_handle_clone,
                pause_audio_enabled_clone,
                audio_tx_clone,
                hook_health_clone,
            );
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
                runtime_handle_clone,
                pause_audio_enabled_clone,
                audio_tx_clone,
            );
        }
    });

    rt.block_on(async {
        let (tx, mut rx) = mpsc::channel(1);
        let addr: SocketAddr = taurine_core::rpc::DEFAULT_RPC_ADDR_RAW.parse().unwrap();
        let daemon_service = DaemonService::new(
            tx,
            state.clone(),
            paused.clone(),
            pause_notifications_enabled.clone(),
            pause_hotkey_spec.clone(),
            pause_hotkey.clone(),
            spinner_style.clone(),
            pause_audio_enabled.clone(),
            hook_health.clone(),
        );

        info!("Starting gRPC server on {}", addr);
        let server_future = Server::builder()
            .add_service(DaemonControlServer::new(daemon_service))
            .serve_with_shutdown(addr, async {
                let _ = rx.recv().await;
                info!("Shutdown signal received, Initiating shutdown...");
            });

        if let Err(e) = server_future.await {
            error!("gRPC server failed: {}", e);
        }
    });

    // The rdev::grab() OS hook thread blocks permanently on a native message
    // loop (SetWindowsHookEx + GetMessage on Windows, similar on Unix) and
    // cannot be unblocked from outside. After the gRPC server has shut down
    // gracefully, the only correct action for a daemon is to exit the process.
    debug!("gRPC server stopped. Exiting daemon process.");
    std::process::exit(0);
}
