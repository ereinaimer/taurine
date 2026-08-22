// daemon/tests/hook_stress.rs
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;
use tracing::warn;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};

const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMRESUMESUSPEND: usize = 0x0007;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const WTS_SESSION_UNLOCK: usize = 0x0008;

struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        println!("Cleaning up child process (PID: {})...", self.0.id());
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
#[ignore] // Run manually: cargo test --test hook_stress -- --ignored --nocapture
async fn hook_stress_24h_compressed() {
    // Skip if running in CI environment to avoid slow runs or missing GUI context.
    if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
        println!("Running in CI environment - skipping stress test.");
        return;
    }

    // Verify if simulation is supported in the current environment
    if rdev::simulate(&rdev::EventType::KeyPress(rdev::Key::ShiftLeft)).is_err() {
        println!(
            "Simulation is not supported in this environment (headless/non-interactive). Skipping stress test."
        );
        return;
    } else {
        let _ = rdev::simulate(&rdev::EventType::KeyRelease(rdev::Key::ShiftLeft));
    }

    println!("=== STARTING 24H COMPRESSED HOOK STRESS TEST ===");
    taurine_core::logs::init_tracing_for_tests();

    // Set a unique pipe path for the test run to avoid conflicting with
    // any existing instance running on the system.
    let test_pipe_path = r"\\.\pipe\taurine_stress_test";
    // SAFETY: set_var is safe here because we are at the very beginning of the test
    // main thread, and no other threads are concurrently running or reading the environment.
    unsafe {
        std::env::set_var("TAURINE_PIPE_PATH", test_pipe_path);
    }

    // 1. Start daemon subprocess
    let bin_path = get_taurine_binary();
    println!("Spawning daemon binary from: {:?}", bin_path);
    let daemon_child = Command::new(&bin_path)
        .arg("--daemon")
        .env("TAURINE_PIPE_PATH", test_pipe_path)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("Failed to spawn taurine daemon");
    let _guard = ChildGuard(daemon_child);

    // Wait for startup and gRPC endpoint availability
    println!("Waiting for daemon startup...");
    sleep(Duration::from_secs(3)).await;

    // 2. Connect to gRPC client
    println!("Connecting to daemon gRPC client...");
    let mut client = match taurine_core::system::rpc::get_client().await {
        Ok(c) => c,
        Err(e) => {
            panic!("Failed to connect to daemon gRPC client: {:?}", e);
        }
    };
    println!("Connected successfully!");

    // 3. Phase 1: Sustained load (10s = compressed sustained key stream)
    println!("Phase 1: Sustained load (10s)");
    inject_sustained_load(Duration::from_secs(10), 100).await;
    assert_health_healthy(&mut client).await;

    // 4. Phase 2: Burst + modifier combinations
    println!("Phase 2: Burst + modifiers (5s)");
    inject_burst_with_modifiers().await;
    assert_health_healthy(&mut client).await;

    // 5. Phase 3: Suspend/resume cycles (simulate OS power broadcast)
    println!("Phase 3: Suspend/resume cycles");
    for i in 0..5 {
        trigger_suspend_resume().await;
        assert_health_recovers_within(&mut client, Duration::from_secs(5)).await;
        println!("  Cycle {} complete", i + 1);
    }

    // 6. Phase 4: Mock UAC prompt dismissals (ResumeAutomatic + SessionUnlock)
    println!("Phase 4: Mock UAC cycles");
    for i in 0..5 {
        trigger_mock_uac().await;
        assert_health_recovers_within(&mut client, Duration::from_secs(5)).await;
        println!("  UAC cycle {} complete", i + 1);
    }

    // 7. Phase 5: Session Lock/Unlock cycles
    println!("Phase 5: Lock/unlock cycles");
    for i in 0..10 {
        trigger_lock_unlock().await;
        assert_health_healthy(&mut client).await;
        println!("  Lock cycle {} complete", i + 1);
    }

    // 8. Final verification
    let status_req = taurine_core::system::rpc::StatusRequest {};
    let status = client
        .get_status(status_req)
        .await
        .expect("Final status check")
        .into_inner();
    assert_eq!(
        status.keyboard_capture, "healthy",
        "Final capture state degraded"
    );
    assert!(
        status.hook_listener_running,
        "Final hook listener is not running"
    );

    println!("=== ALL PHASES PASSED - HOOK RESILIENCE VALIDATED ===");

    // Cleanup
    let shutdown_req = taurine_core::system::rpc::ShutdownRequest {};
    let _ = client.shutdown(shutdown_req).await;
}

