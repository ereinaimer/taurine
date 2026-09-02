#[cfg(not(target_os = "linux"))]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, error, trace, warn};
#[cfg(not(target_os = "linux"))]
use unicode_normalization::UnicodeNormalization;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub(crate) use windows::{spawn_windows_hook_listener, sync_physical_modifiers};

#[cfg(test)]
#[cfg(windows)]
mod pipeline_tests;

#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType, Key};

#[cfg(not(target_os = "linux"))]
use crate::injector;
#[cfg(not(target_os = "linux"))]
use crate::injector::{IS_INJECTING, consume_simulated_event};
#[cfg(not(target_os = "linux"))]
use crate::input::hook_health::HookHealth;
use crate::input::hotkey;
#[cfg(not(target_os = "linux"))]
use crate::input::hotkey_evaluator::{
    HotkeyEvaluation, HotkeyEvaluator, logical_key_from_rdev, modifiers_from_sides,
};
use taurine_core::engine::Evaluator;
#[cfg(not(target_os = "linux"))]
use taurine_core::engine::{EngineEvent, EngineMode};

#[cfg(not(target_os = "linux"))]
use super::completion::{
    completion_key_kind_from_tab_like, should_swallow_trigger_assist_key_release,
    trigger_assist_is_active, trigger_assist_key_action,
};
#[cfg(not(target_os = "linux"))]
use super::dispatch::{
    clear_undo_state, spawn_completion_rewrite_dispatch, spawn_expansion_dispatch,
    spawn_undo_dispatch, take_active_undo_state,
};

#[cfg(not(target_os = "linux"))]
pub(super) static LISTENER_EPOCH: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(not(target_os = "linux"))]
static LAST_PAUSE_TOGGLE_INSTANT: std::sync::OnceLock<std::sync::Mutex<std::time::Instant>> =
    std::sync::OnceLock::new();

#[cfg(not(target_os = "linux"))]
static PAUSE_KEY_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    state: Arc<taurine_core::engine::EngineState>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    pause_audio_enabled: Arc<std::sync::atomic::AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
    pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
) {
    crate::platform::linux::evdev::start_listener(
        evaluator,
        state,
        paused,
        pause_notifications_enabled,
        pause_hotkey,
        spinner_style,
        pause_audio_enabled,
        audio_tx,
        pause_transition_tx,
    );
}

