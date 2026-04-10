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

fn write_vbs_launcher(exe_path: &std::path::Path) -> std::io::Result<()> {
    let vbs_path = taurine_core::paths::get_startup_vbs_path();
    let exe_str = exe_path.to_string_lossy();
    let vbs_content = format!(
        "Set WshShell = CreateObject(\"WScript.Shell\")\r\n\
         WshShell.Run \"\"\"{}\"\" --daemon\", 0, False\r\n\
         Set WshShell = Nothing\r\n",
        exe_str
    );
    std::fs::write(&vbs_path, vbs_content)?;
    Ok(())
}

fn delete_vbs_launcher() {
    let vbs_path = taurine_core::paths::get_startup_vbs_path();
    if vbs_path.exists()
        && let Err(e) = std::fs::remove_file(&vbs_path)
    {
        debug!("Failed to delete daemon-startup.vbs: {}", e);
    }
}

fn set_autorun(current_exe: &std::path::Path) -> std::io::Result<()> {
    write_vbs_launcher(current_exe)?;
    let vbs_path = taurine_core::paths::get_startup_vbs_path();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
    let (key, _) = hkcu.create_subkey(path)?;

    let val = format!("wscript.exe //B \"{}\"", vbs_path.to_string_lossy());
    key.set_value("Taurine", &val)?;
    Ok(())
}

fn is_autorun_registered() -> bool {
    let vbs_path = taurine_core::paths::get_startup_vbs_path();
    if !vbs_path.exists() {
        return false;
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
    hkcu.open_subkey(path)
        .and_then(|key| key.get_value::<String, _>("Taurine"))
        .is_ok()
}

fn remove_autorun() -> std::io::Result<()> {
    delete_vbs_launcher();

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

pub fn up(start_on_boot: bool) -> taurine_core::error::Result<()> {
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

    if start_on_boot {
        if is_autorun_registered() {
            debug!("Startup hook already registered; skipping.");
        } else {
            debug!("Registering Taurine to start on login...");
            if let Err(e) = set_autorun(&current_exe) {
                error!("Failed to register startup hook: {}", e);
            } else {
                debug!("Startup hook registered.");
            }
        }
    } else {
        debug!("start_on_boot is disabled; removing startup hook if present.");
        if let Err(e) = remove_autorun() {
            debug!("No startup hook to remove (or removal failed): {}", e);
        }
    }

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

pub fn restart(start_on_boot: bool) -> taurine_core::error::Result<()> {
    let mut sys = System::new();
    let current_exe = env::current_exe()?;

    // ── Phase 1: Stop the running daemon (if any) ──────────────────────
    let was_running = is_daemon_running(&mut sys);

    if was_running {
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
                if !is_daemon_running(&mut sys) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }

        // Force-kill if still alive
        if is_daemon_running(&mut sys) {
            debug!("Daemon did not exit gracefully; force-killing for restart.");
            kill_daemon(&mut sys);

            // Brief wait after kill to ensure the port is freed
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // ── Phase 2: Spawn a fresh daemon ──────────────────────────────────
    match Command::new(&current_exe)
        .arg("--daemon")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        Ok(_) => info!("Taurine has been restarted."),
        Err(e) => {
            error!("Failed to restart Taurine: {}", e);
            return Err(e.into());
        }
    }

    // Honour the start_on_boot preference (idempotent — keeps existing
    // hook intact or removes it as appropriate).
    if start_on_boot {
        if !is_autorun_registered() {
            debug!("Registering Taurine to start on login...");
            if let Err(e) = set_autorun(&current_exe) {
                error!("Failed to register startup hook: {}", e);
            }
        }
    } else {
        debug!("start_on_boot is disabled; removing startup hook if present.");
        let _ = remove_autorun();
    }

    Ok(())
}

pub fn status() -> taurine_core::error::Result<()> {
    debug!("Fetching status from daemon via gRPC...");

    if let Ok(rt) = Runtime::new() {
        rt.block_on(async {
            if let Ok(mut client) =
                DaemonControlClient::connect(taurine_core::rpc::DEFAULT_RPC_URL).await
            {
                let request = tonic::Request::new(StatusRequest {});
                match client.get_status(request).await {
                    Ok(res) => {
                        let s = res.into_inner();
                        if s.paused {
                            info!(
                                "Taurine is Paused. Press {} to resume operations",
                                s.pause_hotkey
                            );
                        } else {
                            debug!("Engine status: ONLINE (gRPC)");
                        }
                    }
                    Err(e) => error!("Engine status error via gRPC: {}", e),
                }
            } else {
                debug!("Engine status: OFFLINE (gRPC)");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_vbs_launcher_lifecycle() {
        // Setup tracing + env override for test dir
        taurine_core::logs::init_tracing_for_tests();
        let test_dir = std::env::temp_dir().join("taurine_vbs_lifecycle_test");
        unsafe { std::env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };

        let dummy_exe = PathBuf::from(r"C:\fake\taurine.exe");
        let vbs_path = taurine_core::paths::get_startup_vbs_path();

        // 1. Write the launcher
        write_vbs_launcher(&dummy_exe).expect("Failed to write VBS launcher");
        assert!(vbs_path.exists());

        // Verify contents
        let contents = std::fs::read_to_string(&vbs_path).expect("Failed to read VBS");
        assert!(contents.contains(&dummy_exe.to_string_lossy().into_owned()));
        assert!(contents.contains("--daemon"));
        assert!(contents.contains("WshShell.Run"));

        // 2. Delete the launcher
        delete_vbs_launcher();
        assert!(!vbs_path.exists());

        // Cleanup
        let _ = std::fs::remove_dir_all(&test_dir);
        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
    }
}