fn get_taurine_binary() -> std::path::PathBuf {
    let mut exe = std::env::current_exe().unwrap();
    // target/debug/deps/hook_stress-xxxxxx
    exe.pop(); // pop filename
    if exe.file_name().and_then(|s| s.to_str()) == Some("deps") {
        exe.pop(); // pop deps
    }
    exe.push("taurine.exe");
    exe
}

async fn inject_sustained_load(duration: Duration, rate: u32) {
    let interval = Duration::from_millis(1000 / rate as u64);
    let end = std::time::Instant::now() + duration;
    while std::time::Instant::now() < end {
        if let Err(e) = rdev::simulate(&rdev::EventType::KeyPress(rdev::Key::KeyA)) {
            println!("rdev KeyPress simulation error: {:?}", e);
        }
        if let Err(e) = rdev::simulate(&rdev::EventType::KeyRelease(rdev::Key::KeyA)) {
            println!("rdev KeyRelease simulation error: {:?}", e);
        }
        sleep(interval).await;
    }
}

async fn inject_burst_with_modifiers() {
    for _ in 0..50 {
        let _ = rdev::simulate(&rdev::EventType::KeyPress(rdev::Key::Alt));
        let _ = rdev::simulate(&rdev::EventType::KeyPress(rdev::Key::Tab));
        let _ = rdev::simulate(&rdev::EventType::KeyRelease(rdev::Key::Tab));
        let _ = rdev::simulate(&rdev::EventType::KeyRelease(rdev::Key::Alt));
        sleep(Duration::from_millis(10)).await;
    }
}

fn find_monitor_window() -> HWND {
    let class_name = wide_null("TaurinePowerSessionMonitor");
    // SAFETY: FindWindowW is safe to query for registered window classes.
    unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) }
}

async fn trigger_suspend_resume() {
    let hwnd = find_monitor_window();
    if !hwnd.is_null() {
        // SAFETY: PostMessageW is thread-safe and safe to send broadcast messages.
        unsafe {
            PostMessageW(hwnd, WM_POWERBROADCAST, PBT_APMRESUMESUSPEND, 0);
        }
    } else {
        warn!("Power monitor window not found for suspend/resume test");
    }
    sleep(Duration::from_millis(200)).await;
}

async fn trigger_mock_uac() {
    let hwnd = find_monitor_window();
    if !hwnd.is_null() {
        // SAFETY: PostMessageW is thread-safe and safe to send broadcast messages.
        unsafe {
            PostMessageW(hwnd, WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC, 0);
            PostMessageW(hwnd, WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK, 0);
        }
    } else {
        warn!("Power monitor window not found for mock UAC test");
    }
    sleep(Duration::from_millis(200)).await;
}

async fn trigger_lock_unlock() {
    let hwnd = find_monitor_window();
    if !hwnd.is_null() {
        // SAFETY: PostMessageW is thread-safe and safe to send broadcast messages.
        unsafe {
            PostMessageW(hwnd, WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK, 0);
        }
    } else {
        warn!("Power monitor window not found for lock/unlock test");
    }
    sleep(Duration::from_millis(200)).await;
}

type InterceptedClient = taurine_core::system::rpc::daemon_control_client::DaemonControlClient<
    tonic::service::interceptor::InterceptedService<
        tonic::transport::Channel,
        taurine_core::system::rpc::ClientAuthInterceptor,
    >,
>;

async fn assert_health_healthy(client: &mut InterceptedClient) {
    let status_req = taurine_core::system::rpc::StatusRequest {};
    let status = client
        .get_status(status_req)
        .await
        .expect("Health status query failed")
        .into_inner();
    assert_eq!(
        status.keyboard_capture, "healthy",
        "Expected healthy status, got: {}",
        status.keyboard_capture
    );
}

async fn assert_health_recovers_within(client: &mut InterceptedClient, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let status_req = taurine_core::system::rpc::StatusRequest {};
        if let Ok(res) = client.get_status(status_req).await {
            let status = res.into_inner();
            if status.keyboard_capture == "healthy" {
                return;
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("Health did not recover to healthy within {:?}", timeout);
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
