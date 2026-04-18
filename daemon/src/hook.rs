#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType, Key};
#[cfg(not(target_os = "linux"))]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use tokio::runtime::Handle;
#[cfg(not(target_os = "linux"))]
use tracing::{debug, error};

use crate::engine::ai::InlineAiUiState;
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
    ai_ui_state: Arc<InlineAiUiState>,
) {
    crate::platform::linux::evdev::start_listener(
        evaluator,
        paused,
        pause_notifications_enabled,
        pause_hotkey,
        spinner_style,
        runtime_handle,
        ai_ui_state,
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
    ai_ui_state: Arc<InlineAiUiState>,
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

        let engine_mode = evaluator
            .lock()
            .map(|lock| lock.state.engine_mode())
            .unwrap_or(EngineMode::Normal);

        if should_submit_ai_prompt(
            &event.event_type,
            alt_down.load(Ordering::Relaxed),
            engine_mode,
        ) {
            let mut lock = evaluator.lock().unwrap();
            if let Some(expansion) = lock.process_event(EngineEvent::SubmitAiPrompt) {
                drop(lock);

                debug!("Inline AI prompt submitted. Starting loading state.");

                IS_INJECTING.store(true, Ordering::SeqCst);

                let spinner_style_inner = spinner_style.read().map(|s| *s).unwrap_or_default();
                spawn_expansion_dispatch(
                    expansion,
                    spinner_style_inner,
                    runtime_handle.clone(),
                    ai_ui_state.clone(),
                );
            }
            return None;
        }

        if should_block_ai_session_chord(
            &event.event_type,
            alt_down.load(Ordering::Relaxed),
            engine_mode,
        ) {
            return None;
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
                // Mouse click — always physical. If injection is active, abort it.
                if IS_INJECTING.load(Ordering::SeqCst) {
                    injector::abort_injection();
                    return Some(event);
                }
                let mut lock = evaluator.lock().unwrap();
                let _ = lock.process_event(EngineEvent::Interrupt);
            }
            EventType::KeyPress(key) => {
                // All events during injection are synthetic (our own backspaces / Ctrl+V).
                // We cannot distinguish physical from synthetic in rdev::grab,
                // so pass them through without feeding the evaluator.
                if IS_INJECTING.load(Ordering::SeqCst) {
                    // If this keyboard event is NOT synthetic (it occurred while IS_SIMULATING is false),
                    // then it must be physical user input. Abort the injection.
                    if !IS_SIMULATING.load(Ordering::SeqCst) {
                        injector::abort_injection();
                    }
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
                    // Structural keys — break any active typing sequence.
                    Key::Return => Some(map_return_key(engine_mode)),
                    Key::Tab => Some(EngineEvent::Interrupt),
                    // Navigation keys — cursor moved, buffer is now desynchronized.
                    Key::UpArrow
                    | Key::DownArrow
                    | Key::LeftArrow
                    | Key::RightArrow
                    | Key::Home
                    | Key::End
                    | Key::PageUp
                    | Key::PageDown => Some(EngineEvent::Interrupt),
                    _ => {
                        // Any key pressed with a modifier (ctrl/alt) is a
                        // system chord, not a character — reset the buffer.
                        if alt_down.load(Ordering::Relaxed) || ctrl_down.load(Ordering::Relaxed) {
                            return Some(event);
                        }
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

                        let spinner_style_inner =
                            spinner_style.read().map(|s| *s).unwrap_or_default();

                        spawn_expansion_dispatch(
                            expansion,
                            spinner_style_inner,
                            runtime_handle.clone(),
                            ai_ui_state.clone(),
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
fn should_submit_ai_prompt(
    event_type: &EventType,
    alt_down: bool,
    engine_mode: EngineMode,
) -> bool {
    engine_mode == EngineMode::AiCapture
        && alt_down
        && matches!(event_type, EventType::KeyPress(Key::Return))
}

#[cfg(not(target_os = "linux"))]
fn should_block_ai_session_chord(
    event_type: &EventType,
    alt_down: bool,
    engine_mode: EngineMode,
) -> bool {
    matches!(
        engine_mode,
        EngineMode::AiCapture | EngineMode::AiGenerating
    ) && alt_down
        && matches!(
            event_type,
            EventType::KeyPress(Key::Return)
                | EventType::KeyRelease(Key::Return)
                | EventType::KeyPress(Key::Escape)
                | EventType::KeyRelease(Key::Escape)
        )
}

#[cfg(not(target_os = "linux"))]
fn map_return_key(engine_mode: EngineMode) -> EngineEvent {
    if engine_mode == EngineMode::AiCapture {
        EngineEvent::Char('\n')
    } else {
        EngineEvent::Interrupt
    }
}

pub(crate) fn spawn_expansion_dispatch(
    expansion: taurine_core::engine::ExpansionResult,
    spinner_style: taurine_core::settings::SpinnerStyle,
    runtime_handle: Handle,
    ai_ui_state: Arc<InlineAiUiState>,
) {
    std::thread::spawn(move || {
        let trigger_clone = expansion.trigger.clone();
        let delete_count = expansion.delete_count;
        let track_usage = expansion.track_usage;
        let should_start_ai_spinner = expansion.start_ai_spinner;

        let output_len: usize = expansion
            .steps
            .iter()
            .filter_map(|s| match s {
                taurine_core::engine::variables::ExpansionStep::Text(t) => Some(t.chars().count()),
                _ => None,
            })
            .sum();

        crate::injector::inject_expansion(expansion.steps, expansion.delete_count, spinner_style);

        if should_start_ai_spinner {
            let spinner_handle = crate::engine::ai::spinner::spawn(&runtime_handle);
            ai_ui_state.set_spinner(spinner_handle);
        }

        if track_usage {
            if expansion.is_calculation {
                taurine_core::db::crud::record_calculation_usage(output_len, delete_count, 0);
            } else {
                taurine_core::db::crud::record_expansion_usage(
                    &trigger_clone,
                    output_len,
                    delete_count,
                    0,
                );
            }
        }
    });
}

#[cfg(test)]
#[cfg(not(target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn ai_capture_submits_only_on_alt_enter_press() {
        assert!(should_submit_ai_prompt(
            &EventType::KeyPress(Key::Return),
            true,
            EngineMode::AiCapture
        ));
        assert!(!should_submit_ai_prompt(
            &EventType::KeyRelease(Key::Return),
            true,
            EngineMode::AiCapture
        ));
        assert!(!should_submit_ai_prompt(
            &EventType::KeyPress(Key::Return),
            true,
            EngineMode::AiGenerating
        ));
        assert!(!should_submit_ai_prompt(
            &EventType::KeyPress(Key::Escape),
            true,
            EngineMode::AiCapture
        ));
    }

    #[test]
    fn ai_session_blocks_alt_enter_and_alt_escape_before_pass_through() {
        assert!(should_block_ai_session_chord(
            &EventType::KeyPress(Key::Return),
            true,
            EngineMode::AiCapture
        ));
        assert!(should_block_ai_session_chord(
            &EventType::KeyPress(Key::Escape),
            true,
            EngineMode::AiCapture
        ));
        assert!(should_block_ai_session_chord(
            &EventType::KeyRelease(Key::Return),
            true,
            EngineMode::AiGenerating
        ));
        assert!(!should_block_ai_session_chord(
            &EventType::KeyPress(Key::Return),
            false,
            EngineMode::AiCapture
        ));
        assert!(!should_block_ai_session_chord(
            &EventType::KeyPress(Key::Return),
            true,
            EngineMode::Normal
        ));
    }

    #[test]
    fn return_key_maps_to_newline_only_in_ai_capture() {
        assert_eq!(
            map_return_key(EngineMode::AiCapture),
            EngineEvent::Char('\n')
        );
        assert_eq!(map_return_key(EngineMode::Normal), EngineEvent::Interrupt);
    }
}
