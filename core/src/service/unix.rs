use service_manager::{
    ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx, ServiceStatus,
    ServiceStatusCtx, ServiceStopCtx, native_service_manager,
};
use std::env;

use tokio::runtime::Runtime;
#[cfg(target_os = "linux")]
use tracing::warn;
use tracing::{debug, error, info};

use crate::rpc::daemon_control_client::DaemonControlClient;
use crate::rpc::{ShutdownRequest, StatusRequest};

const TAURINE_SERVICE_LABEL: &str = "com.ereinaimer.taurine";

fn get_manager() -> crate::error::Result<Box<dyn ServiceManager>> {
    let mut manager = native_service_manager().map_err(|e| {
        error!("Failed to initialize OS user service manager: {}", e);
        crate::Error::Service(e.to_string())
    })?;

    manager.set_level(ServiceLevel::User).map_err(|e| {
        error!("Failed to set service level: {}", e);
        crate::Error::Service(e.to_string())
    })?;
    Ok(manager)
}

#[cfg(target_os = "linux")]
fn ensure_linux_permissions() -> crate::error::Result<()> {
    let exe = env::current_exe()?;
    let mut needs_fix = false;
    let mut capability_missing = false;
    let mut group_missing = false;

    let cap_output = std::process::Command::new("getcap").arg(&exe).output();
    match cap_output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.contains("cap_dac_override") {
                capability_missing = true;
                needs_fix = true;
            }
        }
        Err(err) => {
            warn!("Failed to probe Linux capabilities with getcap: {}", err);
            capability_missing = true;
            needs_fix = true;
        }
    }

    let groups_output = std::process::Command::new("id").arg("-Gn").output();
    match groups_output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.split_whitespace().any(|g| g == "input") {
                group_missing = true;
                needs_fix = true;
            }
        }
        Err(err) => {
            warn!("Failed to probe Linux groups with id -Gn: {}", err);
            group_missing = true;
            needs_fix = true;
        }
    }

    if needs_fix {
        info!(
            "Taurine requires additional kernel-level permissions to operate on Linux (Wayland/X11)."
        );

        if capability_missing {
            info!(
                "Requesting administrative access to grant cap_dac_override to {}",
                exe.display()
            );
            let status = std::process::Command::new("sudo")
                .arg("setcap")
                .arg("cap_dac_override+ep")
                .arg(&exe)
                .status()
                .map_err(|err| {
                    crate::Error::Service(format!(
                        "Failed to invoke sudo setcap for {}: {}",
                        exe.display(),
                        err
                    ))
                })?;
            if !status.success() {
                return Err(crate::Error::Service(
                    "Failed to grant hardware access permissions. Taurine cannot start without these privileges.".to_string(),
                ));
            }
        }
        if group_missing {
            let user = std::process::Command::new("id")
                .arg("-un")
                .output()
                .map_err(|err| {
                    crate::Error::Service(format!(
                        "Failed to resolve the current user for Linux input-group setup: {}",
                        err
                    ))
                })?;
            let user = String::from_utf8_lossy(&user.stdout).trim().to_string();
            if user.is_empty() {
                return Err(crate::Error::Service(
                    "Failed to resolve the current user for Linux input-group setup.".to_string(),
                ));
            }

            info!(
                "Requesting administrative access to add {} to the Linux input group",
                user
            );
            let status = std::process::Command::new("sudo")
                .arg("usermod")
                .arg("-aG")
                .arg("input")
                .arg(&user)
                .status()
                .map_err(|err| {
                    crate::Error::Service(format!(
                        "Failed to invoke sudo usermod for {}: {}",
                        user, err
                    ))
                })?;
            if !status.success() {
                return Err(crate::Error::Service(
                    "Failed to grant hardware access permissions. Taurine cannot start without these privileges.".to_string(),
                ));
            }
        }

        if group_missing {
            warn!(
                "User added to 'input' group. You MUST log out and back in for these changes to take effect."
            );
        } else {
            info!("Hardware access permissions granted successfully.");
        }
        return Err(crate::Error::Service(
            "Permissions updated. Please re-run 'taurine up' after restarting your session."
                .to_string(),
        ));
    }

    debug!("Linux input permissions verified and active.");
    Ok(())
}

pub fn sync_boot(enabled: bool) -> crate::error::Result<()> {
    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                crate::Error::Service(e.to_string())
            })?;

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
                .map_err(|e| crate::Error::Service(e.to_string()))?;
            Ok(())
        }
    }
}

pub fn up(start_on_boot: bool) -> crate::error::Result<()> {
    #[cfg(target_os = "linux")]
    ensure_linux_permissions()?;

    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                crate::Error::Service(e.to_string())
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
                .map_err(|e| crate::Error::Service(e.to_string()))?;
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
                .map_err(|e| crate::Error::Service(e.to_string()))?;

            debug!("Install successful. Starting...");
            manager
                .start(ServiceStartCtx {
                    label: label.clone(),
                })
                .map_err(|e| crate::Error::Service(e.to_string()))?;
            info!("Taurine started successfully.");
        }
    }

    sync_boot(start_on_boot)?;

    Ok(())
}

pub fn down() -> crate::error::Result<()> {
    debug!("Attempting graceful shutdown via gRPC...");

    let mut grpc_success = false;
    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            if let Ok(channel) = crate::rpc::connect_to_daemon().await {
                let mut client = DaemonControlClient::new(channel);
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

    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                crate::Error::Service(e.to_string())
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
        Err(e) => {
            error!("Failed to stop service: {}", e);
            return Err(crate::Error::Service(e.to_string()));
        }
    }

    Ok(())
}

pub fn restart(start_on_boot: bool) -> crate::error::Result<()> {
    #[cfg(target_os = "linux")]
    ensure_linux_permissions()?;

    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                crate::Error::Service(e.to_string())
            })?;

    let is_running = matches!(
        manager.status(ServiceStatusCtx {
            label: label.clone(),
        }),
        Ok(ServiceStatus::Running)
    );

    if is_running {
        let mut grpc_success = false;
        if let Ok(rt) = Runtime::new() {
            rt.block_on(async {
                if let Ok(channel) = crate::rpc::connect_to_daemon().await {
                    let mut client = DaemonControlClient::new(channel);
                    let request = tonic::Request::new(ShutdownRequest {});
                    if client.shutdown(request).await.is_ok() {
                        grpc_success = true;
                    }
                }
            });
        }

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
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

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
                .map_err(|e| crate::Error::Service(e.to_string()))?;
        }
        _ => {}
    }

    match manager.start(ServiceStartCtx {
        label: label.clone(),
    }) {
        Ok(_) => info!("Taurine has been restarted."),
        Err(e) => {
            error!("Failed to restart Taurine: {}", e);
            return Err(crate::Error::Service(e.to_string()));
        }
    }

    Ok(())
}

pub fn status() -> crate::error::Result<()> {
    let mut grpc_status = None;

    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            if let Ok(channel) = crate::rpc::connect_to_daemon().await {
                let mut client = DaemonControlClient::new(channel);
                let request = tonic::Request::new(StatusRequest {});
                if let Ok(res) = client.get_status(request).await {
                    grpc_status = Some(res.into_inner());
                }
            }
        });
    }

    if let Some(status) = grpc_status {
        if status.paused {
            info!(
                "Taurine is paused. Press {} to resume!",
                status.pause_hotkey
            );
        } else {
            info!("Taurine is running.");
        }
        return Ok(());
    }

    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                crate::Error::Service(e.to_string())
            })?;

    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::Running) => info!("Taurine is running."),
        _ => info!("Taurine is stopped."),
    }

    Ok(())
}
