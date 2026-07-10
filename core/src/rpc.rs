tonic::include_proto!("taurine");

pub const DEFAULT_RPC_HOST: &str = "127.0.0.1";
pub const DEFAULT_RPC_PORT: u16 = 50051;
pub const DEFAULT_RPC_ADDR_RAW: &str = "127.0.0.1:50051";
pub const DEFAULT_RPC_URL: &str = "http://127.0.0.1:50051";

pub fn get_rpc_port() -> u16 {
    if let Ok(conn) = crate::db::get_conn() {
        let manager = crate::settings::SettingsManager::new(&conn);
        manager.load_all().rpc_port
    } else {
        DEFAULT_RPC_PORT
    }
}

pub fn get_rpc_url() -> String {
    format!("http://127.0.0.1:{}", get_rpc_port())
}

pub async fn connect_to_daemon() -> Result<tonic::transport::Channel, tonic::transport::Error> {
    let settings = if let Ok(conn) = crate::db::get_conn() {
        let manager = crate::settings::SettingsManager::new(&conn);
        manager.load_all()
    } else {
        crate::settings::Settings::default()
    };

    let use_tcp = {
        #[cfg(target_os = "windows")]
        {
            true
        }
        #[cfg(not(target_os = "windows"))]
        {
            settings.rpc_mode == crate::settings::RpcMode::Tcp
        }
    };

    if use_tcp {
        let host = if settings.rpc_host.is_empty() {
            "127.0.0.1"
        } else {
            &settings.rpc_host
        };
        let rpc_url = format!("http://{}:{}", host, settings.rpc_port);
        tonic::transport::Endpoint::from_shared(rpc_url)?
            .connect()
            .await
    } else {
        #[cfg(all(unix, not(target_os = "android")))]
        {
            use std::convert::TryFrom;
            use tokio::net::UnixStream;
            use tower::service_fn;

            let socket_path = crate::paths::get_data_dir().join("taurine.sock");

            tonic::transport::Endpoint::try_from("http://[::]:50051")?
                .connect_with_connector(service_fn(move |_: tonic::transport::Uri| {
                    let socket_path = socket_path.clone();
                    async move { UnixStream::connect(socket_path).await }
                }))
                .await
        }
        #[cfg(not(all(unix, not(target_os = "android"))))]
        {
            tonic::transport::Endpoint::from_shared(get_rpc_url())?
                .connect()
                .await
        }
    }
}

pub fn get_rpc_token() -> String {
    if let Ok(val) = std::env::var("TAURINE_RPC_TOKEN") {
        return val;
    }

    if let Ok(conn) = crate::db::get_conn() {
        let manager = crate::settings::SettingsManager::new(&conn);
        manager.load_all().rpc_token
    } else {
        String::new()
    }
}

#[derive(Debug, Clone)]
pub struct ClientAuthInterceptor {
    token: String,
    use_auth: bool,
}

impl tonic::service::Interceptor for ClientAuthInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        use std::str::FromStr;
        if self.use_auth && !self.token.is_empty() {
            let auth_header = format!("Bearer {}", self.token);
            if let Ok(header_val) = tonic::metadata::MetadataValue::from_str(&auth_header) {
                request.metadata_mut().insert("authorization", header_val);
            }
        }
        Ok(request)
    }
}

pub async fn get_client() -> Result<
    daemon_control_client::DaemonControlClient<
        tonic::service::interceptor::InterceptedService<
            tonic::transport::Channel,
            ClientAuthInterceptor,
        >,
    >,
    tonic::transport::Error,
> {
    #[cfg(not(target_os = "windows"))]
    let settings = if let Ok(conn) = crate::db::get_conn() {
        let manager = crate::settings::SettingsManager::new(&conn);
        manager.load_all()
    } else {
        crate::settings::Settings::default()
    };

    let token = get_rpc_token();

    let use_tcp = {
        #[cfg(target_os = "windows")]
        {
            true
        }
        #[cfg(not(target_os = "windows"))]
        {
            settings.rpc_mode == crate::settings::RpcMode::Tcp
        }
    };

    let channel = connect_to_daemon().await?;

    let interceptor = ClientAuthInterceptor {
        token,
        use_auth: use_tcp,
    };

    Ok(daemon_control_client::DaemonControlClient::with_interceptor(channel, interceptor))
}

pub fn notify_daemon_reload() {
    tracing::debug!("Dispatching Reload instruction to daemon...");

    let perform_reload = async {
        match get_client().await {
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
