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
    runtime_handle: Handle,
    pause_audio_enabled: Arc<std::sync::atomic::AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
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
        .name("taurine-hook-supervisor".to_string())
        .spawn(move || {
            const RESTART_BACKOFF: Duration = Duration::from_secs(2);

            let mut listener_handle: Option<ListenerHandle> = Some(spawn_windows_hook_listener(
                evaluator.clone(),
                state.clone(),
                paused.clone(),
                pause_notifications_enabled.clone(),
                pause_hotkey.clone(),
                spinner_style.clone(),
                runtime_handle.clone(),
                pause_audio_enabled.clone(),
                audio_tx.clone(),
                hook_health.clone(),
                tx.clone(),
            ));

            while let Ok(event) = rx.recv() {
                let mut delay_restart = false;
                match event {
                    WindowsSupervisorEvent::ListenerExited { error } => {
                        hook_health.mark_listener_exit(error.clone());

                        if let Some(ref error) = error {
                            error!(error = %error, "Windows hook listener exited");
                        } else {
                            warn!("Windows hook listener exited without an error");
                        }

                        listener_handle.take();
                        std::thread::sleep(RESTART_BACKOFF);
                        info!("Restarting Windows hook listener after backoff");
                    }
                    WindowsSupervisorEvent::ResumeAutomatic => {
                        hook_health.mark_recovery_signal("automatic resume");
                        LISTENER_EPOCH.fetch_add(1, Ordering::SeqCst);
                        warn!(
                            "Windows automatic resume detected; tearing down old listener hook before reinstall"
                        );
                        tear_down_listener(&mut listener_handle);
                        delay_restart = true;
                    }
                    WindowsSupervisorEvent::ResumeFromSuspend => {
                        hook_health.mark_recovery_signal("resume from suspend");
                        LISTENER_EPOCH.fetch_add(1, Ordering::SeqCst);
                        warn!(
                            "Windows resume from suspend detected; tearing down old listener hook before reinstall"
                        );
                        tear_down_listener(&mut listener_handle);
                        delay_restart = true;
                    }
                    WindowsSupervisorEvent::SessionUnlock => {
                        hook_health.mark_recovery_signal("session unlock");
                        LISTENER_EPOCH.fetch_add(1, Ordering::SeqCst);
                        warn!(
                            "Windows session unlock detected; tearing down old listener hook before reinstall"
                        );
                        tear_down_listener(&mut listener_handle);
                        delay_restart = true;
                    }
                    WindowsSupervisorEvent::SessionLogon => {
                        hook_health.mark_recovery_signal("session logon");
                        LISTENER_EPOCH.fetch_add(1, Ordering::SeqCst);
                        warn!(
                            "Windows session logon detected; tearing down old listener hook before reinstall"
                        );
                        tear_down_listener(&mut listener_handle);
                        delay_restart = true;
                    }
                    WindowsSupervisorEvent::Shutdown => {
                        info!("Windows hook supervisor received Shutdown event");
                        break;
                    }
                }

                if listener_handle.is_none() {
                    if delay_restart {
                        std::thread::sleep(RESTART_BACKOFF);
                    }
                    info!("Reinstalling Windows hook listener");
                    listener_handle = Some(spawn_windows_hook_listener(
                        evaluator.clone(),
                        state.clone(),
                        paused.clone(),
                        pause_notifications_enabled.clone(),
                        pause_hotkey.clone(),
                        spinner_style.clone(),
                        runtime_handle.clone(),
                        pause_audio_enabled.clone(),
                        audio_tx.clone(),
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
    send_wm_quit_to_thread(handle.thread_id);
    if let Err(_error) = handle.join.join() {
        warn!("Listener thread panicked during teardown; hook chain may be inconsistent");
    }
}

fn send_wm_quit_to_thread(thread_id: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};

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
    // for WM_QUIT. Retrying 500 times with 10ms sleep handles the race where
    // the target thread hasn't created its message queue yet.
    unsafe {
        for _ in 0..500 {
            if PostThreadMessageW(thread_id, WM_QUIT, 0, 0) != 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    warn!(
        thread_id,
        "Failed to post WM_QUIT after 5s of retries; listener thread likely crashed before creating its message queue"
    );
}
