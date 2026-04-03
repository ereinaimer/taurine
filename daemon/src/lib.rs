// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

use std::net::SocketAddr;
use taurine_core::db::init;
use taurine_core::rpc::daemon_control_server::DaemonControlServer;
use tokio::sync::mpsc;
use tonic::transport::Server;
use tracing::{debug, error, info};

mod server;
use server::DaemonService;

pub fn start() -> Result<(), Box<dyn std::error::Error>> {
    let _conn = init::setup().map_err(|e| {
        error!("Fatal database error during daemon boot: {}", e);
        e
    })?;

    debug!("Daemon initialization complete!");

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

    Ok(())
}
