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

pub async fn connect_to_daemon_with_settings(
    settings: &crate::settings::Settings,
) -> Result<tonic::transport::Channel, tonic::transport::Error> {
    let use_tcp = settings.rpc_mode == crate::settings::RpcMode::Tcp;

    if use_tcp {
        let host = if settings.rpc_host.is_empty() {
            "127.0.0.1"
        } else {
            &settings.rpc_host
        };
        let rpc_url = format!("http://{}:{}", host, settings.rpc_port);
        tonic::transport::Endpoint::from_shared(rpc_url)?
            .connect_timeout(std::time::Duration::from_millis(500))
            .timeout(std::time::Duration::from_secs(2))
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
                .connect_timeout(std::time::Duration::from_millis(500))
                .timeout(std::time::Duration::from_secs(2))
                .connect_with_connector(service_fn(move |_: tonic::transport::Uri| {
                    let socket_path = socket_path.clone();
                    async move {
                        let stream = UnixStream::connect(socket_path).await?;
                        Ok::<_, std::io::Error>(hyper_util::rt::tokio::TokioIo::new(stream))
                    }
                }))
                .await
        }
        #[cfg(target_os = "windows")]
        {
            use tokio::net::windows::named_pipe::ClientOptions;
            use tower::service_fn;

            let pipe_path = std::env::var("TAURINE_PIPE_PATH")
                .unwrap_or_else(|_| r"\\.\pipe\taurine".to_string());

            tonic::transport::Endpoint::try_from("http://[::]:50051")?
                .connect_timeout(std::time::Duration::from_millis(500))
                .timeout(std::time::Duration::from_secs(2))
                .connect_with_connector(service_fn(move |_: tonic::transport::Uri| {
                    let pipe_path = pipe_path.clone();
                    async move {
                        let client = ClientOptions::new().open(pipe_path)?;
                        Ok::<_, std::io::Error>(hyper_util::rt::tokio::TokioIo::new(client))
                    }
                }))
                .await
        }
        #[cfg(not(any(all(unix, not(target_os = "android")), target_os = "windows")))]
        {
            tonic::transport::Endpoint::from_shared(get_rpc_url())?
                .connect_timeout(std::time::Duration::from_millis(500))
                .timeout(std::time::Duration::from_secs(2))
                .connect()
                .await
        }
    }
}

pub async fn connect_to_daemon() -> Result<tonic::transport::Channel, tonic::transport::Error> {
    let settings = if let Ok(conn) = crate::db::get_conn() {
        let manager = crate::settings::SettingsManager::new(&conn);
        manager.load_all()
    } else {
        crate::settings::Settings::default()
    };
    connect_to_daemon_with_settings(&settings).await
}

static FALLBACK_TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn get_rpc_token() -> String {
    if let Ok(entry) = keyring::Entry::new("taurine", "rpc_token") {
        if let Ok(token) = entry.get_password()
            && !token.is_empty()
        {
            return token;
        }

        let new_token = uuid::Uuid::new_v4().to_string();
        if entry.set_password(&new_token).is_ok() {
            if let Ok(stored) = entry.get_password()
                && !stored.is_empty()
            {
                return stored;
            }
            return new_token;
        }
    }

    FALLBACK_TOKEN
        .get_or_init(|| uuid::Uuid::new_v4().to_string())
        .clone()
}

pub fn delete_rpc_token() {
    if let Ok(entry) = keyring::Entry::new("taurine", "rpc_token") {
        let _ = entry.delete_credential();
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

pub async fn get_client_with_settings(
    settings: &crate::settings::Settings,
) -> Result<
    daemon_control_client::DaemonControlClient<
        tonic::service::interceptor::InterceptedService<
            tonic::transport::Channel,
            ClientAuthInterceptor,
        >,
    >,
    tonic::transport::Error,
> {
    let token = get_rpc_token();
    let channel = connect_to_daemon_with_settings(settings).await?;
    let interceptor = ClientAuthInterceptor {
        token,
        use_auth: true,
    };
    Ok(daemon_control_client::DaemonControlClient::with_interceptor(channel, interceptor))
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
    let settings = if let Ok(conn) = crate::db::get_conn() {
        let manager = crate::settings::SettingsManager::new(&conn);
        manager.load_all()
    } else {
        crate::settings::Settings::default()
    };
    get_client_with_settings(&settings).await
}

pub fn notify_daemon_reload() {
    if !crate::service::is_service_running() {
        tracing::debug!("Service is offline; skipping reload notification.");
        return;
    }

    tracing::debug!("Dispatching Reload instruction to service...");

    let perform_reload = async {
        let settings = if let Ok(conn) = crate::db::get_conn() {
            let manager = crate::settings::SettingsManager::new(&conn);
            manager.load_all()
        } else {
            crate::settings::Settings::default()
        };

        // Try primary config (new settings)
        let mut reload_success = false;
        if let Ok(mut client) = get_client_with_settings(&settings).await {
            let req = tonic::Request::new(ReloadRequest {});
            if client.reload(req).await.is_ok() {
                tracing::debug!("Service state reloaded successfully.");
                reload_success = true;
            }
        }

        // Try fallback config (opposite rpc_mode) if primary failed
        if !reload_success {
            let mut fallback_settings = settings.clone();
            fallback_settings.rpc_mode = match settings.rpc_mode {
                crate::settings::RpcMode::Tcp => crate::settings::RpcMode::Socket,
                crate::settings::RpcMode::Socket => crate::settings::RpcMode::Tcp,
            };
            if let Ok(mut client) = get_client_with_settings(&fallback_settings).await {
                let req = tonic::Request::new(ReloadRequest {});
                if client.reload(req).await.is_ok() {
                    tracing::debug!("Service state reloaded successfully via fallback channel.");
                }
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
                    "Failed to create tokio runtime for service notification: {}",
                    e
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use super::*;

    static MOCK_KEYRING: Once = Once::new();

    fn use_mock_keyring() {
        MOCK_KEYRING.call_once(|| {
            keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        });
    }

    #[test]
    fn test_get_rpc_token_returns_non_empty_token() {
        use_mock_keyring();
        let token = get_rpc_token();
        assert!(!token.trim().is_empty());
    }

    #[test]
    fn test_delete_rpc_token_runs_without_panic() {
        use_mock_keyring();
        delete_rpc_token();
    }

    #[test]
    fn test_notify_daemon_reload_skips_when_service_not_running() {
        use_mock_keyring();
        let start = std::time::Instant::now();
        notify_daemon_reload();
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 50,
            "notify_daemon_reload should return in < 50ms when service is offline, took {:?}",
            elapsed
        );
    }
}
