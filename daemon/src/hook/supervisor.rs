#![cfg(windows)]

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tracing::{debug, error, warn};

use crate::input::hook_health::HookHealth;
use crate::input::hotkey;
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
    HookUnresponsive,
    Shutdown,
}

static SUPERVISOR_SENDER: std::sync::OnceLock<Mutex<Option<mpsc::Sender<WindowsSupervisorEvent>>>> =
    std::sync::OnceLock::new();

pub(super) struct ListenerHandle {
    pub(super) join: Option<std::thread::JoinHandle<()>>,
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

    let left_alt_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let right_alt_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let left_ctrl_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let right_ctrl_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let left_shift_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let right_shift_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let left_meta_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let right_meta_down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hotkey_evaluator = Arc::new(Mutex::new(
        crate::input::hotkey_evaluator::HotkeyEvaluator::new(),
    ));
    let event_counter = Arc::new(std::sync::atomic::AtomicU32::new(0));

    // Start permanent Raw Input monitor as watchdog
    let raw_ctx = crate::hook::raw_input::RawInputContext {
        hook_health: Some(hook_health.clone()),
        supervisor_tx: Some(tx.clone()),
    };
    if let Err(e) = crate::hook::raw_input::start_raw_input_listener(raw_ctx) {
        warn!("Failed to start Raw Input monitor watchdog: {}", e);
    }

    let spawn_result = std::thread::Builder::new()
        .name("tau-hook-super".to_string())
        .spawn(move || {
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
                left_alt_down.clone(),
                right_alt_down.clone(),
                left_ctrl_down.clone(),
                right_ctrl_down.clone(),
                left_shift_down.clone(),
                right_shift_down.clone(),
                left_meta_down.clone(),
                right_meta_down.clone(),
                hotkey_evaluator.clone(),
                event_counter.clone(),
            ));
            let mut next_spawn_allowed_after = std::time::Instant::now();
            let mut last_health_log_at_instant = std::time::Instant::now();
            let mut last_seen_started_unix: u64 = 0;
            let mut last_seen_started_instant = std::time::Instant::now();
            let mut last_unresponsive_recovery =
                std::time::Instant::now() - Duration::from_secs(10);

