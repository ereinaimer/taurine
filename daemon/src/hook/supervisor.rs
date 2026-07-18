#![cfg(windows)]

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::runtime::Handle;
use tracing::{error, info, warn};

use crate::hook_health::HookHealth;
use crate::hotkey;
use taurine_core::engine::Evaluator;

use super::listener::{LISTENER_EPOCH, spawn_windows_hook_listener};

#[derive(Debug, Clone)]
pub enum WindowsSupervisorEvent {
    ResumeAutomatic,
    ResumeFromSuspend,
    SessionUnlock,
    SessionLogon,
    DisplayChange,
    ListenerExited { error: Option<String> },
    Shutdown,
}

static SUPERVISOR_SENDER: std::sync::OnceLock<Mutex<Option<mpsc::Sender<WindowsSupervisorEvent>>>> =
    std::sync::OnceLock::new();

pub(super) struct ListenerHandle {
    pub(super) join: std::thread::JoinHandle<()>,
    pub(super) thread_id: u32,
}

#[allow(clippy::too_many_arguments)]
pub fn start_windows_supervisor(
    evaluator: Arc<Mutex<Evaluator>>,
    state: Arc<taurine_core::engine::EngineState>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    pause_audio_enabled: Arc<std::sync::atomic::AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
    pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
    hook_health: HookHealth,
) -> Option<std::thread::JoinHandle<()>> {
    let (tx, rx) = mpsc::channel::<WindowsSupervisorEvent>();

    if let Ok(mut lock) = SUPERVISOR_SENDER.get_or_init(|| Mutex::new(None)).lock() {
        *lock = Some(tx.clone());
    }

    if let Err(error) = crate::platform::windows::power::start_listener(tx.clone()) {
        error!(
            error = %error,
            "Failed to start Windows power/session monitor"
        );
    }

    let spawn_result = std::thread::Builder::new()
        .name("tau-hook-super".to_string())
        .spawn(move || {
            const RESTART_BACKOFF: Duration = Duration::from_secs(2);
            // Fast-failure threshold: if a listener exits within this many ms of
            // being spawned, it counts as a startup failure and gets a shorter
            // retry delay rather than the full RESTART_BACKOFF.
            const FAST_FAILURE_THRESHOLD_MS: u64 = 5000;

            let supervisor_start_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let mut listener_handle: Option<ListenerHandle> = Some(spawn_windows_hook_listener(
                evaluator.clone(),
                state.clone(),
                paused.clone(),
                pause_notifications_enabled.clone(),
                pause_hotkey.clone(),
                spinner_style.clone(),
                pause_audio_enabled.clone(),
                audio_tx.clone(),
                pause_transition_tx.clone(),
                hook_health.clone(),
                tx.clone(),
            ));
            let mut listener_spawned_at_unix_ms: u64 = supervisor_start_ms;
            let mut consecutive_fast_failures: u32 = 0;
            let mut last_ping_sent_at_unix_ms: u64 = 0;
            let mut ping_pending = false;
            let mut force_ping_at_unix_ms: u64 = 0;
            let mut next_spawn_allowed_after = std::time::Instant::now();

            loop {
                let event = rx.recv_timeout(Duration::from_millis(100));

                match event {
                    Ok(WindowsSupervisorEvent::ListenerExited { error }) => {
                        hook_health.mark_listener_exit(error.clone());

                        if let Some(ref error) = error {
                            error!(error = %error, "Windows hook listener exited");
                        } else {
                            warn!("Windows hook listener exited without an error");
                        }

                        listener_handle.take();

                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        let is_fast_failure = listener_spawned_at_unix_ms > 0
                            && now < listener_spawned_at_unix_ms + FAST_FAILURE_THRESHOLD_MS;

                        if is_fast_failure {
                            consecutive_fast_failures += 1;
                            if consecutive_fast_failures <= 3 {
                                warn!(
                                    consecutive_fast_failures,
                                    "Hook listener fast failure; retrying in 1s"
                                );
                                std::thread::sleep(Duration::from_secs(1));
                            } else {
                                error!(
                                    consecutive_fast_failures,
                                    "Hook listener failed repeatedly on startup; backing off 5s"
                                );
                                std::thread::sleep(Duration::from_secs(5));
                            }
                        } else {
                            consecutive_fast_failures = 0;
                            std::thread::sleep(RESTART_BACKOFF);
                            info!("Restarting Windows hook listener after backoff");
                        }
                    }
                    Ok(WindowsSupervisorEvent::ResumeAutomatic) => {
                        hook_health.mark_recovery_signal("automatic resume");
                        warn!("Windows automatic resume detected; tearing down stale hook listener");
                        tear_down_listener(&mut listener_handle);
                        // Coalesce sequential wakeup/session events by setting a 1s delay
                        // before the supervisor is allowed to spawn a new listener.
                        next_spawn_allowed_after = std::time::Instant::now() + Duration::from_secs(1);
                        force_ping_at_unix_ms = 0;
                        consecutive_fast_failures = 0;
                    }
                    Ok(WindowsSupervisorEvent::ResumeFromSuspend) => {
                        hook_health.mark_recovery_signal("resume from suspend");
                        warn!("Windows resume from suspend detected; tearing down stale hook listener");
                        tear_down_listener(&mut listener_handle);
                        next_spawn_allowed_after = std::time::Instant::now() + Duration::from_secs(1);
                        force_ping_at_unix_ms = 0;
                        consecutive_fast_failures = 0;
                    }
                    Ok(WindowsSupervisorEvent::SessionUnlock) => {
                        hook_health.mark_recovery_signal("session unlock");
                        warn!("Windows session unlock detected; tearing down stale hook listener");
                        tear_down_listener(&mut listener_handle);
                        next_spawn_allowed_after = std::time::Instant::now() + Duration::from_secs(1);
                        force_ping_at_unix_ms = 0;
                        consecutive_fast_failures = 0;
                    }
                    Ok(WindowsSupervisorEvent::SessionLogon) => {
                        hook_health.mark_recovery_signal("session logon");
                        warn!("Windows session logon detected; tearing down stale hook listener");
                        tear_down_listener(&mut listener_handle);
                        next_spawn_allowed_after = std::time::Instant::now() + Duration::from_secs(1);
                        force_ping_at_unix_ms = 0;
                        consecutive_fast_failures = 0;
                    }
                    Ok(WindowsSupervisorEvent::DisplayChange) => {
                        hook_health.mark_recovery_signal("display change");
                        warn!("Windows display change detected; scheduling liveness verification");
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        force_ping_at_unix_ms = now + 3000;
                    }
                    Ok(WindowsSupervisorEvent::Shutdown) => {
                        info!("Windows hook supervisor received Shutdown event");
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // Watchdog timer: periodically check listener health.
                        // Compute restart decision without holding a borrow on
                        // listener_handle so tear_down_listener can borrow it mutably.
                        let mut needs_restart = false;

                        if let Some(ref handle) = listener_handle {
                            let snapshot = hook_health.snapshot();
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;

                            // Check 1: Startup/Reinstall Hang — listener started
                            // but hasn't entered rdev::grab within 3 seconds.
                            let startup_hang = snapshot.hook_thread_started_at_unix_ms > 0
                                && snapshot.hook_entered_grab_at_unix_ms == 0
                                && now >= snapshot.hook_thread_started_at_unix_ms + 3000;

                            // Check 2: Silent Thread Termination — thread exited
                            // without sending ListenerExited.
                            let silent_exit = handle.join.is_finished();

                            // Check 3: Hook seems stale — hook entered grab, but last keyboard event is too old
                            // and we are not currently awaiting a recovery.
                            let seems_stale = snapshot.hook_entered_grab_at_unix_ms > 0
                                && snapshot.last_keyboard_event_at_unix_ms > 0
                                && now >= snapshot.last_keyboard_event_at_unix_ms + 300_000
                                && snapshot.pending_recovery_reason.is_none();

                            let scheduled_ping_due = force_ping_at_unix_ms > 0 && now >= force_ping_at_unix_ms;
                            let should_ping = seems_stale || scheduled_ping_due;

                            if startup_hang {
                                warn!(
                                    "Watchdog: hook listener started but hasn't entered grab after 3s; restarting"
                                );
                                hook_health.mark_recovery_signal("watchdog: startup hang");
                            }
                            if silent_exit {
                                warn!(
                                    "Watchdog: hook listener thread silently terminated; restarting"
                                );
                                hook_health.mark_recovery_signal("watchdog: silent termination");
                            }

                            needs_restart = startup_hang || silent_exit;

                            if should_ping && !needs_restart {
                                if !ping_pending {
                                    if scheduled_ping_due {
                                        info!("Watchdog: executing scheduled liveness verification ping");
                                    } else {
                                        warn!(
                                            "Watchdog: hook inactive for 5m; sending active verification ping"
                                        );
                                    }
                                    ping_pending = true;
                                    last_ping_sent_at_unix_ms = now;
                                    force_ping_at_unix_ms = 0; // Clear scheduled ping
                                    let _ = rdev::simulate(&rdev::EventType::KeyPress(rdev::Key::Unknown(255)));
                                    let _ = rdev::simulate(&rdev::EventType::KeyRelease(rdev::Key::Unknown(255)));
                                } else if now >= last_ping_sent_at_unix_ms + 500 {
                                    warn!(
                                        "Watchdog: verification ping failed to roundtrip within 500ms; restarting stale hook"
                                    );
                                    hook_health.mark_recovery_signal("watchdog: stale hook");
                                    LISTENER_EPOCH.fetch_add(1, Ordering::SeqCst);
                                    needs_restart = true;
                                    ping_pending = false;
                                }
                            } else if !seems_stale && !scheduled_ping_due {
                                ping_pending = false;
                            }
                        }

                        if needs_restart {
                            tear_down_listener(&mut listener_handle);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        error!("Supervisor channel disconnected; shutting down");
                        break;
                    }
                }

                if listener_handle.is_none() && std::time::Instant::now() >= next_spawn_allowed_after {
                    info!("Reinstalling Windows hook listener");
                    listener_spawned_at_unix_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    listener_handle = Some(spawn_windows_hook_listener(
                        evaluator.clone(),
                        state.clone(),
                        paused.clone(),
                        pause_notifications_enabled.clone(),
                        pause_hotkey.clone(),
                        spinner_style.clone(),
                        pause_audio_enabled.clone(),
                        audio_tx.clone(),
                        pause_transition_tx.clone(),
                        hook_health.clone(),
                        tx.clone(),
                    ));
                }
            }

            info!("Hook supervisor thread is shutting down");
            tear_down_listener(&mut listener_handle);
        });

    if let Err(error) = spawn_result {
        error!(error = %error, "Failed to spawn Windows hook supervisor thread");
        None
    } else {
        spawn_result.ok()
    }
}

pub fn stop_windows_supervisor() {
    if let Some(slot) = SUPERVISOR_SENDER.get() {
        let lock_res = slot.lock();
        if let Ok(mut lock) = lock_res {
            let tx_opt = lock.take();
            if let Some(tx) = tx_opt {
                let _ = tx.send(WindowsSupervisorEvent::Shutdown);
            }
        }
    }
    crate::platform::windows::power::stop_listener();
}

fn tear_down_listener(listener_handle: &mut Option<ListenerHandle>) {
    let Some(handle) = listener_handle.take() else {
        return;
    };
    send_wm_quit_to_thread(handle.thread_id, &handle.join);

    // Give the thread up to 2 seconds to exit, then detach if unresponsive.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        if handle.join.is_finished() {
            if let Err(_error) = handle.join.join() {
                warn!("Listener thread panicked during teardown; hook chain may be inconsistent");
            }
            return;
        }
        if std::time::Instant::now() >= deadline {
            warn!("Listener thread did not exit within 2s after WM_QUIT; detaching thread");
            // ponytail: leaked thread exits at process death; upgrade path is a
            // cooperative cancellation token if this becomes a resource concern.
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn send_wm_quit_to_thread(thread_id: u32, join_handle: &std::thread::JoinHandle<()>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};

    // If the thread has already exited, no need to post WM_QUIT.
    if join_handle.is_finished() {
        return;
    }

    if thread_id == 0 {
        warn!(
            thread_id,
            "Cannot post WM_QUIT to listener: supervisor has no OS thread id (spawn failed earlier)"
        );
        return;
    }

    // SAFETY: PostThreadMessageW posts a message to the message queue of the
    // specified thread. `thread_id` is a valid OS thread ID obtained from
    // GetCurrentThreadId() on the listener thread. WM_QUIT (0x0012) is a
    // standard system message constant. The wparam/lparam values are ignored
    // for WM_QUIT. Retrying 50 times with 10ms sleep handles the race where
    // the target thread hasn't created its message queue yet.
    unsafe {
        for _ in 0..50 {
            if PostThreadMessageW(thread_id, WM_QUIT, 0, 0) != 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    warn!(
        thread_id,
        "Failed to post WM_QUIT after 500ms of retries; listener thread likely crashed before creating its message queue"
    );
}
