tonic::include_proto!("taurine");

pub const DEFAULT_RPC_HOST: &str = "127.0.0.1";
pub const DEFAULT_RPC_PORT: u16 = 50051;
pub const DEFAULT_RPC_ADDR_RAW: &str = "127.0.0.1:50051";
pub const DEFAULT_RPC_URL: &str = "http://127.0.0.1:50051";

pub fn notify_daemon_reload() {
    tracing::debug!("Dispatching Reload instruction to daemon...");

    if let Ok(rt) = tokio::runtime::Runtime::new() {
        rt.block_on(async {
            use daemon_control_client::DaemonControlClient;

            match DaemonControlClient::connect(DEFAULT_RPC_URL).await {
                Ok(mut client) => {
                    let req = tonic::Request::new(ReloadRequest {});
                    if let Err(e) = client.reload(req).await {
                        tracing::error!("Daemon reload request failed: {}", e);
                    } else {
                        tracing::info!("Daemon state reloaded successfully.");
                    }
                }
                Err(_) => {
                    tracing::debug!("Daemon is not reachable for reload notification.");
                }
            }
        });
    } else {
        tracing::error!("Failed to create tokio runtime for daemon notification.");
    }
}
