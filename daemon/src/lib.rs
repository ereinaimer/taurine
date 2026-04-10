// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

use std::net::SocketAddr;
use taurine_core::db::init;
use taurine_core::rpc::daemon_control_server::DaemonControlServer;
use tokio::sync::mpsc;
use tonic::transport::Server;
use tracing::{debug, error, info};

mod hook;
mod hotkey;
mod injector;
mod notify;
mod server;
#[cfg(windows)]
mod win_clipboard;

pub use server::DaemonService;

pub fn start() -> taurine_core::error::Result<()> {
    let conn = init::setup()?;

    debug!("Daemon initialization complete!");

    // Instantiate the Core Engine State
    use std::sync::{Arc, Mutex};
    use taurine_core::db::crud::get_all_active_automations;
    use taurine_core::engine::{EngineState, Evaluator};
    use taurine_core::settings::SettingsManager;

    let settings_manager = SettingsManager::new(&conn);
    let settings = settings_manager.load_all();

    let trigger_char = settings.trigger_char;
    let state = Arc::new(EngineState::new(trigger_char));

    // Global pause toggle hotkey (display + parse).
    let pause_hotkey = settings.pause_hotkey.clone();

    let pause_hotkey_spec =
        hotkey::parse_pause_hotkey_setting(&pause_hotkey).unwrap_or_else(|| {
            // Fall back to strict default if DB is malformed or unsupported.
            hotkey::parse_pause_hotkey_setting("Alt + `").expect("default pause hotkey parses")
        });

    let pause_notifications_enabled = settings.pause_notifications_enabled;

    // Load snippets efficiently!
    if let Ok(active) = get_all_active_automations(&conn) {
        let snippets = active
            .into_iter()
            .map(|(trigger, action)| (trigger, action.output));
        state.load_snippets(snippets);
    }

    let evaluator = Arc::new(Mutex::new(Evaluator::new(state.clone())));

    let paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pause_notifications_enabled = Arc::new(std::sync::atomic::AtomicBool::new(
        pause_notifications_enabled,
    ));

    // Fire up listener in OS thread
    let eval_clone = evaluator.clone();
    let paused_clone = paused.clone();
    let pause_notifications_enabled_clone = pause_notifications_enabled.clone();
    std::thread::spawn(move || {
        info!("Starting OS keyboard hook listener...");
        hook::start_listener(
            eval_clone,
            paused_clone,
            pause_notifications_enabled_clone,
            pause_hotkey_spec,
        );
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let (tx, mut rx) = mpsc::channel(1);
        let addr: SocketAddr = "127.0.0.1:50051".parse().unwrap();
        let daemon_service = DaemonService::new(tx, state.clone(), paused.clone(), pause_hotkey);

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

    // The rdev::listen() OS hook thread blocks permanently on a native message
    // loop (SetWindowsHookEx + GetMessage on Windows, similar on Unix) and
    // cannot be unblocked from outside. After the gRPC server has shut down
    // gracefully, the only correct action for a daemon is to exit the process.
    debug!("gRPC server stopped. Exiting daemon process.");
    std::process::exit(0);
}
