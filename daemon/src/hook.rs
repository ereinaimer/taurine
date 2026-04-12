#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType, Key};
#[cfg(not(target_os = "linux"))]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
#[cfg(not(target_os = "linux"))]
use std::thread;
#[cfg(not(target_os = "linux"))]
use tracing::{debug, error};

use crate::hotkey;
#[cfg(not(target_os = "linux"))]
use crate::injector::{self, IS_INJECTING};
#[cfg(not(target_os = "linux"))]
use crate::notify;
#[cfg(not(target_os = "linux"))]
use taurine_core::engine::EngineEvent;
use taurine_core::engine::Evaluator;

#[cfg(target_os = "linux")]
pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
) {
    crate::platform::linux::evdev::start_listener(
        evaluator,
        paused,
        pause_notifications_enabled,
        pause_hotkey,
    );
}

#[cfg(not(target_os = "linux"))]
pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
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

        // Evaluate pause toggle before any typing buffer / expansion logic.
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
            // Strictly consume the keystroke (do not pass to OS).
            return None;
        }

        // If paused, immediately pass all keystrokes through to the OS.
        if paused.load(Ordering::Relaxed) {
            return Some(event);
        }

        match event.event_type {
            EventType::ButtonPress(_) => {
                // Mouse click — ignore if we're mid-injection; otherwise clear the buffer.
                if IS_INJECTING.load(Ordering::SeqCst) {
                    return Some(event);
                }
                let mut lock = evaluator.lock().unwrap();
                let _ = lock.process_event(EngineEvent::Interrupt);
            }
            EventType::KeyPress(key) => {
                // All events during injection are synthetic (our own backspaces / Ctrl+V).
                // Ignore them so they don't feed back into the evaluator.
                if IS_INJECTING.load(Ordering::SeqCst) {
                    return Some(event);
                }

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
                    // Enter submits / moves to a new line — break any active sequence.
                    Key::Return => Some(EngineEvent::Interrupt),
                    _ => {
                        // Use rdev's pre-decoded character for layout-awareness.
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

                        // CRITICAL: Set IS_INJECTING = true HERE, in the hook thread,
                        // BEFORE spawning. This closes the race window where the OS
                        // could deliver the next keystroke event before the spawned
                        // thread gets scheduled and sets the flag itself.
                        IS_INJECTING.store(true, Ordering::SeqCst);

                        thread::spawn(move || {
                            let trigger_clone = expansion.trigger.clone();
                            let delete_count = expansion.delete_count;

                            // Calculate output char count for metrics from text steps.
                            let output_len: usize = expansion
                                .steps
                                .iter()
                                .filter_map(|s| match s {
                                    taurine_core::engine::variables::ExpansionStep::Text(t) => {
                                        Some(t.chars().count())
                                    }
                                    _ => None,
                                })
                                .sum();

                            injector::inject_expansion(expansion.steps, expansion.delete_count);

                            if expansion.is_calculation {
                                taurine_core::db::crud::record_calculation_usage(
                                    output_len,
                                    delete_count,
                                    0,
                                );
                            } else {
                                taurine_core::db::crud::record_expansion_usage(
                                    &trigger_clone,
                                    output_len,
                                    delete_count,
                                    0,
                                );
                            }
                        });
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
