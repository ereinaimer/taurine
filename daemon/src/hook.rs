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
use crate::hotkey_evaluator::{
    HotkeyEvaluation, HotkeyEvaluator, logical_key_from_rdev, modifiers_from_flags,
};
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
    let shift_down = std::sync::atomic::AtomicBool::new(false);
    let meta_down = std::sync::atomic::AtomicBool::new(false);
    let hotkey_evaluator = Mutex::new(HotkeyEvaluator::new());

    let callback = move |event: Event| -> Option<Event> {
        if IS_INJECTING.load(Ordering::SeqCst) && IS_SIMULATING.load(Ordering::SeqCst) {
            return Some(event);
        }

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
            EventType::KeyPress(Key::ShiftLeft) | EventType::KeyPress(Key::ShiftRight) => {
                shift_down.store(true, Ordering::Relaxed);
            }
            EventType::KeyRelease(Key::ShiftLeft) | EventType::KeyRelease(Key::ShiftRight) => {
                shift_down.store(false, Ordering::Relaxed);
            }
            EventType::KeyPress(Key::MetaLeft) | EventType::KeyPress(Key::MetaRight) => {
                meta_down.store(true, Ordering::Relaxed);
            }
            EventType::KeyRelease(Key::MetaLeft) | EventType::KeyRelease(Key::MetaRight) => {
                meta_down.store(false, Ordering::Relaxed);
            }
            _ => {}
        }

        if IS_INJECTING.load(Ordering::SeqCst) {
            match event.event_type {
                EventType::KeyRelease(key) => {
                    if let Some(logical_key) = logical_key_from_rdev(key)
                        && let Ok(mut lock) = hotkey_evaluator.lock()
                    {
                        let _ = lock.on_key_release(logical_key);
                    }
                }
                EventType::KeyPress(_) => {
                    injector::abort_injection();
                }
                _ => {}
            }

            return Some(event);
        }

        let is_chord = if let Ok(spec) = pause_hotkey.read() {
            hotkey::is_pause_chord(&event, alt_down.load(Ordering::Relaxed), &spec)
        } else {
            false
        };

        if is_chord {
            clear_undo_state(&evaluator);
            let now_paused = !paused.load(Ordering::Relaxed);
            paused.store(now_paused, Ordering::Relaxed);
            if pause_notifications_enabled.load(Ordering::Relaxed) {
                notify::notify_pause_toggled(now_paused);
            }
            return None;
        }

        match event.event_type {
            EventType::ButtonPress(_) => {
                clear_undo_state(&evaluator);
                if let Ok(mut lock) = hotkey_evaluator.lock() {
                    lock.clear();
                }
                if paused.load(Ordering::Relaxed) {
                    return Some(event);
                }
                let mut lock = evaluator.lock().unwrap();
                let _ = lock.process_event(EngineEvent::Interrupt);
            }
            EventType::KeyPress(key) => {
                let shift_active = shift_down.load(Ordering::Relaxed);
                let ctrl_active = ctrl_down.load(Ordering::Relaxed);
                let alt_active = alt_down.load(Ordering::Relaxed);
                let meta_active = meta_down.load(Ordering::Relaxed);
                let modifiers =
                    modifiers_from_flags(ctrl_active, shift_active, alt_active, meta_active);

                if paused.load(Ordering::Relaxed) {
                    return Some(event);
                }

                if let Some(logical_key) = logical_key_from_rdev(key) {
                    let state = evaluator.lock().map(|lock| lock.state.clone()).ok();
                    if let Some(state) = state
                        && let Ok(mut lock) = hotkey_evaluator.lock()
                    {
                        match lock.on_key_event(state.as_ref(), true, modifiers, logical_key) {
                            HotkeyEvaluation::Matched(expansion) => {
                                debug!("Hotkey matched! Expanding: {:?}", expansion);
                                IS_INJECTING.store(true, Ordering::SeqCst);

                                let spinner_style_inner =
                                    spinner_style.read().map(|s| *s).unwrap_or_default();

                                spawn_expansion_dispatch(
                                    expansion,
                                    spinner_style_inner,
                                    runtime_handle.clone(),
                                    state,
                                );
                                return None;
                            }
                            HotkeyEvaluation::Swallow => return None,
                            HotkeyEvaluation::NoMatch => {}
                        }
                    }
                }

                if key == Key::Backspace {
                    if ctrl_active || alt_active || meta_active {
                        clear_undo_state(&evaluator);
                        return Some(event);
                    }

                    if let Some((trigger_string, output_length)) =
                        take_active_undo_state(&evaluator)
                    {
                        // Win32/macOS can swallow directly in the global hook callback by
                        // returning `None` here. Linux cannot do that with a passive reader, so
                        // it uses EVIOCGRAB + uinput proxying instead.
                        IS_INJECTING.store(true, Ordering::SeqCst);
                        spawn_undo_dispatch(trigger_string, output_length);
                        return None;
                    }
                } else if is_solo_modifier_press(
                    key,
                    shift_active,
                    ctrl_active,
                    alt_active,
                    meta_active,
                ) {
                    // Naked modifier presses should not expire the undo window.
                } else {
                    // Invalidate on any non-modifier or combo before normal evaluator handling.
                    clear_undo_state(&evaluator);
                }

                let engine_mode = evaluator
                    .lock()
                    .map(|lock| lock.state.engine_mode())
                    .unwrap_or(EngineMode::Normal);

                let engine_event = match key {
                    Key::Escape => Some(EngineEvent::Interrupt),
                    Key::Backspace => {
                        if ctrl_active {
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
                        if alt_active || ctrl_active || meta_active {
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
                        let state = lock.state.clone();
                        drop(lock);

                        debug!("Trigger matched! Expanding: {:?}", expansion);
                        IS_INJECTING.store(true, Ordering::SeqCst);

                        let spinner_style_inner =
                            spinner_style.read().map(|s| *s).unwrap_or_default();

                        spawn_expansion_dispatch(
                            expansion,
                            spinner_style_inner,
                            runtime_handle.clone(),
                            state,
                        );
                    }
                }
            }
            EventType::KeyRelease(key) => {
                if let Some(logical_key) = logical_key_from_rdev(key)
                    && let Ok(mut lock) = hotkey_evaluator.lock()
                    && matches!(lock.on_key_release(logical_key), HotkeyEvaluation::Swallow)
                {
                    return None;
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

#[cfg(not(target_os = "linux"))]
fn clear_undo_state(evaluator: &Arc<Mutex<Evaluator>>) {
    if let Ok(lock) = evaluator.lock() {
        lock.state.clear_undo_state();
    }
}

#[cfg(not(target_os = "linux"))]
fn take_active_undo_state(evaluator: &Arc<Mutex<Evaluator>>) -> Option<(String, usize)> {
    evaluator.lock().ok().and_then(|lock| {
        lock.state
            .take_active_undo_state()
            .map(|undo| (undo.trigger_string, undo.output_length))
    })
}

#[cfg(not(target_os = "linux"))]
fn spawn_undo_dispatch(trigger_string: String, output_length: usize) {
    std::thread::spawn(move || injector::inject_undo(trigger_string, output_length));
}

#[cfg(not(target_os = "linux"))]
fn is_modifier_key(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::Alt
            | Key::AltGr
            | Key::MetaLeft
            | Key::MetaRight
    )
}

#[cfg(not(target_os = "linux"))]
fn is_solo_modifier_press(
    key: Key,
    shift_active: bool,
    ctrl_active: bool,
    alt_active: bool,
    meta_active: bool,
) -> bool {
    match key {
        Key::ShiftLeft | Key::ShiftRight => !ctrl_active && !alt_active && !meta_active,
        Key::ControlLeft | Key::ControlRight => !shift_active && !alt_active && !meta_active,
        Key::Alt | Key::AltGr => !shift_active && !ctrl_active && !meta_active,
        Key::MetaLeft | Key::MetaRight => !shift_active && !ctrl_active && !alt_active,
        _ => is_modifier_key(key) && !shift_active && !ctrl_active && !alt_active && !meta_active,
    }
}

pub(crate) fn spawn_expansion_dispatch(
    expansion: taurine_core::engine::ExpansionResult,
    spinner_style: taurine_core::settings::SpinnerStyle,
    runtime_handle: Handle,
    state: Arc<taurine_core::engine::EngineState>,
) {
    std::thread::spawn(move || {
        dispatch_expansion_with(
            expansion,
            spinner_style,
            runtime_handle,
            state,
            crate::injector::inject_expansion,
            launch_follow_up,
        );
    });
}

fn dispatch_expansion_with<I, L>(
    expansion: taurine_core::engine::ExpansionResult,
    spinner_style: taurine_core::settings::SpinnerStyle,
    runtime_handle: Handle,
    state: Arc<taurine_core::engine::EngineState>,
    inject_expansion: I,
    launch_follow_up_fn: L,
) where
    I: FnOnce(
        Vec<taurine_core::engine::variables::ExpansionStep>,
        usize,
        taurine_core::settings::SpinnerStyle,
    ),
    L: FnOnce(
        Option<taurine_core::engine::ExpansionFollowUp>,
        taurine_core::settings::SpinnerStyle,
        Handle,
    ),
{
    let taurine_core::engine::ExpansionResult {
        delete_count,
        steps,
        trigger,
        undo_trigger,
        is_calculation,
        track_usage,
        follow_up,
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

    let should_record_undo = follow_up.is_none() && output_len > 0;
    state.clear_undo_state();
    inject_expansion(steps, delete_count, spinner_style);
    if should_record_undo && let Some(undo_trigger) = undo_trigger {
        state.set_undo_state(undo_trigger, output_len);
    }
    launch_follow_up_fn(follow_up, spinner_style, runtime_handle);

    if track_usage {
        if is_calculation {
            taurine_core::db::crud::record_calculation_usage(output_len, delete_count, 0);
        } else {
            taurine_core::db::crud::record_expansion_usage(&trigger, output_len, delete_count, 0);
        }
    }
}

fn launch_follow_up(
    follow_up: Option<taurine_core::engine::ExpansionFollowUp>,
    spinner_style: taurine_core::settings::SpinnerStyle,
    runtime_handle: Handle,
) {
    if let Some(taurine_core::engine::ExpansionFollowUp::InlineAi {
        prompt,
        system_prompt_override,
    }) = follow_up
    {
        crate::injector::IS_INJECTING.store(true, std::sync::atomic::Ordering::SeqCst);
        crate::injector::INJECTION_ABORT.store(false, std::sync::atomic::Ordering::SeqCst);

        let spinner_handle = taurine_core::utils::spinner::spawn_async(
            spinner_style,
            crate::platform::spinner_renderer::OsSpinnerRenderer::default(),
            &runtime_handle,
        );
        runtime_handle.spawn(async move {
            crate::engine::ai::stream::run_inline_ai_stream(
                prompt,
                system_prompt_override,
                spinner_handle,
            )
            .await;

            crate::injector::IS_INJECTING.store(false, std::sync::atomic::Ordering::SeqCst);
            crate::injector::INJECTION_ABORT.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn dispatch_expansion_runs_injection_before_follow_up_consumption() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let inject_events = events.clone();
        let follow_up_events = events.clone();
        let state = Arc::new(taurine_core::engine::EngineState::new('>'));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");

        let expansion = taurine_core::engine::ExpansionResult {
            delete_count: 4,
            steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
                "thinking".to_string(),
            )],
            trigger: "ai".to_string(),
            undo_trigger: Some(">ai".to_string()),
            is_calculation: false,
            track_usage: false,
            follow_up: Some(taurine_core::engine::ExpansionFollowUp::InlineAi {
                prompt: "prompt".to_string(),
                system_prompt_override: Some("expert editor".to_string()),
            }),
        };

        dispatch_expansion_with(
            expansion,
            taurine_core::settings::SpinnerStyle::default(),
            rt.handle().clone(),
            state,
            move |_, _, _| {
                inject_events
                    .lock()
                    .expect("inject events poisoned")
                    .push("inject")
            },
            move |follow_up, _, _| {
                follow_up_events
                    .lock()
                    .expect("follow-up events poisoned")
                    .push("follow_up");
                assert_eq!(
                    follow_up,
                    Some(taurine_core::engine::ExpansionFollowUp::InlineAi {
                        prompt: "prompt".to_string(),
                        system_prompt_override: Some("expert editor".to_string()),
                    })
                );
            },
        );

        assert_eq!(
            &*events.lock().expect("events poisoned"),
            &["inject", "follow_up"]
        );
    }

    #[test]
    fn dispatch_expansion_records_undo_state_for_plain_text_output() {
        let state = Arc::new(taurine_core::engine::EngineState::new('>'));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");

        let expansion = taurine_core::engine::ExpansionResult {
            delete_count: 4,
            steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
                "Good Morning".to_string(),
            )],
            trigger: "gm".to_string(),
            undo_trigger: Some(">gm".to_string()),
            is_calculation: false,
            track_usage: false,
            follow_up: None,
        };

        dispatch_expansion_with(
            expansion,
            taurine_core::settings::SpinnerStyle::default(),
            rt.handle().clone(),
            state.clone(),
            move |_, _, _| {},
            move |_, _, _| {},
        );

        let undo = state
            .take_active_undo_state()
            .expect("undo state should be recorded");
        assert!(undo.trigger_string.starts_with('>'));
        assert_eq!(undo.trigger_string, ">gm");
        assert_eq!(undo.output_length, "Good Morning".chars().count());
    }

    #[test]
    fn dispatch_expansion_skips_undo_registration_for_hotkey_results() {
        let state = Arc::new(taurine_core::engine::EngineState::new('>'));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");

        let expansion = taurine_core::engine::ExpansionResult {
            delete_count: 0,
            steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
                "git status".to_string(),
            )],
            trigger: "ctrl+shift+g".to_string(),
            undo_trigger: None,
            is_calculation: false,
            track_usage: false,
            follow_up: None,
        };

        dispatch_expansion_with(
            expansion,
            taurine_core::settings::SpinnerStyle::default(),
            rt.handle().clone(),
            state.clone(),
            move |_, _, _| {},
            move |_, _, _| {},
        );

        assert!(state.take_active_undo_state().is_none());
    }
}
