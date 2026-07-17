use service_manager::{
    ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx, ServiceStatus,
    ServiceStatusCtx, ServiceStopCtx, native_service_manager,
};
use std::env;

use tokio::runtime::Runtime;
use tracing::{debug, error, info};

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
fn is_current_user_in_group(group_name: &str) -> bool {
    // SAFETY: We use standard Unix APIs to get the current user's groups
    unsafe {
        let egid = libc::getegid();
        let ngroups = libc::getgroups(0, std::ptr::null_mut());
        if ngroups < 0 {
            return false;
        }

        let mut groups = vec![0; ngroups as usize];
        let res = libc::getgroups(ngroups, groups.as_mut_ptr());
        if res < 0 {
            return false;
        }

        // Add primary/effective group as well
        if !groups.contains(&egid) {
            groups.push(egid);
        }

        let mut name_buf = vec![0; 2048];
        for &gid in &groups {
            let mut grp = std::mem::zeroed::<libc::group>();
            let mut grp_res = std::ptr::null_mut();

            let res = libc::getgrgid_r(
                gid,
                &mut grp,
                name_buf.as_mut_ptr() as *mut libc::c_char,
                name_buf.len(),
                &mut grp_res,
            );

            if res == 0 && !grp_res.is_null() && !grp.gr_name.is_null() {
                let name = std::ffi::CStr::from_ptr(grp.gr_name);
                if let Ok(name_str) = name.to_str() {
                    if name_str == group_name {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn ensure_linux_permissions() -> crate::error::Result<()> {
    use std::io::IsTerminal;

    let mut needs_fix = false;

    // 1. Check udev rules
    let udev_path = std::path::Path::new("/etc/udev/rules.d/99-taurine.rules");
    if !udev_path.exists() {
        needs_fix = true;
    } else {
        match std::fs::read_to_string(udev_path) {
            Ok(content) => {
                if !content.contains("KERNEL==\"uinput\"") || !content.contains("GROUP=\"input\"") {
                    needs_fix = true;
                }
            }
            Err(_) => {
                needs_fix = true;
            }
        }
    }

    // 2. Check input group membership
    if !is_current_user_in_group("input") {
        needs_fix = true;
    }

    if needs_fix {
        tracing::info!("Configuring system permissions for hardware access...");

        let exe = std::env::current_exe()?;

        // Detect if we are in a GUI session (X11 or Wayland) and run from an interactive terminal (non-headless)
        let is_gui = (std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok())
            && std::io::stdin().is_terminal();

        let status = if is_gui {
            tracing::info!("Requesting administrative access...");
            match std::process::Command::new("pkexec")
                .arg(&exe)
                .arg("setup")
                .status()
            {
                Ok(status) => status,
                Err(err) => {
                    tracing::debug!(
                        "Polkit (pkexec) failed or not found: {}. Falling back to sudo...",
                        err
                    );
                    std::process::Command::new("sudo")
                        .arg(&exe)
                        .arg("setup")
                        .status()
                        .map_err(|e| {
                            crate::Error::Service(format!("Failed to invoke sudo: {}", e))
                        })?
                }
            }
        } else {
            tracing::info!("Requesting administrative access...");
            std::process::Command::new("sudo")
                .arg(&exe)
                .arg("setup")
                .status()
                .map_err(|e| crate::Error::Service(format!("Failed to invoke sudo: {}", e)))?
        };

        if !status.success() {
            return Err(crate::Error::Service(
                "Failed to obtain administrative permissions. Taurine cannot start without these privileges.".to_string(),
            ));
        }

        tracing::info!(
            "System permissions configured successfully. Please reboot your computer to apply the changes."
        );
        std::process::exit(0);
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
            if let Ok(mut client) = crate::rpc::get_client().await {
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
                if let Ok(mut client) = crate::rpc::get_client().await {
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
            if let Ok(mut client) = crate::rpc::get_client().await {
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

#[cfg(target_os = "linux")]
pub fn linux_setup() -> crate::error::Result<()> {
    let user = if let Ok(sudo_user) = std::env::var("SUDO_USER") {
        sudo_user
    } else if let Ok(pkexec_uid) = std::env::var("PKEXEC_UID") {
        let output = std::process::Command::new("id")
            .arg("-nu")
            .arg(&pkexec_uid)
            .output()
            .map_err(|err| {
                crate::Error::Service(format!("Failed to resolve pkexec user: {}", err))
            })?;
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        let output = std::process::Command::new("id")
            .arg("-un")
            .output()
            .map_err(|err| {
                crate::Error::Service(format!("Failed to resolve current user: {}", err))
            })?;
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    if user.is_empty() {
        return Err(crate::Error::Service(
            "Failed to resolve current user name.".to_string(),
        ));
    }

    // 1. Add user to input group
    let status = std::process::Command::new("usermod")
        .arg("-aG")
        .arg("input")
        .arg(&user)
        .status()
        .map_err(|err| {
            crate::Error::Service(format!("Failed to add user to input group: {}", err))
        })?;
    if !status.success() {
        return Err(crate::Error::Service(
            "Failed to execute usermod.".to_string(),
        ));
    }

    // 2. Write udev rules
    let rule_content =
        "KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\", OPTIONS+=\"static_node=uinput\"\n";
    std::fs::write("/etc/udev/rules.d/99-taurine.rules", rule_content)
        .map_err(|err| crate::Error::Service(format!("Failed to write udev rule: {}", err)))?;

    // 3. Write Polkit policy file
    let policy_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
"http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <action id="com.ereinaimer.taurine.setup">
    <description>Configure system permissions for Taurine</description>
    <message>Taurine needs administrative access to configure hardware permissions.</message>
    <defaults>
      <allow_any>no</allow_any>
      <allow_inactive>no</allow_inactive>
      <allow_active>auth_admin</allow_active>
    </defaults>
    <annotate key="org.freedesktop.policykit.exec.path">/usr/bin/taurine</annotate>
    <annotate key="org.freedesktop.policykit.exec.path">/usr/local/bin/taurine</annotate>
  </action>
</policyconfig>
"#;
    std::fs::write(
        "/usr/share/polkit-1/actions/com.ereinaimer.taurine.policy",
        policy_content,
    )
    .map_err(|err| crate::Error::Service(format!("Failed to write Polkit policy: {}", err)))?;

    // 4. Reload udev rules
    let _ = std::process::Command::new("udevadm")
        .arg("control")
        .arg("--reload-rules")
        .status();
    let _ = std::process::Command::new("udevadm")
        .arg("trigger")
        .status();

    Ok(())
}
