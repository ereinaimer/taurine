#![cfg(target_os = "macos")]

use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::input::hook_health::HookHealth;
use crate::input::hotkey;
use taurine_core::engine::Evaluator;

#[derive(Debug, Clone)]
pub enum MacosSupervisorEvent {
    WillSleep,
    DidWake,
    ListenerExited { error: Option<String> },
    Shutdown,
}

static SUPERVISOR_SENDER: std::sync::OnceLock<Mutex<Option<mpsc::Sender<MacosSupervisorEvent>>>> =
    std::sync::OnceLock::new();

#[allow(clippy::too_many_arguments)]
pub fn start_macos_supervisor(
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
    let (tx, rx) = mpsc::channel::<MacosSupervisorEvent>();

    if let Ok(mut lock) = SUPERVISOR_SENDER.get_or_init(|| Mutex::new(None)).lock() {
        *lock = Some(tx.clone());
    }

    let supervisor_tx = tx.clone();

    // Start supervisor thread
    std::thread::Builder::new()
        .name("tau-macos-supervisor".to_string())
        .spawn(move || {
            info!("macOS Event Tap supervisor active");
            let mut active_listener: Option<std::thread::JoinHandle<()>> = None;

            let spawn_listener =
                |listener_tx: mpsc::Sender<MacosSupervisorEvent>| -> std::thread::JoinHandle<()> {
                    let eval = evaluator.clone();
                    let st = state.clone();
                    let p = paused.clone();
                    let pne = pause_notifications_enabled.clone();
                    let phk = pause_hotkey.clone();
                    let ss = spinner_style.clone();
                    let pae = pause_audio_enabled.clone();
                    let atx = audio_tx.clone();
                    let ptx = pause_transition_tx.clone();
                    let health = hook_health.clone();

                    std::thread::spawn(move || {
                        health.mark_listener_started();
                        health.mark_listener_entering_grab();
                        crate::hook::start_listener(eval, st, p, pne, phk, ss, pae, atx, ptx);
                        let _ =
                            listener_tx.send(MacosSupervisorEvent::ListenerExited { error: None });
                    })
                };

            active_listener = Some(spawn_listener(supervisor_tx.clone()));

            while let Ok(event) = rx.recv() {
                match event {
                    MacosSupervisorEvent::Shutdown => {
                        debug!("macOS supervisor shutting down");
                        crate::hook::listener::macos::stop_run_loop();
                        if let Some(handle) = active_listener.take() {
                            let _ = handle.join();
                        }
                        break;
                    }
                    MacosSupervisorEvent::WillSleep => {
                        debug!("macOS sleep notification received; stopping event tap");
                        crate::hook::listener::macos::stop_run_loop();
                        if let Some(handle) = active_listener.take() {
                            let _ = handle.join();
                        }
                    }
                    MacosSupervisorEvent::DidWake => {
                        info!("macOS wake notification received; re-initializing event tap");
                        hook_health.mark_recovery_signal("macOS wake from sleep");
                        std::thread::sleep(Duration::from_millis(250));
                        crate::hook::listener::macos::stop_run_loop();
                        if let Some(handle) = active_listener.take() {
                            let _ = handle.join();
                        }
                        active_listener = Some(spawn_listener(supervisor_tx.clone()));
                    }
                    MacosSupervisorEvent::ListenerExited { error } => {
                        warn!(
                            ?error,
                            "macOS hook listener thread exited, scheduling restart"
                        );
                        hook_health.mark_listener_exit(error);
                        std::thread::sleep(Duration::from_millis(500));
                        active_listener = Some(spawn_listener(supervisor_tx.clone()));
                    }
                }
            }
        })
        .ok()
}

pub fn stop_macos_supervisor() {
    if let Some(lock) = SUPERVISOR_SENDER.get()
        && let Ok(guard) = lock.lock()
        && let Some(ref tx) = *guard
    {
        let _ = tx.send(MacosSupervisorEvent::Shutdown);
    }
}
