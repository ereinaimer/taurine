#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType, Key};
#[cfg(not(target_os = "linux"))]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use tokio::runtime::Handle;
#[cfg(not(target_os = "linux"))]
use tracing::{debug, error};

use crate::hotkey;
#[cfg(not(target_os = "linux"))]
use crate::injector::{self, IS_INJECTING, IS_SIMULATING};
#[cfg(not(target_os = "linux"))]
use crate::notify;
use taurine_core::engine::Evaluator;
#[cfg(not(target_os = "linux"))]
use taurine_core::engine::{EngineEvent, EngineMode};

#[cfg(target_os = "linux")]
pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    runtime_handle: Handle,
) {
    crate::platform::linux::evdev::start_listener(
        evaluator,
        paused,
        pause_notifications_enabled,
        pause_hotkey,
        spinner_style,
        runtime_handle,
    );
}

#[cfg(not(target_os = "linux"))]
pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    runtime_handle: Handle,
) {
    let alt_down = std::sync::atomic::AtomicBool::new(false);
    let ctrl_down = std::sync::atomic::AtomicBool::new(false);

    let callback = move |event: Event| -> Option<Event> {
        match event.event_type {
            EventType::KeyPress(Key::Alt) | EventType::KeyPress(Key::AltGr) => {
                alt_down.store(true, Ordering::Relaxed);
            }
            EventType::KeyRelease(Key::Alt) | EventType::KeyRelease(Key::AltGr) => {
                alt_down.store(false, Ordering::Relaxed);
            }
            EventType::KeyPress(Key::ControlLeft) | EventType::KeyPress(Key::ControlRight) => {
                ctrl_down.store(true, Ordering::Relaxed);
            }
            EventType::KeyRelease(Key::ControlLeft) | EventType::KeyRelease(Key::ControlRight) => {
                ctrl_down.store(false, Ordering::Relaxed);
            }
            _ => {}
        }

        let is_chord = if let Ok(spec) = pause_hotkey.read() {
            hotkey::is_pause_chord(&event, alt_down.load(Ordering::Relaxed), &spec)
        } else {
            false
        };

        if is_chord {
            let now_paused = !paused.load(Ordering::Relaxed);
            paused.store(now_paused, Ordering::Relaxed);
            if pause_notifications_enabled.load(Ordering::Relaxed) {
                notify::notify_pause_toggled(now_paused);
            }
            return None;
        }

        if paused.load(Ordering::Relaxed) {
            return Some(event);
        }

        match event.event_type {
            EventType::ButtonPress(_) => {
                if IS_INJECTING.load(Ordering::SeqCst) {
                    injector::abort_injection();
                    return Some(event);
                }
                let mut lock = evaluator.lock().unwrap();
                let _ = lock.process_event(EngineEvent::Interrupt);
            }
            EventType::KeyPress(key) => {
                if IS_INJECTING.load(Ordering::SeqCst) {
                    if !IS_SIMULATING.load(Ordering::SeqCst) {
                        injector::abort_injection();
                    }
                    return Some(event);
                }

                let engine_mode = evaluator
                    .lock()
                    .map(|lock| lock.state.engine_mode())
                    .unwrap_or(EngineMode::Normal);

                let engine_event = match key {
                    Key::Escape => Some(EngineEvent::Interrupt),
                    Key::Backspace => {
                        if ctrl_down.load(Ordering::Relaxed) {
                            Some(EngineEvent::WordBackspace)
                        } else {
                            Some(EngineEvent::Backspace)
                        }
                    }
                    Key::Space => Some(EngineEvent::Char(' ')),
                    Key::Return => Some(map_return_key(engine_mode)),
                    Key::Tab => Some(EngineEvent::Interrupt),
                    Key::UpArrow
                    | Key::DownArrow
                    | Key::LeftArrow
                    | Key::RightArrow
                    | Key::Home
                    | Key::End
                    | Key::PageUp
                    | Key::PageDown => Some(EngineEvent::Interrupt),
                    _ => {
                        if alt_down.load(Ordering::Relaxed) || ctrl_down.load(Ordering::Relaxed) {
                            return Some(event);
                        }

                        if let Some(ref text) = event.name {
                            if text.chars().count() == 1 {
                                Some(EngineEvent::Char(text.chars().next().unwrap()))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                };

                if let Some(ev) = engine_event {
                    let mut lock = evaluator.lock().unwrap();
                    if let Some(expansion) = lock.process_event(ev) {
                        drop(lock);

                        debug!("Trigger matched! Expanding: {:?}", expansion);
                        IS_INJECTING.store(true, Ordering::SeqCst);

                        let spinner_style_inner =
                            spinner_style.read().map(|s| *s).unwrap_or_default();

                        spawn_expansion_dispatch(
                            expansion,
                            spinner_style_inner,
                            runtime_handle.clone(),
                        );
                    }
                }
            }
            _ => {}
        }

        Some(event)
    };

    if let Err(e) = rdev::grab(callback) {
        error!("Fatal OS global hook crash: {:?}", e);
    }
}

#[cfg(not(target_os = "linux"))]
fn map_return_key(engine_mode: EngineMode) -> EngineEvent {
    if matches!(engine_mode, EngineMode::AiCapture { .. }) {
        EngineEvent::Char('\n')
    } else {
        EngineEvent::Interrupt
    }
}

pub(crate) fn spawn_expansion_dispatch(
    expansion: taurine_core::engine::ExpansionResult,
    spinner_style: taurine_core::settings::SpinnerStyle,
    runtime_handle: Handle,
) {
    std::thread::spawn(move || {
        let taurine_core::engine::ExpansionResult {
            delete_count,
            steps,
            trigger,
            is_calculation,
            track_usage,
            start_ai_spinner,
            inline_ai_prompt,
            ai_system_prompt_override,
        } = expansion;

        let output_len: usize = steps
            .iter()
            .filter_map(|step| match step {
                taurine_core::engine::variables::ExpansionStep::Text(text) => {
                    Some(text.chars().count())
                }
                _ => None,
            })
            .sum();

        crate::injector::inject_expansion(steps, delete_count, spinner_style);

        if start_ai_spinner && let Some(prompt) = inline_ai_prompt {
            let spinner_handle = crate::engine::ai::spinner::spawn(&runtime_handle);
            runtime_handle.spawn(async move {
                crate::engine::ai::stream::run_inline_ai_stream(
                    prompt,
                    ai_system_prompt_override,
                    spinner_handle,
                )
                .await;
            });
        }

        if track_usage {
            if is_calculation {
                taurine_core::db::crud::record_calculation_usage(output_len, delete_count, 0);
            } else {
                taurine_core::db::crud::record_expansion_usage(
                    &trigger,
                    output_len,
                    delete_count,
                    0,
                );
            }
        }
    });
}