            loop {
                let event = rx.recv_timeout(Duration::from_millis(100));

                match event {
                    Ok(WindowsSupervisorEvent::HookUnresponsive) => {
                        if last_unresponsive_recovery.elapsed() >= Duration::from_millis(1000) {
                            last_unresponsive_recovery = std::time::Instant::now();
                            hook_health.mark_recovery_signal("raw input detected unresponsive hook");
                            warn!(
                                "Raw Input shadow detected missed events; reinstalling low-level hook immediately"
                            );
                            LISTENER_EPOCH.fetch_add(1, Ordering::SeqCst);
                            tear_down_listener(&mut listener_handle);
                            next_spawn_allowed_after = std::time::Instant::now();
                        }
                    }
                    Ok(WindowsSupervisorEvent::ListenerExited { error }) => {
                        hook_health.mark_listener_exit(error.clone());

                        if let Some(ref error) = error {
                            error!(error = %error, "Windows hook listener exited");
                        } else {
                            warn!("Windows hook listener exited without an error");
                        }

                        listener_handle.take();
                        next_spawn_allowed_after =
                            std::time::Instant::now() + Duration::from_millis(50);
                        debug!("Reinstalling Windows hook listener immediately");
                    }
                    Ok(WindowsSupervisorEvent::ResumeAutomatic) => {
                        hook_health.mark_recovery_signal("automatic resume");
                        warn!("Windows automatic resume detected; re-attaching hook listener");
                        tear_down_listener(&mut listener_handle);
                        next_spawn_allowed_after =
                            std::time::Instant::now() + Duration::from_millis(150);
                    }
                    Ok(WindowsSupervisorEvent::ResumeFromSuspend) => {
                        hook_health.mark_recovery_signal("resume from suspend");
                        warn!("Windows resume from suspend detected; re-attaching hook listener");
                        tear_down_listener(&mut listener_handle);
                        next_spawn_allowed_after =
                            std::time::Instant::now() + Duration::from_millis(200);
                    }
                    Ok(WindowsSupervisorEvent::SessionUnlock) => {
                        hook_health.mark_recovery_signal("session unlock");
                        warn!("Windows session unlock detected; re-attaching hook listener");
                        tear_down_listener(&mut listener_handle);
                        next_spawn_allowed_after =
                            std::time::Instant::now() + Duration::from_millis(200);
                    }
                    Ok(WindowsSupervisorEvent::SessionLogon) => {
                        hook_health.mark_recovery_signal("session logon");
                        warn!("Windows session logon detected; re-attaching hook listener");
                        tear_down_listener(&mut listener_handle);
                        next_spawn_allowed_after =
                            std::time::Instant::now() + Duration::from_millis(200);
                    }
                    Ok(WindowsSupervisorEvent::DisplayChange) => {
                        hook_health.mark_recovery_signal("display change");
                        debug!("Windows display change detected");
                    }
                    Ok(WindowsSupervisorEvent::Shutdown) => {
                        debug!("Windows hook supervisor received Shutdown event");
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if last_health_log_at_instant.elapsed().as_millis() >= 30_000 {
                            last_health_log_at_instant = std::time::Instant::now();
                            hook_health.log_periodic_health();
                        }

                        let mut needs_restart = false;

                        if let Some(ref handle) = listener_handle {
                            let snapshot = hook_health.snapshot();

                            if snapshot.hook_thread_started_at_unix_ms != last_seen_started_unix {
                                last_seen_started_unix = snapshot.hook_thread_started_at_unix_ms;
                                last_seen_started_instant = std::time::Instant::now();
                            }

                            // Check 1: Startup/Reinstall Hang — listener started
                            // but hasn't entered rdev::grab within 3 seconds.
                            let startup_hang = snapshot.hook_thread_started_at_unix_ms > 0
                                && snapshot.hook_entered_grab_at_unix_ms == 0
                                && last_seen_started_instant.elapsed().as_millis() >= 3000;

                            // Check 2: Silent Thread Termination — thread exited
                            // without sending ListenerExited.
                            let silent_exit = handle.join.as_ref().is_none_or(|j| j.is_finished());

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

                if listener_handle.is_none()
                    && std::time::Instant::now() >= next_spawn_allowed_after
                {
                    debug!("Reinstalling Windows hook listener");

                    // Clear ghost keyboard states before spawning the new listener thread
                    // to prevent old keys from breaking expansions/hotkeys
                    if let Ok(mut lock) = evaluator.lock() {
                        lock.reset();
                    }
                    crate::hook::dispatch::clear_undo_state(state.as_ref());

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
                        left_alt_down.clone(),
                        right_alt_down.clone(),
                        left_ctrl_down.clone(),
                        right_ctrl_down.clone(),
                        left_shift_down.clone(),
                        right_shift_down.clone(),
                        left_meta_down.clone(),
                        right_meta_down.clone(),
                        hotkey_evaluator.clone(),
                        event_counter.clone(),
                    ));
                }
            }

            debug!("Hook supervisor thread is shutting down");
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
    crate::hook::raw_input::stop_raw_input_listener();
}

fn tear_down_listener(listener_handle: &mut Option<ListenerHandle>) {
    let Some(handle) = listener_handle.take() else {
        return;
    };
    send_wm_quit_to_thread(handle.thread_id, handle.join.as_ref());

    // Give the thread up to 2 seconds to exit, then detach if unresponsive.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let Some(join) = handle.join else {
        return;
    };
    loop {
        if join.is_finished() {
            if let Err(_error) = join.join() {
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

fn send_wm_quit_to_thread(thread_id: u32, join_handle: Option<&std::thread::JoinHandle<()>>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};

    // If the thread has already exited, no need to post WM_QUIT.
    if join_handle.is_some_and(|j| j.is_finished()) {
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
