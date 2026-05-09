use service_manager::{
    ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx, ServiceStatus,
    ServiceStatusCtx, ServiceStopCtx, native_service_manager,
};
use std::env;
use taurine_core::rpc::daemon_control_client::DaemonControlClient;
use taurine_core::rpc::{ShutdownRequest, StatusRequest};
use tokio::runtime::Runtime;
#[cfg(target_os = "linux")]
use tracing::warn;
use tracing::{debug, error, info};

const TAURINE_SERVICE_LABEL: &str = "com.ereinaimer.taurine";

fn get_manager() -> taurine_core::error::Result<Box<dyn ServiceManager>> {
    let mut manager = native_service_manager().map_err(|e| {
        error!("Failed to initialize OS user service manager: {}", e);
        taurine_core::Error::Service(e.to_string())
    })?;

    manager.set_level(ServiceLevel::User).map_err(|e| {
        error!("Failed to set service level: {}", e);
        taurine_core::Error::Service(e.to_string())
    })?;
    Ok(manager)
}

#[cfg(target_os = "linux")]
fn ensure_linux_permissions() -> taurine_core::error::Result<()> {
    let exe = env::current_exe()?;
    let mut needs_fix = false;
    let mut capability_missing = false;
    let mut group_missing = false;

    // 1. Check Capability
    let cap_output = std::process::Command::new("getcap").arg(&exe).output();
    if let Ok(output) = cap_output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.contains("cap_dac_override") {
            capability_missing = true;
            needs_fix = true;
        }
    }

    // 2. Check Group Membership
    let groups_output = std::process::Command::new("id").arg("-Gn").output();
    if let Ok(output) = groups_output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.split_whitespace().any(|g| g == "input") {
            group_missing = true;
            needs_fix = true;
        }
    }

    if needs_fix {
        info!(
            "Taurine requires additional kernel-level permissions to operate on Linux (Wayland/X11)."
        );

        let mut commands = Vec::new();
        if capability_missing {
            commands.push(format!("setcap cap_dac_override+ep \"{}\"", exe.display()));
        }
        if group_missing {
            let user = std::process::Command::new("id")
                .arg("-un")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "current user".to_string());
            commands.push(format!("usermod -aG input \"{}\"", user));
        }

        let combined_cmd = commands.join(" && ");
        info!(
            "Requesting administrative access to configure: {}",
            combined_cmd
        );

        let status = std::process::Command::new("sudo")
            .arg("sh")
            .arg("-c")
            .arg(&combined_cmd)
            .status();

        match status {
            Ok(s) if s.success() => {
                if group_missing {
                    warn!(
                        "User added to 'input' group. You MUST log out and back in for these changes to take effect."
                    );
                } else {
                    info!("Hardware access permissions granted successfully.");
                }
                // Even with success, we exit if group or cap changed because the current process env is stale.
                return Err(taurine_core::Error::Service(
                    "Permissions updated. Please re-run 'taurine up' after restarting your session.".to_string(),
                ));
            }
            _ => {
                return Err(taurine_core::Error::Service(
                    "Failed to grant hardware access permissions. Taurine cannot start without these privileges.".to_string(),
                ));
            }
        }
    }

    debug!("Linux input permissions verified and active.");
    Ok(())
}

pub fn sync_boot(enabled: bool) -> taurine_core::error::Result<()> {
    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                taurine_core::Error::Service(e.to_string())
            })?;

    // Check if installed.
    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::NotInstalled) | Err(_) => {
            debug!("Taurine service is not installed; skipping boot sync.");
            Ok(())
        }
        _ => {
            debug!(
                "Syncing boot (autostart={}) for installed service...",
                enabled
            );
            let current_exe = env::current_exe()?;

            // To update autostart via service_manager, we reinstall.
            // This is safe as it primarily updates the service configuration files.
            manager
                .install(ServiceInstallCtx {
                    label: label.clone(),
                    program: current_exe,
                    args: vec!["--daemon".into()],
                    contents: None,
                    username: None,
                    working_directory: None,
                    environment: None,
                    autostart: enabled,
                    restart_policy: Default::default(),
                })
                .map_err(|e| taurine_core::Error::Service(e.to_string()))?;
            Ok(())
        }
    }
}

