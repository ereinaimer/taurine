tonic::include_proto!("taurine");

/// Control traffic travels over OS-local transports only: a `0600` Unix socket on
/// Unix, a same-user named pipe on Windows. No TCP, no auth token; other local
/// users are blocked by transport permissions enforced by the kernel.
pub async fn connect_to_daemon() -> Result<tonic::transport::Channel, tonic::transport::Error> {
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

        let pipe_path = crate::paths::dev_env_var("TAURINE_PIPE_PATH")
            .unwrap_or_else(|| r"\\.\pipe\taurine".to_string());

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
        tonic::transport::Endpoint::from_shared("http://127.0.0.1:50051")?
            .connect_timeout(std::time::Duration::from_millis(500))
            .timeout(std::time::Duration::from_secs(2))
            .connect()
            .await
    }
}

pub async fn get_client() -> Result<
    daemon_control_client::DaemonControlClient<tonic::transport::Channel>,
    tonic::transport::Error,
> {
    let channel = connect_to_daemon().await?;
    Ok(daemon_control_client::DaemonControlClient::new(channel))
}

pub fn notify_daemon_reload() {
    if !crate::service::is_service_running() {
        tracing::debug!("Service is offline; skipping reload notification.");
        return;
    }

    tracing::debug!("Dispatching Reload instruction to service...");

    let perform_reload = async {
        if let Ok(mut client) = get_client().await {
            let req = tonic::Request::new(ReloadRequest {});
            if client.reload(req).await.is_ok() {
                tracing::debug!("Service state reloaded successfully.");
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
    use super::*;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn test_notify_daemon_reload_skips_when_service_not_running() {
        let _guard = lock();
        // SAFETY: Single-threaded unit test modifying environment variable for isolation.
        unsafe {
            std::env::set_var(
                "TAURINE_SERVICE_LIVENESS_NAME",
                "Local\\TaurineTestNonExistent",
            );
        }
        let start = std::time::Instant::now();
        notify_daemon_reload();
        let elapsed = start.elapsed();
        // SAFETY: Single-threaded unit test cleaning up environment variable.
        unsafe {
            std::env::remove_var("TAURINE_SERVICE_LIVENESS_NAME");
        }
        assert!(
            elapsed.as_millis() < 50,
            "notify_daemon_reload should return in < 50ms when service is offline, took {:?}",
            elapsed
        );
    }
}
