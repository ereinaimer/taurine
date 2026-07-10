use std::env;
use std::os::windows::process::CommandExt;
use std::process::Command;

use sysinfo::System;
use tokio::runtime::Runtime;
use tracing::{debug, error, info};
use winreg::RegKey;
use winreg::enums::*;

use crate::rpc::{ShutdownRequest, StatusRequest};

const CREATE_NO_WINDOW: u32 = 0x08000000;

const STARTUP_RUNNER_BYTES: &[u8] = include_bytes!(env!("STARTUP_RUNNER_PATH"));

fn write_startup_launcher(current_exe: &std::path::Path) -> std::io::Result<()> {
    let exe_path = crate::paths::get_startup_exe_path();
    std::fs::write(&exe_path, STARTUP_RUNNER_BYTES)?;

    let path_file = exe_path.with_extension("path");
    std::fs::write(&path_file, current_exe.to_string_lossy().as_bytes())?;

    Ok(())
}

fn delete_startup_launcher() {
    let exe_path = crate::paths::get_startup_exe_path();
    if exe_path.exists()
        && let Err(e) = std::fs::remove_file(&exe_path)
    {
        debug!("Failed to delete taurine-startup.exe: {}", e);
    }

    let path_file = exe_path.with_extension("path");
    if path_file.exists()
        && let Err(e) = std::fs::remove_file(&path_file)
    {
        debug!("Failed to delete taurine-startup.path: {}", e);
    }
}

fn set_autorun(current_exe: &std::path::Path) -> std::io::Result<()> {
    write_startup_launcher(current_exe)?;
    let exe_path = crate::paths::get_startup_exe_path();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
    let (key, _) = hkcu.create_subkey(path)?;

    let val = format!("\"{}\"", exe_path.to_string_lossy());
    key.set_value("Taurine", &val)?;
    Ok(())
}

fn is_autorun_registered() -> bool {
    let exe_path = crate::paths::get_startup_exe_path();
    if !exe_path.exists() {
        return false;
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
    hkcu.open_subkey(path)
        .and_then(|key| key.get_value::<String, _>("Taurine"))
        .is_ok()
}

fn remove_autorun() -> std::io::Result<()> {
    delete_startup_launcher();

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

pub fn sync_boot(enabled: bool) -> crate::error::Result<()> {
    let current_exe = env::current_exe()?;
    if enabled {
        if is_autorun_registered() {
            debug!("Startup hook already registered; skipping.");
        } else {
            debug!("Registering Taurine to start on login...");
            set_autorun(&current_exe).map_err(|e| crate::Error::Service(e.to_string()))?;
            debug!("Startup hook registered.");
        }
    } else {
        debug!("Removing startup hook if present...");
        if let Err(e) = remove_autorun() {
            debug!("No startup hook to remove (or removal failed): {}", e);
        } else {
            debug!("Startup hook removed.");
        }
    }
    Ok(())
}

pub fn up(start_on_boot: bool) -> crate::error::Result<()> {
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

    if let Err(e) = remove_autorun() {
        error!("Could not remove startup hook (was it installed?): {}", e);
    }

    let mut sys = System::new();

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

pub fn restart(start_on_boot: bool) -> crate::error::Result<()> {
    let mut sys = System::new();
    let current_exe = env::current_exe()?;

    let was_running = is_daemon_running(&mut sys);

    if was_running {
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
                if !is_daemon_running(&mut sys) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }

        if is_daemon_running(&mut sys) {
            debug!("Daemon did not exit gracefully; force-killing for restart.");
            kill_daemon(&mut sys);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

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

    let mut sys = System::new();
    if is_daemon_running(&mut sys) {
        info!("Taurine is running.");
    } else {
        info!("Taurine is stopped.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_startup_launcher_lifecycle() {
        let _guard = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::logs::init_tracing_for_tests();
        let test_dir = std::env::temp_dir().join("taurine_exe_lifecycle_test");
        unsafe { std::env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };

        let exe_path = crate::paths::get_startup_exe_path();

        let current_exe = std::env::current_exe().unwrap();
        write_startup_launcher(&current_exe).expect("Failed to write startup launcher");
        assert!(exe_path.exists());

        let contents = std::fs::read(&exe_path).expect("Failed to read exe");
        assert!(!contents.is_empty());

        delete_startup_launcher();
        assert!(!exe_path.exists());

        let _ = std::fs::remove_dir_all(&test_dir);
        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
    }
}
