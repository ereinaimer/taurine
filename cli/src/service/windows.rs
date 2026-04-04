use std::env;
use std::os::windows::process::CommandExt;
use std::process::Command;
use sysinfo::System;
use taurine_core::rpc::daemon_control_client::DaemonControlClient;
use taurine_core::rpc::{ShutdownRequest, StatusRequest};
use tokio::runtime::Runtime;
use tracing::{debug, error, info};
use winreg::RegKey;
use winreg::enums::*;

const CREATE_NO_WINDOW: u32 = 0x08000000;

/// HKCU Run launches console binaries with a new console window; `taurine up` avoids that via
/// `CREATE_NO_WINDOW`. For logon startup, spawn through PowerShell so the daemon has no visible window.
fn autorun_command_line(current_exe: &std::path::Path) -> String {
    let exe_path = current_exe.to_string_lossy();
    let exe_ps = exe_path.replace('\'', "''");
    format!(
        "powershell.exe -NoProfile -WindowStyle Hidden -Command \"Start-Process -FilePath '{}' -ArgumentList '--daemon' -WindowStyle Hidden\"",
        exe_ps
    )
}

fn set_autorun(current_exe: &std::path::Path) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
    let (key, _) = hkcu.create_subkey(path)?;
    let val = autorun_command_line(current_exe);
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
    debug!("Attempting graceful shutdown via gRPC...");

    let mut grpc_success = false;
    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            if let Ok(mut client) = DaemonControlClient::connect("http://127.0.0.1:50051").await {
                let request = tonic::Request::new(ShutdownRequest {});
                match client.shutdown(request).await {
                    Ok(_) => {
                        info!("Graceful shutdown signal sent successfully.");
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

    if let Err(e) = remove_autorun() {
        error!("Could not remove startup hook (was it installed?): {}", e);
    }

    let mut sys = System::new();

    // Wait for the daemon to stop gracefully (up to 5 seconds)
    if grpc_success {
        for _ in 0..10 {
            if !is_daemon_running(&mut sys) {
                info!("Taurine is stopped.");
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    if !is_daemon_running(&mut sys) {
        info!("Taurine is already stopped.");
        return Ok(());
    }

    debug!(
        "Graceful shutdown did not terminate the process in time; invoking process killer as fallback."
    );
    let killed = kill_daemon(&mut sys);
    if killed > 0 {
        info!("Taurine has been stopped.");
    } else {
        error!("Failed to kill Taurine process. It might still be running.");
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

    let mut sys = System::new();
    if is_daemon_running(&mut sys) {
        info!("Taurine is Running.");
    } else {
        info!("Taurine is Stopped.");
    }

    Ok(())
}
