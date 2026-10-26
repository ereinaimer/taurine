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

/// Blocks on the daemon's `Pause`/`Resume` RPC. `Ok(())` means the
/// daemon confirmed the transition; `Err` carries a message fit for a
/// toast. Blocks, so call it from a synchronous thread.
pub fn set_daemon_paused(paused: bool) -> Result<(), String> {
    if !crate::service::is_service_running() {
        return Err("Taurine is not running.".to_string());
    }

    let action = if paused { "pause" } else { "resume" };
    let perform = async move {
        let mut client = get_client()
            .await
            .map_err(|e| format!("Could not reach Taurine: {e}"))?;
        let response = if paused {
            client
                .pause(tonic::Request::new(PauseRequest {}))
                .await
                .map(|response| response.into_inner().success)
        } else {
            client
                .resume(tonic::Request::new(ResumeRequest {}))
                .await
                .map(|response| response.into_inner().success)
        };
        match response {
            Ok(true) => Ok(()),
            Ok(false) => Err(format!("Taurine refused to {action}.")),
            Err(status) => Err(format!("Could not {action} Taurine: {}", status.message())),
        }
    };

    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt.block_on(perform),
        Err(e) => Err(format!("Could not start the RPC runtime: {e}")),
    }
}

/// Blocks on the daemon's `GetStatus` RPC. `Some(paused)` when the
/// daemon answered, `None` when it is offline or unreachable, so a
/// caller can keep its last known value. Blocks, so call it from a
/// synchronous thread.
pub fn daemon_paused() -> Option<bool> {
    if !crate::service::is_service_running() {
        return None;
    }

    let perform = async {
        let mut client = get_client().await.ok()?;
        let response = client
            .get_status(tonic::Request::new(StatusRequest {}))
            .await
            .ok()?;
        Some(response.into_inner().paused)
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    rt.block_on(perform)
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

    /// Points liveness probes at a name/path nothing holds, so an
    /// offline check never touches a real daemon. The returned temp
    /// dir must outlive the probe on Unix.
    fn force_service_offline() -> Option<tempfile::TempDir> {
        // SAFETY: Tests hold TEST_LOCK while this variable is set.
        #[cfg(target_os = "windows")]
        unsafe {
            std::env::set_var(
                "TAURINE_SERVICE_LIVENESS_NAME",
                "Local\\TaurineTestNonExistent",
            );
        }
        // Non-Windows liveness ignores TAURINE_SERVICE_LIVENESS_NAME and
        // probes a lock file instead, so point it at a fresh temp path.
        // Otherwise the test depends on real filesystem state.
        #[cfg(not(target_os = "windows"))]
        let temp_dir = {
            let temp_dir = tempfile::tempdir().unwrap();
            // SAFETY: Tests hold TEST_LOCK while this variable is set.
            unsafe {
                std::env::set_var(
                    "TAURINE_SERVICE_LIVENESS_PATH",
                    temp_dir
                        .path()
                        .join("taurine_test_nonexistent.lock")
                        .to_string_lossy()
                        .to_string(),
                );
            }
            temp_dir
        };
        #[cfg(target_os = "windows")]
        {
            None
        }
        #[cfg(not(target_os = "windows"))]
        {
            Some(temp_dir)
        }
    }

    fn clear_offline_override() {
        // SAFETY: Tests hold TEST_LOCK while this variable is set.
        #[cfg(target_os = "windows")]
        unsafe {
            std::env::remove_var("TAURINE_SERVICE_LIVENESS_NAME");
        }
        // SAFETY: Tests hold TEST_LOCK while this variable is set.
        #[cfg(not(target_os = "windows"))]
        unsafe {
            std::env::remove_var("TAURINE_SERVICE_LIVENESS_PATH");
        }
    }

    #[test]
    fn test_notify_daemon_reload_skips_when_service_not_running() {
        let _guard = lock();
        let _temp_dir = force_service_offline();
        let start = std::time::Instant::now();
        notify_daemon_reload();
        let elapsed = start.elapsed();
        clear_offline_override();
        assert!(
            elapsed.as_millis() < 50,
            "notify_daemon_reload should return in < 50ms when service is offline, took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_daemon_paused_skips_when_service_not_running() {
        let _guard = lock();
        let _temp_dir = force_service_offline();
        let start = std::time::Instant::now();
        let result = daemon_paused();
        let elapsed = start.elapsed();
        clear_offline_override();
        assert_eq!(result, None);
        assert!(
            elapsed.as_millis() < 50,
            "daemon_paused should return in < 50ms when service is offline, took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_set_daemon_paused_skips_when_service_not_running() {
        let _guard = lock();
        let _temp_dir = force_service_offline();
        let start = std::time::Instant::now();
        let result = set_daemon_paused(true);
        let elapsed = start.elapsed();
        clear_offline_override();
        assert_eq!(result, Err("Taurine is not running.".to_string()));
        assert!(
            elapsed.as_millis() < 50,
            "set_daemon_paused should return in < 50ms when service is offline, took {:?}",
            elapsed
        );
    }
}
