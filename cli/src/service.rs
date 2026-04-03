#[cfg(not(target_os = "windows"))]
mod unix {
    use service_manager::{
        ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx,
        ServiceStatus, ServiceStatusCtx, ServiceStopCtx, native_service_manager,
    };
    use std::env;
    use tracing::{error, info};

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

    pub fn up() -> Result<(), Box<dyn std::error::Error>> {
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
                    autostart: true,
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
        // Placeholder INFO log until gRPC graceful shutdown is implemented
        debug!("TODO: Issue stop command to Taurine background process via gRPC.");

        // Fallback/Hard stop via service manager for now
        let manager = get_manager()?;
        let label: ServiceLabel = TAURINE_SERVICE_LABEL.parse()?;

        match manager.status(ServiceStatusCtx {
            label: label.clone(),
        }) {
            Ok(ServiceStatus::Stopped(_)) | Ok(ServiceStatus::NotInstalled) | Err(_) => {
                info!("Taurine is already stopped.");
                return Ok(());
            }
            _ => {}
        }

        match manager.stop(ServiceStopCtx {
            label: label.clone(),
        }) {
            Ok(_) => info!("Taurine has been stopped."),
            Err(e) => error!("Failed to stop service: {}", e),
        }

        Ok(())
    }

    pub fn status() -> Result<(), Box<dyn std::error::Error>> {
        // Placeholder INFO log until gRPC status check is implemented
        debug!("TODO: Fetch detailed status (active snippets, uptime) via gRPC.");

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
}

#[cfg(not(target_os = "windows"))]
pub use unix::*;

#[cfg(target_os = "windows")]
mod windows {
    use std::env;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use sysinfo::System;
    use tracing::{debug, error, info};
    use winreg::RegKey;
    use winreg::enums::*;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    fn set_autorun(current_exe: &std::path::Path) -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
        let (key, _) = hkcu.create_subkey(path)?;
        let exe_path = current_exe.to_string_lossy();
        let val = format!("\"{}\" --daemon", exe_path);
        key.set_value("Taurine", &val)?;
        Ok(())
    }

    fn remove_autorun() -> std::io::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
        let key = hkcu.open_subkey_with_flags(path, KEY_WRITE)?;
        let _ = key.delete_value("Taurine");
        Ok(())
    }

    fn is_daemon_running(sys: &mut System) -> bool {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let current_pid = sysinfo::Pid::from_u32(std::process::id());

        for (pid, process) in sys.processes() {
            if *pid == current_pid {
                continue;
            }
            let name = process.name().to_string_lossy().to_lowercase();
            if name == "taurine.exe" || name == "taurine" {
                return true;
            }
        }
        false
    }

    fn kill_daemon(sys: &mut System) -> usize {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let current_pid = sysinfo::Pid::from_u32(std::process::id());
        let mut killed = 0;

        for (pid, process) in sys.processes() {
            if *pid == current_pid {
                continue;
            }
            let name = process.name().to_string_lossy().to_lowercase();
            if (name == "taurine.exe" || name == "taurine") && process.kill() {
                killed += 1;
            }
        }
        killed
    }

    pub fn up() -> Result<(), Box<dyn std::error::Error>> {
        let mut sys = System::new();
        let current_exe = env::current_exe()?;

        if is_daemon_running(&mut sys) {
            info!("Taurine is already running.");
        } else {
            Command::new(&current_exe)
                .arg("--daemon")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()?;
            info!("Taurine started successfully.");
        }

        debug!("Registering Taurine to start on login...");
        if let Err(e) = set_autorun(&current_exe) {
            error!("Failed to register startup hook: {}", e);
        } else {
            debug!("Startup hook registered.");
        }

        Ok(())
    }

    pub fn down() -> Result<(), Box<dyn std::error::Error>> {
        debug!("TODO: Issue stop command to Taurine background process via gRPC.");

        if let Err(e) = remove_autorun() {
            error!("Could not remove startup hook (was it installed?): {}", e);
        }

        let mut sys = System::new();
        if !is_daemon_running(&mut sys) {
            info!("Taurine is already stopped.");
            return Ok(());
        }

        let killed = kill_daemon(&mut sys);
        if killed > 0 {
            info!("Taurine has been stopped.");
        } else {
            error!("Failed to kill Taurine process. It might still be running.");
        }

        Ok(())
    }

    pub fn status() -> Result<(), Box<dyn std::error::Error>> {
        debug!("TODO: Fetch detailed status (active snippets, uptime) via gRPC.");

        let mut sys = System::new();
        if is_daemon_running(&mut sys) {
            info!("Taurine is Running.");
        } else {
            info!("Taurine is Stopped.");
        }

        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use windows::*;
