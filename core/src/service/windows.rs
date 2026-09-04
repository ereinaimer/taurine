use std::env;
use std::os::windows::process::CommandExt;
use std::process::Command;

use sysinfo::System;
use tokio::runtime::Runtime;
use tracing::{debug, error, info, warn};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    ITaskService, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::BSTR;
use winreg::RegKey;
use winreg::enums::*;

use crate::rpc::{ShutdownRequest, StatusRequest};

const CREATE_NO_WINDOW: u32 = 0x08000000;
const TASK_NAME: &str = "TaurineStartup";

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
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r#"Software\Microsoft\Windows\CurrentVersion\Run"#;
    let key = hkcu.open_subkey_with_flags(path, KEY_WRITE)?;
    let _ = key.delete_value("Taurine");
    Ok(())
}

fn cleanup_startup_launcher() {
    if !is_task_scheduler_registered() && !is_autorun_registered() {
        delete_startup_launcher();
    }
}

fn get_current_user_identity() -> Option<String> {
    let username = env::var("USERNAME").ok()?;
    if let Ok(domain) = env::var("USERDOMAIN") {
        Some(format!(r"{}\{}", domain, username))
    } else {
        Some(username)
    }
}

fn get_task_xml(exe_path: &std::path::Path, user_id: Option<&str>) -> String {
    let escaped_path = exe_path
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let user_tag = if let Some(uid) = user_id {
        let escaped_uid = uid
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        format!("      <UserId>{}</UserId>\n", escaped_uid)
    } else {
        String::new()
    };

    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Taurine background text expansion service at user logon.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
{user_tag}    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
{user_tag}      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{}</Command>
    </Exec>
  </Actions>
</Task>"#,
        escaped_path
    )
}

fn is_task_scheduler_registered() -> bool {
    // SAFETY: CoInitializeEx initializes COM on the current thread to query the Task Scheduler service.
    // If COM was already initialized, CoInitializeEx returns S_FALSE or RPC_E_CHANGED_MODE, which is safely handled.
    unsafe {
        let init_res = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<bool> {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
            service.Connect(
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
            )?;
            let root_folder = service.GetFolder(&BSTR::from(r"\"))?;
            let task = root_folder.GetTask(&BSTR::from(TASK_NAME));
            Ok(task.is_ok())
        })();
        if init_res.is_ok() {
            CoUninitialize();
        }
        result.unwrap_or(false)
    }
}

fn unregister_task_scheduler() -> windows::core::Result<()> {
    // SAFETY: CoInitializeEx initializes COM on the current thread to delete the registered task from Task Scheduler.
    // CoUninitialize is called upon cleanup if CoInitializeEx succeeded.
    unsafe {
        let init_res = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<()> {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
            service.Connect(
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
            )?;
            let root_folder = service.GetFolder(&BSTR::from(r"\"))?;
            let _ = root_folder.DeleteTask(&BSTR::from(TASK_NAME), 0);
            Ok(())
        })();
        if init_res.is_ok() {
            CoUninitialize();
        }
        result
    }
}

fn register_task_scheduler(current_exe: &std::path::Path) -> windows::core::Result<()> {
    write_startup_launcher(current_exe).map_err(|e| {
        windows::core::Error::new(windows::core::HRESULT(0x80004005u32 as i32), e.to_string())
    })?;
    let exe_path = crate::paths::get_startup_exe_path();
    let user_id = get_current_user_identity();
    let xml = get_task_xml(&exe_path, user_id.as_deref());

    // SAFETY: Initializes COM, connects to the local Task Scheduler service in-process, and
    // registers the logon task using the XML specification. Runs under the interactive user token
    // with least privileges (TASK_RUNLEVEL_LUA) to guarantee non-elevated, safe execution.
    unsafe {
        let init_res = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<()> {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
            service.Connect(
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
            )?;
            let root_folder = service.GetFolder(&BSTR::from(r"\"))?;
            let _ = root_folder.RegisterTask(
                &BSTR::from(TASK_NAME),
                &BSTR::from(&xml),
                TASK_CREATE_OR_UPDATE.0,
                &VARIANT::default(),
                &VARIANT::default(),
                TASK_LOGON_INTERACTIVE_TOKEN,
                &VARIANT::default(),
            )?;
            Ok(())
        })();
        if init_res.is_ok() {
            CoUninitialize();
        }
        result
    }
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
        if is_task_scheduler_registered() || is_autorun_registered() {
            debug!("Startup hook already registered; skipping.");
        } else {
            debug!("Registering Taurine to start on login via Task Scheduler...");
            match register_task_scheduler(&current_exe) {
                Ok(_) => {
                    info!("Startup hook registered via Task Scheduler.");
                    let _ = remove_autorun();
                }
                Err(e) => {
                    warn!(
                        "Task Scheduler registration failed ({}); falling back to registry Run key...",
                        e
                    );
                    set_autorun(&current_exe).map_err(|e| crate::Error::Service(e.to_string()))?;
                    info!("Startup hook registered via registry Run key.");
                }
            }
        }
    } else {
        debug!("Removing startup hooks if present...");
        if let Err(e) = unregister_task_scheduler() {
            debug!(
                "No Task Scheduler task to remove (or removal failed): {}",
                e
            );
        }
        if let Err(e) = remove_autorun() {
            debug!("No startup hook to remove (or removal failed): {}", e);
        } else {
            debug!("Startup hook removed.");
        }
        cleanup_startup_launcher();
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
                    "Failed to connect to service for graceful shutdown. It may already be stopped."
                );
            }
        });
    }

    if let Err(e) = unregister_task_scheduler() {
        debug!(
            "Could not remove Task Scheduler task (was it installed?): {}",
            e
        );
    }
    if let Err(e) = remove_autorun() {
        error!("Could not remove startup hook (was it installed?): {}", e);
    }
    cleanup_startup_launcher();

    let mut sys = System::new();

    if grpc_success {
        for _ in 0..10 {
            if !is_daemon_running(&mut sys) {
                info!("Taurine has been stopped.");
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
            debug!("Service did not exit gracefully; force-killing for restart.");
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

    sync_boot(start_on_boot)?;

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

    #[test]
    fn test_task_xml_structure() {
        let fake_path = std::path::Path::new(r"C:\Program Files\Taurine\taurine-startup.exe");
        let xml = get_task_xml(fake_path, Some(r"DOMAIN\User"));

        assert!(xml.contains("<LogonTrigger>"));
        assert!(xml.contains("<UserId>DOMAIN\\User</UserId>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
        assert!(xml.contains(r"<Command>C:\Program Files\Taurine\taurine-startup.exe</Command>"));
    }
}
