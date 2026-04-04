// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

use std::net::SocketAddr;
use taurine_core::db::init;
use taurine_core::rpc::daemon_control_server::DaemonControlServer;
use tokio::sync::mpsc;
use tonic::transport::Server;
use tracing::{debug, error, info};

mod hook;
mod injector;
mod server;
#[cfg(windows)]
mod win_clipboard;

pub use server::DaemonService;

pub fn start() -> Result<(), Box<dyn std::error::Error>> {
    let conn = init::setup().map_err(|e| {
        error!("Fatal database error during daemon boot: {}", e);
        e
    })?;

    debug!("Daemon initialization complete!");

    // Instantiate our Core Engine State
    use std::sync::{Arc, Mutex};
    use taurine_core::db::crud::{get_all_active_automations, get_setting_value};
    use taurine_core::engine::{EngineState, Evaluator};

    // The trigger_char is stored as a JSON string literal (e.g. `">"`).
    // Deserialize it properly with serde_json to get the raw Rust String.
    let trigger_char = get_setting_value(&conn, "trigger_char")
        .unwrap_or(None)
        .and_then(|json| serde_json::from_str::<String>(&json).ok())
        .and_then(|s| s.chars().next())
        .unwrap_or('>');

    let state = EngineState::new(trigger_char);

    // Load snippets efficiently!
    if let Ok(active) = get_all_active_automations(&conn) {
        let snippets = active
            .into_iter()
            .map(|(trigger, action)| (trigger, action.payload));
        state.load_snippets(snippets);
    }

    let evaluator = Arc::new(Mutex::new(Evaluator::new(Arc::new(state))));

    // Fire up listener in OS thread
    let eval_clone = evaluator.clone();
    std::thread::spawn(move || {
        info!("Starting OS keyboard hook listener...");
        hook::start_listener(eval_clone);
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let (tx, mut rx) = mpsc::channel(1);
        let addr: SocketAddr = "127.0.0.1:50051".parse().unwrap();
        let daemon_service = DaemonService::new(tx);

        info!("Starting gRPC server on {}", addr);
        let server_future = Server::builder()
            .add_service(DaemonControlServer::new(daemon_service))
            .serve_with_shutdown(addr, async {
                let _ = rx.recv().await;
                info!("Shutdown signal received, initiating graceful shutdown.");
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