pub fn up(start_on_boot: bool) -> taurine_core::error::Result<()> {
    #[cfg(target_os = "linux")]
    ensure_linux_permissions()?;

    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                taurine_core::Error::Service(e.to_string())
            })?;

    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::Running) => {
            info!("Taurine is already running.");
        }
        Ok(ServiceStatus::Stopped(_)) => {
            debug!("Taurine service found but stopped. Starting...");
            manager
                .start(ServiceStartCtx {
                    label: label.clone(),
                })
                .map_err(|e| taurine_core::Error::Service(e.to_string()))?;
            info!("Taurine started successfully.");
        }
        Ok(ServiceStatus::NotInstalled) | Err(_) => {
            debug!("Taurine service not found. Installing...");

            let current_exe = env::current_exe()?;

            manager
                .install(ServiceInstallCtx {
                    label: label.clone(),
                    program: current_exe,
                    args: vec!["--daemon".into()],
                    contents: None,
                    username: None,
                    working_directory: None,
                    environment: None,
                    autostart: start_on_boot,
                    restart_policy: Default::default(),
                })
                .map_err(|e| taurine_core::Error::Service(e.to_string()))?;

            debug!("Install successful. Starting...");
            manager
                .start(ServiceStartCtx {
                    label: label.clone(),
                })
                .map_err(|e| taurine_core::Error::Service(e.to_string()))?;
            info!("Taurine started successfully.");
        }
    }

    // Ensure boot sync is applied (in case it was already installed but with different flag)
    sync_boot(start_on_boot)?;

    Ok(())
}

pub fn down() -> taurine_core::error::Result<()> {
    debug!("Attempting graceful shutdown via gRPC...");

    let mut grpc_success = false;
    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            if let Ok(mut client) =
                DaemonControlClient::connect(taurine_core::rpc::DEFAULT_RPC_URL).await
            {
                let request = tonic::Request::new(ShutdownRequest {});
                match client.shutdown(request).await {
                    Ok(_) => {
                        debug!("Shutdown signal sent successfully.");
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
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                taurine_core::Error::Service(e.to_string())
            })?;

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

pub fn restart(start_on_boot: bool) -> taurine_core::error::Result<()> {
    #[cfg(target_os = "linux")]
    ensure_linux_permissions()?;

    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                taurine_core::Error::Service(e.to_string())
            })?;

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
                if let Ok(mut client) =
                    DaemonControlClient::connect(taurine_core::rpc::DEFAULT_RPC_URL).await
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
            manager
                .install(ServiceInstallCtx {
                    label: label.clone(),
                    program: current_exe,
                    args: vec!["--daemon".into()],
                    contents: None,
                    username: None,
                    working_directory: None,
                    environment: None,
                    autostart: start_on_boot,
                    restart_policy: Default::default(),
                })
                .map_err(|e| taurine_core::Error::Service(e.to_string()))?;
        }
        _ => {}
    }

    match manager.start(ServiceStartCtx {
        label: label.clone(),
    }) {
        Ok(_) => info!("Taurine has been restarted."),
        Err(e) => {
            error!("Failed to restart Taurine: {}", e);
            return Err(taurine_core::Error::Service(e.to_string()));
        }
    }

    Ok(())
}

pub fn status() -> taurine_core::error::Result<()> {
    let mut grpc_status = None;

    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            if let Ok(mut client) =
                DaemonControlClient::connect(taurine_core::rpc::DEFAULT_RPC_URL).await
            {
                let request = tonic::Request::new(StatusRequest {});
                if let Ok(res) = client.get_status(request).await {
                    grpc_status = Some(res.into_inner());
                }
            }
        });
    }

    if let Some(status) = grpc_status {
        if status.paused {
            info!("Taurine is Paused. Press {} to resume!", status.pause_hotkey);
        } else {
            info!("Taurine is Running.");
        }
        return Ok(());
    }

    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                taurine_core::Error::Service(e.to_string())
            })?;

    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::Running) => info!("Taurine is Running."),
        _ => info!("Taurine is Stopped."),
    }

    Ok(())
}