#[cfg(not(target_os = "linux"))]
#[cfg_attr(windows, allow(dead_code))]
#[allow(clippy::too_many_arguments)]
pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    state: Arc<taurine_core::engine::EngineState>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    pause_audio_enabled: Arc<std::sync::atomic::AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
    pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
) {
    if let Err(error) = run_listener_once(
        evaluator,
        state,
        paused,
        pause_notifications_enabled,
        pause_hotkey,
        spinner_style,
        pause_audio_enabled,
        audio_tx,
        pause_transition_tx,
        None,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        Arc::new(Mutex::new(HotkeyEvaluator::new())),
        Arc::new(std::sync::atomic::AtomicU32::new(0)),
    ) {
        error!(error = %error, "Fatal OS global hook crash");
    }
}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn run_listener_once(
    evaluator: Arc<Mutex<Evaluator>>,
    state: Arc<taurine_core::engine::EngineState>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    _pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    _pause_audio_enabled: Arc<std::sync::atomic::AtomicBool>,
    _audio_tx: tokio::sync::mpsc::Sender<bool>,
    pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
    hook_health: Option<HookHealth>,
    left_alt_down: Arc<std::sync::atomic::AtomicBool>,
    right_alt_down: Arc<std::sync::atomic::AtomicBool>,
    left_ctrl_down: Arc<std::sync::atomic::AtomicBool>,
    right_ctrl_down: Arc<std::sync::atomic::AtomicBool>,
    left_shift_down: Arc<std::sync::atomic::AtomicBool>,
    right_shift_down: Arc<std::sync::atomic::AtomicBool>,
    left_meta_down: Arc<std::sync::atomic::AtomicBool>,
    right_meta_down: Arc<std::sync::atomic::AtomicBool>,
    hotkey_evaluator: Arc<Mutex<HotkeyEvaluator>>,
    event_counter: Arc<std::sync::atomic::AtomicU32>,
) -> Result<u64, String> {
    let callback_health = hook_health.clone();
    let my_epoch = LISTENER_EPOCH.load(Ordering::SeqCst);

    let evaluator_clone = evaluator.clone();
    let state_clone = state.clone();
    let paused_clone = paused.clone();
    let pause_hotkey_clone = pause_hotkey.clone();
    let spinner_style_clone = spinner_style.clone();
    let pause_transition_tx_clone = pause_transition_tx.clone();
    let left_alt_down_clone = left_alt_down.clone();
    let right_alt_down_clone = right_alt_down.clone();
    let left_ctrl_down_clone = left_ctrl_down.clone();
    let right_ctrl_down_clone = right_ctrl_down.clone();
    let left_shift_down_clone = left_shift_down.clone();
    let right_shift_down_clone = right_shift_down.clone();
    let left_meta_down_clone = left_meta_down.clone();
    let right_meta_down_clone = right_meta_down.clone();
    let hotkey_evaluator_clone = hotkey_evaluator.clone();
    let event_counter_clone = event_counter.clone();

    let callback = move |event: Event| -> Option<Event> {
        if LISTENER_EPOCH.load(Ordering::Relaxed) != my_epoch {
            return Some(event);
        }

        if is_keyboard_event(&event.event_type)
            && let Some(health) = callback_health.as_ref()
        {
            health.record_keyboard_event();
        }

        process_keyboard_event(
            event,
            &evaluator_clone,
            &state_clone,
            &paused_clone,
            &pause_hotkey_clone,
            &spinner_style_clone,
            &pause_transition_tx_clone,
            &left_alt_down_clone,
            &right_alt_down_clone,
            &left_ctrl_down_clone,
            &right_ctrl_down_clone,
            &left_shift_down_clone,
            &right_shift_down_clone,
            &left_meta_down_clone,
            &right_meta_down_clone,
            &hotkey_evaluator_clone,
            &event_counter_clone,
        )
    };

    if let Some(health) = hook_health.as_ref() {
        health.mark_listener_entering_grab();
    }

    #[cfg(target_os = "macos")]
    {
        macos::register_run_loop();
    }

    debug!("Hook listener entering rdev::grab");
    #[cfg(windows)]
    let res = windows::windows_grab(callback).map_err(|error| format!("{error:?}"));
    #[cfg(not(windows))]
    let res = rdev::grab(callback).map_err(|error| format!("{error:?}"));

    res?;
    Ok(my_epoch)
}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::too_many_arguments)]
pub fn process_keyboard_event(
    event: Event,
    evaluator: &Arc<Mutex<Evaluator>>,
    state: &Arc<taurine_core::engine::EngineState>,
    paused: &Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: &Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: &Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    pause_transition_tx: &tokio::sync::mpsc::Sender<bool>,
    left_alt_down: &std::sync::atomic::AtomicBool,
    right_alt_down: &std::sync::atomic::AtomicBool,
    left_ctrl_down: &std::sync::atomic::AtomicBool,
    right_ctrl_down: &std::sync::atomic::AtomicBool,
    left_shift_down: &std::sync::atomic::AtomicBool,
    right_shift_down: &std::sync::atomic::AtomicBool,
    left_meta_down: &std::sync::atomic::AtomicBool,
    right_meta_down: &std::sync::atomic::AtomicBool,
    hotkey_evaluator: &Arc<Mutex<HotkeyEvaluator>>,
    _event_counter: &Arc<std::sync::atomic::AtomicU32>,
) -> Option<Event> {
    #[cfg(windows)]
    if matches!(
        event.event_type,
        EventType::KeyPress(Key::Unknown(255)) | EventType::KeyRelease(Key::Unknown(255))
    ) {
        return Some(event);
    }

    #[cfg(windows)]
    if std::env::var("TAURINE_TEST_HANG_HOOK").is_ok()
        && matches!(event.event_type, EventType::KeyPress(Key::Unknown(254)))
    {
        warn!("Test trigger: Hanging listener thread for 6 seconds!");
        std::thread::sleep(Duration::from_secs(6));
        return Some(event);
    }

    if consume_simulated_event(&event.event_type) {
        return Some(event);
    }

    if is_keyboard_event(&event.event_type) {
        trace!(
            event_kind = event_type_label(&event.event_type),
            "Hook callback received keyboard event"
        );
    }

    match event.event_type {
        EventType::KeyPress(Key::Alt) => left_alt_down.store(true, Ordering::Relaxed),
        EventType::KeyRelease(Key::Alt) => left_alt_down.store(false, Ordering::Relaxed),
        EventType::KeyPress(Key::AltGr) => right_alt_down.store(true, Ordering::Relaxed),
        EventType::KeyRelease(Key::AltGr) => right_alt_down.store(false, Ordering::Relaxed),
        EventType::KeyPress(Key::ControlLeft) => {
            left_ctrl_down.store(true, Ordering::Relaxed);
        }
        EventType::KeyRelease(Key::ControlLeft) => {
            left_ctrl_down.store(false, Ordering::Relaxed);
        }
        EventType::KeyPress(Key::ControlRight) => {
            right_ctrl_down.store(true, Ordering::Relaxed);
        }
        EventType::KeyRelease(Key::ControlRight) => {
            right_ctrl_down.store(false, Ordering::Relaxed);
        }
        EventType::KeyPress(Key::ShiftLeft) => {
            left_shift_down.store(true, Ordering::Relaxed);
        }
        EventType::KeyRelease(Key::ShiftLeft) => {
            left_shift_down.store(false, Ordering::Relaxed);
        }
        EventType::KeyPress(Key::ShiftRight) => {
            right_shift_down.store(true, Ordering::Relaxed);
        }
        EventType::KeyRelease(Key::ShiftRight) => {
            right_shift_down.store(false, Ordering::Relaxed);
        }
        EventType::KeyPress(Key::MetaLeft) => {
            left_meta_down.store(true, Ordering::Relaxed);
        }
        EventType::KeyRelease(Key::MetaLeft) => {
            left_meta_down.store(false, Ordering::Relaxed);
        }
        EventType::KeyPress(Key::MetaRight) => {
            right_meta_down.store(true, Ordering::Relaxed);
        }
        EventType::KeyRelease(Key::MetaRight) => {
            right_meta_down.store(false, Ordering::Relaxed);
        }
        _ => {}
    }

    let is_modifier_event = matches!(
        event.event_type,
        EventType::KeyPress(
            Key::ShiftLeft
                | Key::ShiftRight
                | Key::ControlLeft
                | Key::ControlRight
                | Key::Alt
                | Key::AltGr
                | Key::MetaLeft
                | Key::MetaRight
        ) | EventType::KeyRelease(
            Key::ShiftLeft
                | Key::ShiftRight
                | Key::ControlLeft
                | Key::ControlRight
                | Key::Alt
                | Key::AltGr
                | Key::MetaLeft
                | Key::MetaRight
        )
    );

    let any_modifier_active = left_ctrl_down.load(Ordering::Relaxed)
        || right_ctrl_down.load(Ordering::Relaxed)
        || left_shift_down.load(Ordering::Relaxed)
        || right_shift_down.load(Ordering::Relaxed)
        || left_alt_down.load(Ordering::Relaxed)
        || right_alt_down.load(Ordering::Relaxed)
        || left_meta_down.load(Ordering::Relaxed)
        || right_meta_down.load(Ordering::Relaxed);

    // Active GetAsyncKeyState checks executed on modifier key transitions or when a modifier
    // is active in memory to ensure physical state is always synchronized with zero overhead on normal typing.
    #[cfg(windows)]
    if is_modifier_event || any_modifier_active {
        sync_physical_modifiers(
            left_alt_down,
            right_alt_down,
            left_ctrl_down,
            right_ctrl_down,
            left_shift_down,
            right_shift_down,
            left_meta_down,
            right_meta_down,
        );
    }

    let left_ctrl_active = left_ctrl_down.load(Ordering::Relaxed);
    let right_ctrl_active = right_ctrl_down.load(Ordering::Relaxed);
    let left_shift_active = left_shift_down.load(Ordering::Relaxed);
    let right_shift_active = right_shift_down.load(Ordering::Relaxed);
    let left_alt_active = left_alt_down.load(Ordering::Relaxed);
    let right_alt_active = right_alt_down.load(Ordering::Relaxed);
    let left_meta_active = left_meta_down.load(Ordering::Relaxed);
    let right_meta_active = right_meta_down.load(Ordering::Relaxed);
    let modifiers = modifiers_from_sides(
        left_ctrl_active,
        right_ctrl_active,
        left_shift_active,
        right_shift_active,
        left_alt_active,
        right_alt_active,
        left_meta_active,
        right_meta_active,
    );

    if IS_INJECTING.load(Ordering::SeqCst) {
        match event.event_type {
            EventType::KeyRelease(key) => {
                if let Some(logical_key) = logical_key_from_rdev(key)
                    && let Ok(mut lock) = hotkey_evaluator.lock()
                {
                    let _ = lock.on_key_release(logical_key);
                }
                return Some(event);
            }
            EventType::KeyPress(_) => {
                injector::abort_injection();
                trace!(
                    "Injection aborted by physical keypress, falling through to normal pipeline"
                );
            }
            _ => return Some(event),
        }
    }

    let is_chord = if let Ok(spec) = pause_hotkey.read() {
        hotkey::is_pause_chord(&event, modifiers, &spec)
    } else {
        false
    };

    let is_release_chord = if let Ok(spec) = pause_hotkey.read() {
        if let EventType::KeyRelease(rdev_key) = event.event_type
            && let Some(logical_key) = logical_key_from_rdev(rdev_key)
        {
            logical_key == spec.hotkey.key
        } else {
            false
        }
    } else {
        false
    };

    if is_release_chord {
        PAUSE_KEY_DOWN.store(false, Ordering::Relaxed);
    }

    if is_chord {
        let was_down = PAUSE_KEY_DOWN.swap(true, Ordering::Relaxed);
        if was_down {
            return None; // Ignore repeating keys
        }

        let now = std::time::Instant::now();
        let mut last_toggle_lock = LAST_PAUSE_TOGGLE_INSTANT
            .get_or_init(|| {
                std::sync::Mutex::new(
                    now.checked_sub(std::time::Duration::from_millis(300))
                        .unwrap_or(now),
                )
            })
            .lock()
            .unwrap();

        if now.duration_since(*last_toggle_lock).as_millis() >= 300 {
            *last_toggle_lock = now;
            drop(last_toggle_lock);

            clear_undo_state(state.as_ref());
            let now_paused = !paused.load(Ordering::Relaxed);
            paused.store(now_paused, Ordering::Relaxed);

            // Notify coordinator
            let _ = pause_transition_tx.try_send(now_paused);
        }
        return None;
    }

    // If we are currently paused and this is not the pause chord, immediately bypass
    if paused.load(Ordering::Relaxed) {
        return Some(event);
    }

    match event.event_type {
        EventType::ButtonPress(_) => {
            #[cfg(windows)]
            sync_physical_modifiers(
                left_alt_down,
                right_alt_down,
                left_ctrl_down,
                right_ctrl_down,
                left_shift_down,
                right_shift_down,
                left_meta_down,
                right_meta_down,
            );
            #[cfg(not(windows))]
            {
                left_alt_down.store(false, Ordering::Relaxed);
                right_alt_down.store(false, Ordering::Relaxed);
                left_ctrl_down.store(false, Ordering::Relaxed);
                right_ctrl_down.store(false, Ordering::Relaxed);
                left_shift_down.store(false, Ordering::Relaxed);
                right_shift_down.store(false, Ordering::Relaxed);
                left_meta_down.store(false, Ordering::Relaxed);
                right_meta_down.store(false, Ordering::Relaxed);
            }
            clear_undo_state(state.as_ref());
            if let Ok(mut lock) = hotkey_evaluator.lock() {
                lock.clear();
            }
            if paused.load(Ordering::Relaxed) {
                return Some(event);
            }
            let _ = with_evaluator_lock(evaluator, "button_interrupt", |lock| {
                let _ = lock.process_event(EngineEvent::Interrupt, None);
            });
        }
        EventType::KeyPress(key) => {
            let ctrl_active = left_ctrl_active || right_ctrl_active;
            let shift_active = left_shift_active || right_shift_active;
            let alt_active = left_alt_active || right_alt_active;
            let meta_active = left_meta_active || right_meta_active;

            if paused.load(Ordering::Relaxed) {
                return Some(event);
            }

            let assist_active = trigger_assist_is_active(evaluator, state.as_ref());

            if assist_active {
                clear_undo_state(state.as_ref());

                if key == Key::Backspace && !alt_active && !meta_active {
                    let rewrite =
                        with_evaluator_lock(evaluator, "rewrite_backspace_query", |lock| {
                            if ctrl_active {
                                lock.rewrite_word_backspace_query()
                            } else {
                                lock.rewrite_backspace_query()
                            }
                        })
                        .flatten();

                    if let Some(rewrite) = rewrite {
                        let spinner_style_inner =
                            spinner_style.read().map(|s| *s).unwrap_or_default();
                        spawn_completion_rewrite_dispatch(rewrite, spinner_style_inner);
                        return None;
                    }
                }

                match trigger_assist_key_action(
                    state.as_ref(),
                    completion_key_kind_from_tab_like(
                        key == Key::Tab,
                        key == Key::Escape,
                        key == Key::UpArrow,
                        key == Key::DownArrow,
                    ),
                    shift_active,
                    ctrl_active,
                    alt_active,
                    meta_active,
                ) {
                    super::completion::CompletionKeyAction::CycleForward => {
                        let rewrite =
                            with_evaluator_lock(evaluator, "cycle_completion_next", |lock| {
                                lock.cycle_completion_next()
                            })
                            .flatten();

                        if let Some(rewrite) = rewrite {
                            let spinner_style_inner =
                                spinner_style.read().map(|s| *s).unwrap_or_default();
                            spawn_completion_rewrite_dispatch(rewrite, spinner_style_inner);
                        }

                        return None;
                    }
                    super::completion::CompletionKeyAction::CycleBackward => {
                        let rewrite =
                            with_evaluator_lock(evaluator, "cycle_completion_prev", |lock| {
                                lock.cycle_completion_prev()
                            })
                            .flatten();

                        if let Some(rewrite) = rewrite {
                            let spinner_style_inner =
                                spinner_style.read().map(|s| *s).unwrap_or_default();
                            spawn_completion_rewrite_dispatch(rewrite, spinner_style_inner);
                        }

                        return None;
                    }
                    super::completion::CompletionKeyAction::CancelAndSwallow => {
                        let _ =
                            with_evaluator_lock(evaluator, "cancel_completion_swallow", |lock| {
                                lock.cancel_completion();
                            });
                        return None;
                    }
                    super::completion::CompletionKeyAction::CancelAndPassThrough => {
                        let _ = with_evaluator_lock(
                            evaluator,
                            "cancel_completion_pass_through",
                            |lock| {
                                lock.cancel_completion();
                            },
                        );
                    }
                    super::completion::CompletionKeyAction::PassThrough => {}
                }
            } else if key == Key::Tab
                && !shift_active
                && !ctrl_active
                && !alt_active
                && !meta_active
            {
                let rewrite =
                    with_evaluator_lock(evaluator, "activate_triggerless_completion", |lock| {
                        lock.activate_triggerless_completion()
                    })
                    .flatten();

                if let Some(rewrite) = rewrite {
                    clear_undo_state(state.as_ref());
                    let spinner_style_inner = spinner_style.read().map(|s| *s).unwrap_or_default();
                    spawn_completion_rewrite_dispatch(rewrite, spinner_style_inner);
                    return None;
                }
            }

            if let Some(logical_key) = logical_key_from_rdev(key)
                && let Ok(mut lock) = hotkey_evaluator.lock()
            {
                match lock.on_key_event(state.as_ref(), true, modifiers, logical_key) {
                    HotkeyEvaluation::Matched(expansion) => {
                        debug!("Hotkey matched: {}", expansion.trigger);

                        let spinner_style_inner =
                            spinner_style.read().map(|s| *s).unwrap_or_default();

                        spawn_expansion_dispatch(expansion, spinner_style_inner, state.clone());
                        return None;
                    }
                    HotkeyEvaluation::Swallow => return None,
                    HotkeyEvaluation::NoMatch => {}
                }
            }

            if let Some(direction) = crate::hook::case_cycle_key_action(
                key,
                shift_active,
                ctrl_active,
                alt_active,
                meta_active,
                state.engine_mode(),
                state.inline_case_transform_enabled(),
            ) && let Some(rewrite) = state.advance_case_variant(direction)
            {
                let style = spinner_style.read().map(|s| *s).unwrap_or_default();
                spawn_completion_rewrite_dispatch(rewrite, style);
                return None;
            }

            if key == Key::Backspace {
                if ctrl_active || alt_active || meta_active {
                    clear_undo_state(state.as_ref());
                    return Some(event);
                }

                if let Some((trigger_string, output_length)) =
                    take_active_undo_state(state.as_ref())
                {
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
                clear_undo_state(state.as_ref());
            }

            let engine_mode = state.engine_mode();

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
                Key::Return => Some(EngineEvent::ActionKey),
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
                    if is_ai_capture_paste_key(&engine_mode, ctrl_active, meta_active, key) {
                        match crate::platform::read_clipboard_text() {
                            Ok(text) if !text.is_empty() => {
                                let engine_event = EngineEvent::Paste(text);
                                let _ = with_evaluator_lock(evaluator, "ai_paste", |lock| {
                                    lock.process_event(engine_event, None)
                                });
                            }
                            _ => {}
                        }
                        return None;
                    }

                    if alt_active || ctrl_active || meta_active {
                        return Some(event);
                    }

                    if let Some(ref text) = event.name {
                        let normalized: String = text.nfc().collect();
                        if normalized.chars().count() == 1 {
                            normalized.chars().next().map(EngineEvent::Char)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            };

            if let Some(ev) = engine_event {
                trace!(
                    engine_event = engine_event_label(&ev),
                    "Dispatching engine event from hook callback"
                );
                let is_action_key = ev == EngineEvent::ActionKey;

                if let Some((expansion, state)) =
                    with_evaluator_lock(evaluator, "process_engine_event", |lock| {
                        lock.process_event_lazy(ev, crate::platform::get_active_window_info)
                            .map(|expansion| {
                                let state = lock.state.clone();
                                (expansion, state)
                            })
                    })
                    .flatten()
                {
                    debug!("Trigger matched: {}", expansion.trigger);

                    let spinner_style_inner = spinner_style.read().map(|s| *s).unwrap_or_default();

                    spawn_expansion_dispatch(expansion, spinner_style_inner, state);

                    if is_action_key {
                        return None;
                    }
                }
            }
        }
        EventType::KeyRelease(key) => {
            if trigger_assist_is_active(evaluator, state.as_ref())
                && should_swallow_trigger_assist_key_release(
                    state.as_ref(),
                    completion_key_kind_from_tab_like(
                        key == Key::Tab,
                        false,
                        key == Key::UpArrow,
                        key == Key::DownArrow,
                    ),
                )
            {
                return None;
            }

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
}

#[allow(dead_code)]
pub fn stop_listener() {
    #[cfg(target_os = "macos")]
    {
        macos::stop_run_loop();
    }

    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::input_supervisor::stop();
    }
}

pub(super) fn with_evaluator_lock<T>(
    evaluator: &Arc<Mutex<Evaluator>>,
    operation: &'static str,
    action: impl FnOnce(&mut Evaluator) -> T,
) -> Option<T> {
    let lock_wait_started = Instant::now();
    let mut lock = match evaluator.try_lock() {
        Ok(lock) => lock,
        Err(std::sync::TryLockError::Poisoned(error)) => {
            error!(
                operation,
                error = %error,
                "Evaluator mutex poisoned inside hook callback"
            );
            return None;
        }
        Err(std::sync::TryLockError::WouldBlock) => {
            // Contention backoff: brief spin-wait for up to 1.5ms
            let mut acquired = None;
            while lock_wait_started.elapsed() < Duration::from_micros(1500) {
                std::hint::spin_loop();
                match evaluator.try_lock() {
                    Ok(guard) => {
                        acquired = Some(guard);
                        break;
                    }
                    Err(std::sync::TryLockError::Poisoned(error)) => {
                        error!(
                            operation,
                            error = %error,
                            "Evaluator mutex poisoned inside hook callback"
                        );
                        return None;
                    }
                    Err(std::sync::TryLockError::WouldBlock) => {}
                }
            }

            match acquired {
                Some(guard) => guard,
                None => {
                    warn!(
                        operation,
                        elapsed_us = lock_wait_started.elapsed().as_micros() as u64,
                        "Evaluator mutex contention in hook callback; passing keystroke through"
                    );
                    return None;
                }
            }
        }
    };
    let lock_wait = lock_wait_started.elapsed();

    let evaluation_started = Instant::now();
    let result = action(&mut lock);
    let evaluation_elapsed = evaluation_started.elapsed();

    log_callback_timing(operation, lock_wait, evaluation_elapsed);
    Some(result)
}

fn log_callback_timing(operation: &'static str, lock_wait: Duration, evaluation: Duration) {
    if lock_wait > Duration::from_millis(5) || evaluation > Duration::from_millis(5) {
        debug!(
            operation,
            lock_wait_us = lock_wait.as_micros() as u64,
            evaluation_us = evaluation.as_micros() as u64,
            "Hook callback evaluator timing"
        );
    } else {
        trace!(
            operation,
            lock_wait_us = lock_wait.as_micros() as u64,
            evaluation_us = evaluation.as_micros() as u64,
            "Hook callback evaluator timing"
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn is_keyboard_event(event_type: &EventType) -> bool {
    matches!(
        event_type,
        EventType::KeyPress(_) | EventType::KeyRelease(_)
    )
}

#[cfg(not(target_os = "linux"))]
fn event_type_label(event_type: &EventType) -> &'static str {
    match event_type {
        EventType::KeyPress(_) => "key_press",
        EventType::KeyRelease(_) => "key_release",
        EventType::ButtonPress(_) => "button_press",
        EventType::ButtonRelease(_) => "button_release",
        EventType::MouseMove { .. } => "mouse_move",
        EventType::Wheel { .. } => "wheel",
    }
}

#[cfg(not(target_os = "linux"))]
fn engine_event_label(event: &EngineEvent) -> &'static str {
    match event {
        EngineEvent::Interrupt => "interrupt",
        EngineEvent::Backspace => "backspace",
        EngineEvent::WordBackspace => "word_backspace",
        EngineEvent::ActionKey => "action_key",
        EngineEvent::Char(_) => "char",
        EngineEvent::Paste(_) => "paste",
    }
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
fn is_ai_capture_paste_key(
    engine_mode: &EngineMode,
    ctrl_active: bool,
    meta_active: bool,
    key: Key,
) -> bool {
    if !matches!(engine_mode, EngineMode::AiCapture { .. }) {
        return false;
    }
    let modifier_active =
        cfg!(target_os = "macos") && meta_active || cfg!(not(target_os = "macos")) && ctrl_active;
    modifier_active && key == Key::KeyV
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

#[cfg(test)]
#[cfg(not(target_os = "linux"))]
mod paste_detection_tests {
    use super::*;

    #[test]
    fn test_is_ai_capture_paste_key_in_ai_capture() {
        let ai = EngineMode::AiCapture {
            system_prompt_override: None,
        };

        #[cfg(target_os = "macos")]
        let paste_modifier = (false, true);
        #[cfg(not(target_os = "macos"))]
        let paste_modifier = (true, false);

        assert!(is_ai_capture_paste_key(
            &ai,
            paste_modifier.0,
            paste_modifier.1,
            Key::KeyV
        ));

        assert!(!is_ai_capture_paste_key(&ai, false, false, Key::KeyV));
        assert!(!is_ai_capture_paste_key(&ai, true, false, Key::KeyC));
        assert!(!is_ai_capture_paste_key(&ai, false, true, Key::KeyX));
    }

    #[test]
    fn test_is_ai_capture_paste_key_outside_ai_capture() {
        let normal = EngineMode::Normal;

        assert!(!is_ai_capture_paste_key(&normal, true, false, Key::KeyV));
        assert!(!is_ai_capture_paste_key(&normal, false, true, Key::KeyV));
        assert!(!is_ai_capture_paste_key(&normal, false, false, Key::KeyV));
    }
}
