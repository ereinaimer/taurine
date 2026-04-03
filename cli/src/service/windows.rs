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
