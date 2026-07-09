tonic::include_proto!("taurine");

pub const DEFAULT_RPC_HOST: &str = "127.0.0.1";
pub const DEFAULT_RPC_PORT: u16 = 50051;
pub const DEFAULT_RPC_ADDR_RAW: &str = "127.0.0.1:50051";
pub const DEFAULT_RPC_URL: &str = "http://127.0.0.1:50051";

pub fn get_rpc_port() -> u16 {
    if let Ok(conn) = rusqlite::Connection::open(crate::paths::get_db_path()) {
        let manager = crate::settings::SettingsManager::new(&conn);
        manager.load_all().rpc_port
    } else {
        DEFAULT_RPC_PORT
    }
}

pub fn get_rpc_url() -> String {
    format!("http://127.0.0.1:{}", get_rpc_port())
}

pub fn notify_daemon_reload() {
    tracing::debug!("Dispatching Reload instruction to daemon...");

    let perform_reload = async {
        use daemon_control_client::DaemonControlClient;

        match DaemonControlClient::connect(get_rpc_url()).await {
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
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(perform_reload);
    } else {
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => {
                rt.block_on(perform_reload);
            }
            Err(e) => {
                tracing::error!(
                    "Failed to create tokio runtime for daemon notification: {}",
                    e
                );
            }
        }
    }
}
