use service_manager::{
    ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx, ServiceStatus,
    ServiceStatusCtx, ServiceStopCtx, native_service_manager,
};
use std::env;
use taurine_core::rpc::daemon_control_client::DaemonControlClient;
use taurine_core::rpc::{ShutdownRequest, StatusRequest};
use tokio::runtime::Runtime;
use tracing::{debug, error, info};

const TAURINE_SERVICE_LABEL: &str = "com.ereinaimer.taurine";

fn get_manager() -> Result<Box<dyn ServiceManager>, Box<dyn std::error::Error>> {
    let mut manager = native_service_manager().map_err(|e| {
        error!("Failed to initialize OS user service manager: {}", e);
        e
    })?;

    manager.set_level(ServiceLevel::User).map_err(|e| {
        error!("Failed to set service level: {}", e);
        e
    })?;
    Ok(manager)
}

pub fn up(start_on_boot: bool) -> Result<(), Box<dyn std::error::Error>> {
    let manager = get_manager()?;
    let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse()?;

    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::Running) => {
            info!("Taurine is already running.");
        }
        Ok(ServiceStatus::Stopped(_)) => {
            debug!("Taurine service found but stopped. Starting...");
            manager.start(ServiceStartCtx {
                label: label.clone(),
            })?;
            info!("Taurine started successfully.");
        }
        Ok(ServiceStatus::NotInstalled) | Err(_) => {
            debug!("Taurine service not found. Installing...");

            let current_exe = env::current_exe()?;

            manager.install(ServiceInstallCtx {
                label: label.clone(),
                program: current_exe,
                args: vec!["--daemon".into()],
                contents: None,
                username: None,
                working_directory: None,
                environment: None,
                autostart: start_on_boot,
                restart_policy: Default::default(),
            })?;

            debug!("Install successful. Starting...");
            manager.start(ServiceStartCtx {
                label: label.clone(),
            })?;
            info!("Taurine started successfully.");
        }
    }

    Ok(())
}

pub fn down() -> Result<(), Box<dyn std::error::Error>> {
    debug!("Attempting graceful shutdown via gRPC...");

    let mut grpc_success = false;
    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            if let Ok(mut client) = DaemonControlClient::connect("http://127.0.0.1:50051").await {
                let request = tonic::Request::new(ShutdownRequest {});
                match client.shutdown(request).await {
                    Ok(_) => {
                        info!("Shutdown signal sent successfully.");
                        grpc_success = true;
                    }
                    Err(e) => error!("Failed to send graceful shutdown signal: {}", e),
                }
            } else {
                debug!(
                    "Failed to connect to daemon for graceful shutdown. It may already be stopped."
                );
            }
        });
    }

    // Fallback/Hard stop via service manager for now
    let manager = get_manager()?;
    let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse()?;

    if grpc_success {
        for _ in 0..10 {
            match manager.status(ServiceStatusCtx {
                label: label.clone(),
            }) {
                Ok(ServiceStatus::Stopped(_)) | Ok(ServiceStatus::NotInstalled) | Err(_) => {
                    info!("Taurine is stopped.");
                    return Ok(());
                }
                _ => {}
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::Stopped(_)) | Ok(ServiceStatus::NotInstalled) | Err(_) => {
            info!("Taurine is already stopped.");
            return Ok(());
        }
        _ => {}
    }

    debug!(
        "Graceful shutdown did not terminate the process in time; invoking service manager hard stop as fallback."
    );
    match manager.stop(ServiceStopCtx {
        label: label.clone(),
    }) {
        Ok(_) => info!("Taurine has been stopped (fallback)."),
        Err(e) => error!("Failed to stop service: {}", e),
    }

    Ok(())
}

pub fn restart(start_on_boot: bool) -> Result<(), Box<dyn std::error::Error>> {
    let manager = get_manager()?;
    let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse()?;

    // ── Phase 1: Stop the running daemon (if any) ──────────────────────
    let is_running = matches!(
        manager.status(ServiceStatusCtx {
            label: label.clone(),
        }),
        Ok(ServiceStatus::Running)
    );

    if is_running {
        // Try graceful gRPC shutdown first
        let mut grpc_success = false;
        if let Ok(rt) = Runtime::new() {
            rt.block_on(async {
                if let Ok(mut client) = DaemonControlClient::connect("http://127.0.0.1:50051").await
                {
                    let request = tonic::Request::new(ShutdownRequest {});
                    if client.shutdown(request).await.is_ok() {
                        grpc_success = true;
                    }
                }
            });
        }

        // Wait up to 5 seconds for graceful exit
        if grpc_success {
            for _ in 0..10 {
                match manager.status(ServiceStatusCtx {
                    label: label.clone(),
                }) {
                    Ok(ServiceStatus::Stopped(_)) | Ok(ServiceStatus::NotInstalled) | Err(_) => {
                        break;
                    }
                    _ => {}
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }

        // Force-stop via service manager if still running
        let still_running = matches!(
            manager.status(ServiceStatusCtx {
                label: label.clone(),
            }),
            Ok(ServiceStatus::Running)
        );

        if still_running {
            debug!("Daemon did not exit gracefully; hard-stopping for restart.");
            let _ = manager.stop(ServiceStopCtx {
                label: label.clone(),
            });
            // Brief wait after stop to ensure the port is freed
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // ── Phase 2: Start a fresh daemon ──────────────────────────────────
    // If the service was never installed, install it first.
    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::NotInstalled) | Err(_) => {
            let current_exe = env::current_exe()?;
            manager.install(ServiceInstallCtx {
                label: label.clone(),
                program: current_exe,
                args: vec!["--daemon".into()],
                contents: None,
                username: None,
                working_directory: None,
                environment: None,
                autostart: start_on_boot,
                restart_policy: Default::default(),
            })?;
        }
        _ => {}
    }

    match manager.start(ServiceStartCtx {
        label: label.clone(),
    }) {
        Ok(_) => info!("Taurine has been restarted."),
        Err(e) => {
            error!("Failed to restart Taurine: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}

pub fn status() -> Result<(), Box<dyn std::error::Error>> {
    debug!("Fetching status from daemon via gRPC...");

    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            if let Ok(mut client) = DaemonControlClient::connect("http://127.0.0.1:50051").await {
                let request = tonic::Request::new(StatusRequest {});
                match client.get_status(request).await {
                    Ok(_) => info!("Engine status: ONLINE (gRPC)"),
                    Err(e) => info!("Engine status error via gRPC: {}", e),
                }
            } else {
                info!("Engine status: OFFLINE (gRPC)");
            }
        });
    }

    let manager = get_manager()?;
    let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse()?;

    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::Running) => info!("ONLINE (Running)"),
        Ok(ServiceStatus::Stopped(_)) => info!("OFFLINE (Stopped)"),
        Ok(ServiceStatus::NotInstalled) | Err(_) => info!("NOT INSTALLED"),
    }

    Ok(())
}
