use service_manager::{
    ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx, ServiceStatus, ServiceStatusCtx,
    ServiceStopCtx, native_service_manager,
};

#[cfg(not(target_os = "linux"))]
use service_manager::ServiceInstallCtx;

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
                if let Ok(name_str) = name.to_str()
                    && name_str == group_name
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn check_user_in_etc_group(group_name: &str) -> bool {
    let mut username = String::new();
    // SAFETY: Using standard Unix APIs to get the current user's name
    unsafe {
        let uid = libc::getuid();
        let pw = libc::getpwuid(uid);
        if !pw.is_null() && !(*pw).pw_name.is_null() {
            let name_cstr = std::ffi::CStr::from_ptr((*pw).pw_name);
            if let Ok(name_str) = name_cstr.to_str() {
                username = name_str.to_string();
            }
        }
    }

    if username.is_empty() {
        username = std::env::var("USER").unwrap_or_default();
    }

    if username.is_empty() {
        return false;
    }

    if let Ok(content) = std::fs::read_to_string("/etc/group") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 4 && parts[0] == group_name {
                let users: Vec<&str> = parts[3].split(',').collect();
                if users.contains(&username.as_str()) {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(target_os = "linux")]
fn ensure_linux_permissions() -> crate::error::Result<()> {
    use std::io::IsTerminal;

    let mut needs_udev_fix = false;

    // 1. Check udev rules
    let udev_path = std::path::Path::new("/etc/udev/rules.d/99-taurine.rules");
    if !udev_path.exists() {
        needs_udev_fix = true;
    } else {
        match std::fs::read_to_string(udev_path) {
            Ok(content) => {
                if !content.contains("KERNEL==\"uinput\"") || !content.contains("GROUP=\"input\"") {
                    needs_udev_fix = true;
                }
            }
            Err(_) => {
                needs_udev_fix = true;
            }
        }
    }

    // 2. Check input group membership
    let active_group = is_current_user_in_group("input");

    if !needs_udev_fix && !active_group && check_user_in_etc_group("input") {
        tracing::info!(
            "System permissions are already configured, but have not been applied to the current session."
        );
        tracing::info!("Please reboot your computer to apply the changes.");
        std::process::exit(0);
    }

    let needs_fix = needs_udev_fix || !active_group;

    if needs_fix {
        tracing::info!("Configuring system permissions for hardware access...");

        let exe = std::env::current_exe()?;
        let canonical_exe = exe.canonicalize().unwrap_or_else(|_| exe.clone());
        if !canonical_exe.exists() || !canonical_exe.is_file() {
            return Err(crate::Error::Service(
                "Invalid executable path for administrative elevation.".to_string(),
            ));
        }

        // Detect if we are in a GUI session (X11 or Wayland) and run from an interactive terminal (non-headless)
        let is_gui = (std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok())
            && std::io::stdin().is_terminal();

        let status = if is_gui {
            tracing::info!("Requesting administrative access...");
            match std::process::Command::new("pkexec")
                .arg(&canonical_exe)
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
                        .arg(&canonical_exe)
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
                .arg(&canonical_exe)
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

#[cfg(target_os = "linux")]
fn linux_unit_path(label: &ServiceLabel) -> Option<std::path::PathBuf> {
    let config_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".config")
    } else {
        return None;
    };
    Some(
        config_dir
            .join("systemd")
            .join("user")
            .join(format!("{}.service", label.to_script_name())),
    )
}

/// Reloads the systemd user manager so unit file edits take effect.
/// Best-effort: ignored when systemd is unavailable (containers, WSL, SSH without a user bus).
#[cfg(target_os = "linux")]
fn systemd_daemon_reload() {
    let _ = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("daemon-reload")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
}

/// Fire-and-forget stop of our systemd unit. Used by the in-daemon tray Quit
/// handler, which cannot use the blocking service-manager API from its event
/// loop and must register a stop job *before* the process self-exits —
/// otherwise `Restart=` resurrects it. Never fails the caller; falls back to
/// the gRPC shutdown the caller already issues when systemd is absent.
#[cfg(target_os = "linux")]
pub fn request_systemd_stop() {
    let service_name = TAURINE_SERVICE_LABEL
        .parse::<ServiceLabel>()
        .map(|label| format!("{}.service", label.to_script_name()))
        .unwrap_or_else(|_| "ereinaimer-taurine.service".to_string());
    if let Err(e) = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("stop")
        .arg(&service_name)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        debug!("systemd stop request failed (non-systemd session?): {}", e);
    }
}

/// Rewrites units written by previous releases with `Restart=always`, which
/// resurrects user-initiated shutdowns. Preserves the autostart symlink.
/// Best-effort: returns false instead of failing the caller's shutdown path.
#[cfg(target_os = "linux")]
fn migrate_linux_restart_policy(label: &ServiceLabel) -> bool {
    let service_path = match linux_unit_path(label) {
        Some(path) => path,
        None => return false,
    };
    let content = match std::fs::read_to_string(&service_path) {
        Ok(content) => content,
        Err(_) => return true, // nothing installed; nothing to migrate
    };
    if !content.contains("Restart=always") {
        return true;
    }
    debug!("Migrating legacy systemd unit (Restart=always -> on-failure)...");
    // Preserve the user's autostart choice across the rewrite.
    let autostart = service_path
        .file_name()
        .and_then(|name| {
            service_path
                .parent()
                .map(|dir| dir.join("default.target.wants").join(name))
        })
        .map(|link| link.exists())
        .unwrap_or(false);
    // linux_direct_install rewrites the unit and reloads systemd.
    linux_direct_install(autostart, label).is_ok()
}

#[cfg(target_os = "linux")]
fn manager_is_running(manager: &Box<dyn ServiceManager>, label: &ServiceLabel) -> bool {
    matches!(
        manager.status(ServiceStatusCtx {
            label: label.clone(),
        }),
        Ok(ServiceStatus::Running)
    )
}

#[cfg(target_os = "linux")]
fn wait_for_manager_stop(
    manager: &Box<dyn ServiceManager>,
    label: &ServiceLabel,
    iters: u32,
) -> bool {
    for _ in 0..iters {
        match manager.status(ServiceStatusCtx {
            label: label.clone(),
        }) {
            Ok(ServiceStatus::Stopped(_)) | Ok(ServiceStatus::NotInstalled) | Err(_) => {
                return true;
            }
            _ => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    false
}

#[cfg(target_os = "linux")]
fn linux_direct_install(autostart: bool, label: &ServiceLabel) -> crate::error::Result<()> {
    let current_exe = env::current_exe()?;
    let exe_path = current_exe.to_string_lossy();
    let service_content = format!(
        "[Unit]\n\
         Description=Taurine\n\n\
         [Service]\n\
         ExecStart=\"{}\" --daemon\n\
         Restart=on-failure\n\
         RestartSec=2\n\n\
         [Install]\n\
         WantedBy=default.target\n",
        exe_path
    );

    let config_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".config")
    } else {
        return Err(crate::Error::Service(
            "Could not determine user config directory (HOME or XDG_CONFIG_HOME not set)"
                .to_string(),
        ));
    };

    let systemd_user_dir = config_dir.join("systemd").join("user");
    std::fs::create_dir_all(&systemd_user_dir).map_err(|e| crate::Error::Service(e.to_string()))?;

    let service_name = format!("{}.service", label.to_script_name());
    let service_path = systemd_user_dir.join(&service_name);
    std::fs::write(&service_path, service_content)
        .map_err(|e| crate::Error::Service(e.to_string()))?;

    let wants_dir = systemd_user_dir.join("default.target.wants");
    std::fs::create_dir_all(&wants_dir).map_err(|e| crate::Error::Service(e.to_string()))?;

    let link_path = wants_dir.join(&service_name);

    if autostart {
        if !link_path.exists() {
            std::os::unix::fs::symlink(&service_path, &link_path).unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to create symlink for {} autostart: {}",
                    service_name,
                    e
                );
            });
        }
    } else {
        if link_path.exists() {
            std::fs::remove_file(&link_path).unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to remove symlink for {} autostart: {}",
                    service_name,
                    e
                );
            });
        }
    }

    systemd_daemon_reload();

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

            #[cfg(target_os = "linux")]
            {
                linux_direct_install(enabled, &label)?;
            }
            #[cfg(not(target_os = "linux"))]
            {
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
            }
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

            #[cfg(target_os = "linux")]
            {
                linux_direct_install(start_on_boot, &label)?;
            }
            #[cfg(not(target_os = "linux"))]
            {
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

#[cfg(target_os = "linux")]
fn confirm_stays_stopped(manager: &Box<dyn ServiceManager>, label: &ServiceLabel) -> bool {
    // A single `Stopped` sample may be the transient gap between the old
    // process exiting and systemd resurrecting it (legacy `Restart=always`
    // restarts in ~100ms by default). Settle past that window before
    // reporting success. Units migrated to `Restart=on-failure` never restart
    // on a clean exit, so this is a cheap one-time wait on a rare path.
    std::thread::sleep(std::time::Duration::from_millis(1200));
    !manager_is_running(manager, label)
}

pub fn down() -> crate::error::Result<()> {
    let manager = get_manager()?;
    let label: ServiceLabel =
        TAURINE_SERVICE_LABEL
            .parse()
            .map_err(|e: <ServiceLabel as std::str::FromStr>::Err| {
                crate::Error::Service(e.to_string())
            })?;

    #[cfg(target_os = "linux")]
    {
        // Heal legacy `Restart=always` units so this and future stops stick.
        let _ = migrate_linux_restart_policy(&label);
        // Stop via a systemd stop job FIRST. Unlike a gRPC self-exit, a stop
        // job never triggers `Restart=` — even on a legacy unit.
        if manager_is_running(&manager, &label) {
            debug!("Requesting service manager stop...");
            if manager
                .stop(ServiceStopCtx {
                    label: label.clone(),
                })
                .is_ok()
                && wait_for_manager_stop(&manager, &label, 10)
                && confirm_stays_stopped(&manager, &label)
            {
                info!("Taurine has been stopped.");
                return Ok(());
            }
            debug!("Manager stop did not settle; falling through to graceful shutdown...");
        }
    }

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
                    "Failed to connect to service for graceful shutdown. It may already be stopped."
                );
            }
        });
    }

    if grpc_success {
        for _ in 0..10 {
            match manager.status(ServiceStatusCtx {
                label: label.clone(),
            }) {
                Ok(ServiceStatus::Stopped(_)) | Ok(ServiceStatus::NotInstalled) | Err(_) => {
                    #[cfg(target_os = "linux")]
                    {
                        if confirm_stays_stopped(&manager, &label) {
                            info!("Taurine has been stopped.");
                            return Ok(());
                        }
                        break;
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        info!("Taurine has been stopped.");
                        return Ok(());
                    }
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
            #[cfg(target_os = "linux")]
            {
                if confirm_stays_stopped(&manager, &label) {
                    info!("Taurine is already stopped.");
                    return Ok(());
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                info!("Taurine is already stopped.");
                return Ok(());
            }
        }
        _ => {}
    }

    debug!(
        "Graceful shutdown did not terminate the process in time; invoking service manager hard stop as fallback."
    );
    match manager.stop(ServiceStopCtx {
        label: label.clone(),
    }) {
        Ok(_) => {
            #[cfg(target_os = "linux")]
            {
                // A legacy unit may have resurrected the process between the
                // gRPC exit and this stop; verify it stays down.
                if wait_for_manager_stop(&manager, &label, 10)
                    && confirm_stays_stopped(&manager, &label)
                {
                    info!("Taurine has been stopped (fallback).");
                    return Ok(());
                }
                error!("Service was stopped but did not stay stopped.");
                return Err(crate::Error::Service(
                    "Taurine did not stay stopped.".to_string(),
                ));
            }
            #[cfg(not(target_os = "linux"))]
            {
                info!("Taurine has been stopped (fallback).");
            }
        }
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

    #[cfg(target_os = "linux")]
    {
        // Keep the installed unit on the current restart policy.
        let _ = migrate_linux_restart_policy(&label);
    }

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
            debug!("Service did not exit gracefully; hard-stopping for restart.");
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
            #[cfg(target_os = "linux")]
            {
                linux_direct_install(start_on_boot, &label)?;
            }
            #[cfg(not(target_os = "linux"))]
            {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_name_matches_label() {
        let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse().unwrap();
        assert_eq!(label.to_script_name(), "ereinaimer-taurine");
        // Verify that the file name used in linux_direct_install will match this expected script name
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_linux_unit_restarts_on_failure_not_always() {
        let _guard = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        // SAFETY: Serialized via TEST_LOCK for test isolation.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", temp_dir.path()) };

        struct EnvGuard(Option<String>);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                // SAFETY: Serialized via TEST_LOCK for test isolation.
                unsafe {
                    match &self.0 {
                        Some(prev) => std::env::set_var("XDG_CONFIG_HOME", prev),
                        None => std::env::remove_var("XDG_CONFIG_HOME"),
                    }
                }
            }
        }
        let _env_guard = EnvGuard(old_xdg);

        let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse().unwrap();
        linux_direct_install(false, &label).unwrap();
        let content = std::fs::read_to_string(linux_unit_path(&label).expect("unit path")).unwrap();
        assert!(
            content.contains("Restart=on-failure"),
            "unit must self-heal crashes via Restart=on-failure"
        );
        assert!(
            !content.contains("Restart=always"),
            "unit must not resurrect user-initiated shutdowns"
        );

        // Legacy units from previous releases migrate in place.
        let path = linux_unit_path(&label).expect("unit path");
        std::fs::write(
            &path,
            "[Service]\nExecStart=\"x\" --daemon\nRestart=always\n",
        )
        .unwrap();
        assert!(migrate_linux_restart_policy(&label));
        let migrated = std::fs::read_to_string(&path).unwrap();
        assert!(migrated.contains("Restart=on-failure"));
        assert!(!migrated.contains("Restart=always"));
    }
}
