#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType, Key};
#[cfg(windows)]
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(not(target_os = "linux"))]
use std::sync::atomic::Ordering;
#[cfg(windows)]
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
#[allow(unused_imports)]
use tracing::{debug, error, info, trace, warn};

#[cfg(not(target_os = "linux"))]
use crate::hook_health::HookHealth;
use crate::hotkey;
#[cfg(not(target_os = "linux"))]
use crate::hotkey_evaluator::{
    HotkeyEvaluation, HotkeyEvaluator, logical_key_from_rdev, modifiers_from_sides,
};
use crate::injector;
#[cfg(not(target_os = "linux"))]
use crate::injector::{IS_INJECTING, consume_simulated_event};
#[cfg(not(target_os = "linux"))]
use crate::notify;
use taurine_core::engine::Evaluator;
#[cfg(not(target_os = "linux"))]
use taurine_core::engine::{EngineEvent, EngineMode};

#[cfg(windows)]
#[derive(Debug, Clone)]
pub enum WindowsSupervisorEvent {
    ResumeAutomatic,
    ResumeFromSuspend,
    SessionUnlock,
    SessionLogon,
    ListenerExited { error: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionKeyKind {
    Tab,
    Escape,
    Up,
    Down,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionKeyAction {
    CycleForward,
    CycleBackward,
    HistoryOlder,
    HistoryNewer,
    CancelAndSwallow,
    CancelAndPassThrough,
    PassThrough,
}

pub(crate) fn completion_key_action(
    key: CompletionKeyKind,
    shift_active: bool,
    ctrl_active: bool,
    alt_active: bool,
    meta_active: bool,
) -> CompletionKeyAction {
    match key {
        CompletionKeyKind::Tab => {
            if ctrl_active || alt_active || meta_active {
                CompletionKeyAction::CancelAndPassThrough
            } else if shift_active {
                CompletionKeyAction::CycleBackward
            } else {
                CompletionKeyAction::CycleForward
            }
        }
        CompletionKeyKind::Escape => CompletionKeyAction::CancelAndSwallow,
        CompletionKeyKind::Up => CompletionKeyAction::HistoryOlder,
        CompletionKeyKind::Down => CompletionKeyAction::HistoryNewer,
        CompletionKeyKind::Other => CompletionKeyAction::PassThrough,
    }
}

pub(crate) fn trigger_assist_key_action(
    state: &taurine_core::engine::EngineState,
    key: CompletionKeyKind,
    shift_active: bool,
    ctrl_active: bool,
    alt_active: bool,
    meta_active: bool,
) -> CompletionKeyAction {
    match completion_key_action(key, shift_active, ctrl_active, alt_active, meta_active) {
        CompletionKeyAction::CycleForward | CompletionKeyAction::CycleBackward
            if !state.inline_tab_completion_enabled() =>
        {
            CompletionKeyAction::PassThrough
        }
        CompletionKeyAction::HistoryOlder | CompletionKeyAction::HistoryNewer
            if !state.inline_history_enabled() =>
        {
            CompletionKeyAction::PassThrough
        }
        action => action,
    }
}

pub(crate) fn completion_key_kind_from_tab_like(
    is_tab: bool,
    is_escape: bool,
    is_up: bool,
    is_down: bool,
) -> CompletionKeyKind {
    if is_tab {
        CompletionKeyKind::Tab
    } else if is_escape {
        CompletionKeyKind::Escape
    } else if is_up {
        CompletionKeyKind::Up
    } else if is_down {
        CompletionKeyKind::Down
    } else {
        CompletionKeyKind::Other
    }
}

pub(crate) fn should_swallow_trigger_assist_key_release(
    state: &taurine_core::engine::EngineState,
    key: CompletionKeyKind,
) -> bool {
    match key {
        CompletionKeyKind::Tab => state.inline_tab_completion_enabled(),
        CompletionKeyKind::Up | CompletionKeyKind::Down => state.inline_history_enabled(),
        CompletionKeyKind::Escape | CompletionKeyKind::Other => false,
    }
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    state: Arc<taurine_core::engine::EngineState>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    runtime_handle: Handle,
    pause_audio_enabled: Arc<std::sync::atomic::AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
) {
    crate::platform::linux::evdev::start_listener(
        evaluator,
        state,
        paused,
        pause_notifications_enabled,
        pause_hotkey,
        spinner_style,
        runtime_handle,
        pause_audio_enabled,
        audio_tx,
    );
}

#[cfg(windows)]
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
) {
    let (tx, rx) = mpsc::channel::<WindowsSupervisorEvent>();

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

            let mut listener_running = spawn_windows_hook_listener(
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
            );

            while let Ok(event) = rx.recv() {
                match event {
                    WindowsSupervisorEvent::ListenerExited { error } => {
                        hook_health.mark_listener_exit(error.clone());

                        if let Some(ref error) = error {
                            error!(error = %error, "Windows hook listener exited");
                        } else {
                            warn!("Windows hook listener exited without an error");
                        }

                        std::thread::sleep(RESTART_BACKOFF);
                        info!("Restarting Windows hook listener after backoff");
                        listener_running = spawn_windows_hook_listener(
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
                        );
                    }
                    WindowsSupervisorEvent::ResumeAutomatic => {
                        mark_windows_recovery_signal(
                            &hook_health,
                            listener_running,
                            "automatic resume",
                        );
                    }
                    WindowsSupervisorEvent::ResumeFromSuspend => {
                        mark_windows_recovery_signal(
                            &hook_health,
                            listener_running,
                            "resume from suspend",
                        );
                    }
                    WindowsSupervisorEvent::SessionUnlock => {
                        mark_windows_recovery_signal(
                            &hook_health,
                            listener_running,
                            "session unlock",
                        );
                    }
                    WindowsSupervisorEvent::SessionLogon => {
                        mark_windows_recovery_signal(
                            &hook_health,
                            listener_running,
                            "session logon",
                        );
                    }
                }

                if !listener_running {
                    info!("Hook listener is not running; attempting supervised restart");
                    listener_running = spawn_windows_hook_listener(
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
                    );
                }
            }

            warn!("Windows hook supervisor channel closed; hook supervision stopped");
        });

    if let Err(error) = spawn_result {
        error!(error = %error, "Failed to spawn Windows hook supervisor thread");
    }
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
    runtime_handle: Handle,
    pause_audio_enabled: Arc<std::sync::atomic::AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
) {
    if let Err(error) = run_listener_once(
        evaluator,
        state,
        paused,
        pause_notifications_enabled,
        pause_hotkey,
        spinner_style,
        runtime_handle,
        pause_audio_enabled,
        audio_tx,
        None,
    ) {
        error!(error = %error, "Fatal OS global hook crash");
    }
}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::too_many_arguments)]
fn run_listener_once(
    evaluator: Arc<Mutex<Evaluator>>,
    state: Arc<taurine_core::engine::EngineState>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    runtime_handle: Handle,
    pause_audio_enabled: Arc<std::sync::atomic::AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
    hook_health: Option<HookHealth>,
) -> Result<(), String> {
    let left_alt_down = std::sync::atomic::AtomicBool::new(false);
    let right_alt_down = std::sync::atomic::AtomicBool::new(false);
    let left_ctrl_down = std::sync::atomic::AtomicBool::new(false);
    let right_ctrl_down = std::sync::atomic::AtomicBool::new(false);
    let left_shift_down = std::sync::atomic::AtomicBool::new(false);
    let right_shift_down = std::sync::atomic::AtomicBool::new(false);
    let left_meta_down = std::sync::atomic::AtomicBool::new(false);
    let right_meta_down = std::sync::atomic::AtomicBool::new(false);
    let hotkey_evaluator = Mutex::new(HotkeyEvaluator::new());
    let callback_health = hook_health.clone();

    let callback = move |event: Event| -> Option<Event> {
        if consume_simulated_event(&event.event_type) {
            return Some(event);
        }

        if is_keyboard_event(&event.event_type) {
            if let Some(health) = callback_health.as_ref() {
                health.record_keyboard_event();
            }
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
                }
                EventType::KeyPress(_) => {
                    injector::abort_injection();
                }
                _ => {}
            }

            return Some(event);
        }

        let is_chord = if let Ok(spec) = pause_hotkey.read() {
            hotkey::is_pause_chord(&event, modifiers, &spec)
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
            if pause_audio_enabled.load(Ordering::Relaxed) {
                let _ = audio_tx.try_send(now_paused);
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
                let _ = with_evaluator_lock(&evaluator, "button_interrupt", |lock| {
                    let _ = lock.process_event(EngineEvent::Interrupt);
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

                if trigger_assist_is_active(&evaluator, state.as_ref()) {
                    clear_undo_state(&evaluator);

                    if key == Key::Backspace && !alt_active && !meta_active {
                        let rewrite =
                            with_evaluator_lock(&evaluator, "rewrite_backspace_query", |lock| {
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
                        CompletionKeyAction::CycleForward => {
                            let rewrite =
                                with_evaluator_lock(&evaluator, "cycle_completion_next", |lock| {
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
                        CompletionKeyAction::CycleBackward => {
                            let rewrite =
                                with_evaluator_lock(&evaluator, "cycle_completion_prev", |lock| {
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
                        CompletionKeyAction::HistoryOlder => {
                            let rewrite =
                                with_evaluator_lock(&evaluator, "navigate_history_older", |lock| {
                                    lock.navigate_history_older()
                                })
                                .flatten();

                            if let Some(rewrite) = rewrite {
                                let spinner_style_inner =
                                    spinner_style.read().map(|s| *s).unwrap_or_default();
                                spawn_completion_rewrite_dispatch(rewrite, spinner_style_inner);
                            }

                            return None;
                        }
                        CompletionKeyAction::HistoryNewer => {
                            let rewrite =
                                with_evaluator_lock(&evaluator, "navigate_history_newer", |lock| {
                                    lock.navigate_history_newer()
                                })
                                .flatten();

                            if let Some(rewrite) = rewrite {
                                let spinner_style_inner =
                                    spinner_style.read().map(|s| *s).unwrap_or_default();
                                spawn_completion_rewrite_dispatch(rewrite, spinner_style_inner);
                            }

                            return None;
                        }
                        CompletionKeyAction::CancelAndSwallow => {
                            let _ = with_evaluator_lock(
                                &evaluator,
                                "cancel_completion_swallow",
                                |lock| {
                                    lock.cancel_completion();
                                },
                            );
                            return None;
                        }
                        CompletionKeyAction::CancelAndPassThrough => {
                            let _ = with_evaluator_lock(
                                &evaluator,
                                "cancel_completion_pass_through",
                                |lock| {
                                    lock.cancel_completion();
                                },
                            );
                        }
                        CompletionKeyAction::PassThrough => {}
                    }
                }

                if let Some(logical_key) = logical_key_from_rdev(key)
                    && let Ok(mut lock) = hotkey_evaluator.lock()
                {
                    match lock.on_key_event(state.as_ref(), true, modifiers, logical_key) {
                        HotkeyEvaluation::Matched(expansion) => {
                            debug!("Hotkey matched! Expanding: {:?}", expansion);

                            let spinner_style_inner =
                                spinner_style.read().map(|s| *s).unwrap_or_default();

                            spawn_expansion_dispatch(
                                expansion,
                                spinner_style_inner,
                                runtime_handle.clone(),
                                state.clone(),
                            );
                            return None;
                        }
                        HotkeyEvaluation::Swallow => return None,
                        HotkeyEvaluation::NoMatch => {}
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
                    Key::Space => {
                        if *state.action_delimiter.read().unwrap()
                            == taurine_core::settings::ActionDelimiter::Space
                        {
                            Some(EngineEvent::ActionDelimiter)
                        } else {
                            Some(EngineEvent::Char(' '))
                        }
                    }
                    Key::Return => {
                        if *state.action_delimiter.read().unwrap()
                            == taurine_core::settings::ActionDelimiter::Enter
                        {
                            Some(EngineEvent::ActionDelimiter)
                        } else {
                            Some(map_return_key(engine_mode))
                        }
                    }
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
                    trace!(
                        engine_event = engine_event_label(&ev),
                        "Dispatching engine event from hook callback"
                    );
                    if let Some((expansion, state)) =
                        with_evaluator_lock(&evaluator, "process_engine_event", |lock| {
                            lock.process_event(ev).map(|expansion| {
                                let state = lock.state.clone();
                                (expansion, state)
                            })
                        })
                        .flatten()
                    {
                        debug!("Trigger matched! Expanding: {:?}", expansion);

                        let spinner_style_inner =
                            spinner_style.read().map(|s| *s).unwrap_or_default();

                        spawn_expansion_dispatch(
                            expansion,
                            spinner_style_inner,
                            runtime_handle.clone(),
                            state,
                        );

                        if ev == EngineEvent::ActionDelimiter {
                            return None;
                        }
                    }
                }
            }
            EventType::KeyRelease(key) => {
                if trigger_assist_is_active(&evaluator, state.as_ref())
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
    };

    if let Some(health) = hook_health.as_ref() {
        health.mark_listener_entering_grab();
    }
    info!("Hook listener entering rdev::grab");
    rdev::grab(callback).map_err(|error| format!("{error:?}"))
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn spawn_windows_hook_listener(
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
    supervisor_tx: mpsc::Sender<WindowsSupervisorEvent>,
) -> bool {
    hook_health.mark_listener_started();
    info!("Starting supervised Windows hook listener thread");
    let listener_health = hook_health.clone();

    let spawn_result = std::thread::Builder::new()
        .name("taurine-hook-listener".to_string())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                run_listener_once(
                    evaluator,
                    state,
                    paused,
                    pause_notifications_enabled,
                    pause_hotkey,
                    spinner_style,
                    runtime_handle,
                    pause_audio_enabled,
                    audio_tx,
                    Some(listener_health.clone()),
                )
            }));

            let exit_error = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(_) => Some("Windows hook listener panicked".to_string()),
            };

            if let Some(ref error) = exit_error {
                error!(error = %error, "Windows hook listener is exiting");
            } else {
                warn!("Windows hook listener returned unexpectedly without an error");
            }

            if let Err(error) =
                supervisor_tx.send(WindowsSupervisorEvent::ListenerExited { error: exit_error })
            {
                error!(
                    error = %error,
                    "Failed to notify hook supervisor that the listener exited"
                );
            }
        });

    if let Err(error) = spawn_result {
        let message = format!("Failed to spawn Windows hook listener thread: {error}");
        hook_health.mark_listener_exit(Some(message.clone()));
        error!(error = %message, "Unable to spawn Windows hook listener");
        false
    } else {
        true
    }
}

#[cfg(windows)]
fn mark_windows_recovery_signal(hook_health: &HookHealth, listener_running: bool, reason: &str) {
    hook_health.mark_recovery_signal(reason);
    if listener_running {
        warn!(
            recovery_reason = reason,
            "Windows power/session change detected; listener is still running so automatic rehook is skipped to avoid duplicate hooks"
        );
    } else {
        warn!(
            recovery_reason = reason,
            "Windows power/session change detected while listener is down"
        );
    }
}

fn with_evaluator_lock<T>(
    evaluator: &Arc<Mutex<Evaluator>>,
    operation: &'static str,
    action: impl FnOnce(&mut Evaluator) -> T,
) -> Option<T> {
    let lock_wait_started = Instant::now();
    let mut lock = match evaluator.lock() {
        Ok(lock) => lock,
        Err(error) => {
            error!(
                operation,
                error = %error,
                "Evaluator mutex poisoned inside hook callback"
            );
            return None;
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
        EngineEvent::ActionDelimiter => "action_delimiter",
        EngineEvent::Char(_) => "char",
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
    let _ = with_evaluator_lock(evaluator, "clear_undo_state", |lock| {
        lock.state.clear_undo_state();
    });
}

#[cfg(not(target_os = "linux"))]
fn take_active_undo_state(evaluator: &Arc<Mutex<Evaluator>>) -> Option<(String, usize)> {
    with_evaluator_lock(evaluator, "take_active_undo_state", |lock| {
        lock.state
            .take_active_undo_state()
            .map(|undo| (undo.trigger_string, undo.output_length))
    })
    .flatten()
}

#[cfg(not(target_os = "linux"))]
fn spawn_undo_dispatch(trigger_string: String, output_length: usize) {
    injector::spawn_guarded_injection_thread("taurine-undo-dispatch", move || {
        injector::inject_undo(trigger_string, output_length);
    });
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
    injector::spawn_guarded_injection_thread("taurine-expansion-dispatch", move || {
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

fn completion_is_active(evaluator: &Arc<Mutex<Evaluator>>) -> bool {
    with_evaluator_lock(evaluator, "completion_is_active", |lock| {
        lock.is_completion_active()
    })
    .unwrap_or(false)
}

pub(crate) fn trigger_assist_is_active(
    evaluator: &Arc<Mutex<Evaluator>>,
    state: &taurine_core::engine::EngineState,
) -> bool {
    !matches!(
        state.engine_mode(),
        taurine_core::engine::EngineMode::AiCapture { .. }
    ) && completion_is_active(evaluator)
}

pub(crate) fn spawn_completion_rewrite_dispatch(
    rewrite: taurine_core::engine::CompletionRewrite,
    spinner_style: taurine_core::settings::SpinnerStyle,
) {
    injector::spawn_guarded_injection_thread("taurine-completion-rewrite", move || {
        dispatch_completion_rewrite_with(rewrite, spinner_style, crate::injector::inject_expansion);
    });
}

fn dispatch_completion_rewrite_with<I>(
    rewrite: taurine_core::engine::CompletionRewrite,
    spinner_style: taurine_core::settings::SpinnerStyle,
    inject: I,
) where
    I: FnOnce(
        Vec<taurine_core::engine::variables::ExpansionStep>,
        usize,
        taurine_core::settings::SpinnerStyle,
    ) -> crate::injector::InjectionReport,
{
    let taurine_core::engine::CompletionRewrite {
        delete_count,
        replacement,
    } = rewrite;
    debug!(
        delete_count,
        replacement_chars = replacement.chars().count(),
        "Starting completion rewrite dispatch"
    );
    let injection = inject(
        vec![taurine_core::engine::variables::ExpansionStep::Text(
            replacement,
        )],
        delete_count,
        spinner_style,
    );
    debug!(
        delete_count,
        completed = injection.completed,
        output_chars = injection.successful_chars,
        "Finished completion rewrite dispatch"
    );
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
    ) -> crate::injector::InjectionReport,
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
        metric_kind,
        track_usage,
        follow_up,
    } = expansion;
    let step_count = steps.len();
    let has_follow_up = follow_up.is_some();

    state.clear_undo_state();
    debug!(
        delete_count,
        step_count, has_follow_up, track_usage, "Starting expansion dispatch"
    );
    let injection = inject_expansion(steps, delete_count, spinner_style);
    debug!(
        delete_count,
        step_count,
        completed = injection.completed,
        output_chars = injection.successful_chars,
        has_follow_up,
        "Finished expansion dispatch"
    );
    if track_usage && delete_count > 0 && (injection.completed || injection.successful_chars > 0) {
        state.record_word_trigger_usage(&trigger);
    }
    if follow_up.is_none()
        && injection.successful_chars > 0
        && let Some(undo_trigger) = undo_trigger
    {
        state.set_undo_state(undo_trigger, injection.successful_chars);
    }
    launch_follow_up_fn(follow_up, spinner_style, runtime_handle);

    if track_usage {
        taurine_core::db::crud::record_automation_metric(
            taurine_core::db::crud::AutomationMetricEvent {
                automation_trigger: Some(trigger.clone()),
                trigger_chars: trigger.chars().count(),
                success: injection.completed,
                output_chars: injection.successful_chars,
                kind: if is_calculation {
                    taurine_core::db::crud::AutomationMetricKind::Calculation
                } else {
                    metric_kind
                },
                wpm: None,
            },
        );
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
        info!("Starting inline AI follow-up dispatch");
        let injection_guard = crate::injector::InjectionFlagGuard::begin();

        let spinner_handle = taurine_core::utils::spinner::spawn_async(
            spinner_style,
            crate::platform::spinner_renderer::OsSpinnerRenderer::default(),
            &runtime_handle,
        );
        runtime_handle.spawn(async move {
            let _guard = injection_guard;
            crate::engine::ai::stream::run_inline_ai_stream(
                prompt,
                system_prompt_override,
                spinner_handle,
            )
            .await;
            info!("Finished inline AI follow-up dispatch");
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
            metric_kind: taurine_core::db::crud::AutomationMetricKind::InlineAi,
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
                    .push("inject");
                crate::injector::InjectionReport::default()
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
            metric_kind: taurine_core::db::crud::AutomationMetricKind::Snippet,
            track_usage: false,
            follow_up: None,
        };

        dispatch_expansion_with(
            expansion,
            taurine_core::settings::SpinnerStyle::default(),
            rt.handle().clone(),
            state.clone(),
            move |_, _, _| crate::injector::InjectionReport {
                successful_chars: "Good Morning".chars().count(),
                completed: true,
            },
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
            metric_kind: taurine_core::db::crud::AutomationMetricKind::Hotkey,
            track_usage: false,
            follow_up: None,
        };

        dispatch_expansion_with(
            expansion,
            taurine_core::settings::SpinnerStyle::default(),
            rt.handle().clone(),
            state.clone(),
            move |_, _, _| crate::injector::InjectionReport {
                successful_chars: "git status".chars().count(),
                completed: true,
            },
            move |_, _, _| {},
        );

        assert!(state.take_active_undo_state().is_none());
    }

    #[test]
    fn completion_rewrite_dispatch_uses_single_bulk_text_step() {
        let captured = Arc::new(Mutex::new(None));
        let captured_clone = captured.clone();

        dispatch_completion_rewrite_with(
            taurine_core::engine::CompletionRewrite {
                delete_count: 5,
                replacement: "gco".to_string(),
            },
            taurine_core::settings::SpinnerStyle::default(),
            move |steps, delete_count, _| {
                *captured_clone.lock().expect("capture poisoned") = Some((steps, delete_count));
                crate::injector::InjectionReport::default()
            },
        );

        let (steps, delete_count) = captured
            .lock()
            .expect("capture poisoned")
            .clone()
            .expect("rewrite should be captured");
        assert_eq!(delete_count, 5);
        assert_eq!(
            steps,
            vec![taurine_core::engine::variables::ExpansionStep::Text(
                "gco".to_string()
            )]
        );
    }

    #[test]
    fn completion_key_action_wraps_plain_and_shift_tab_into_cycle_actions() {
        assert_eq!(
            completion_key_action(CompletionKeyKind::Tab, false, false, false, false),
            CompletionKeyAction::CycleForward
        );
        assert_eq!(
            completion_key_action(CompletionKeyKind::Tab, true, false, false, false),
            CompletionKeyAction::CycleBackward
        );
    }

    #[test]
    fn completion_key_action_treats_modified_tabs_as_pass_through_cancels() {
        assert_eq!(
            completion_key_action(CompletionKeyKind::Tab, false, false, true, false),
            CompletionKeyAction::CancelAndPassThrough
        );
        assert_eq!(
            completion_key_action(CompletionKeyKind::Tab, false, true, false, false),
            CompletionKeyAction::CancelAndPassThrough
        );
        assert_eq!(
            completion_key_action(CompletionKeyKind::Tab, true, true, false, false),
            CompletionKeyAction::CancelAndPassThrough
        );
        assert_eq!(
            completion_key_action(CompletionKeyKind::Tab, false, false, false, true),
            CompletionKeyAction::CancelAndPassThrough
        );
    }

    #[test]
    fn completion_key_action_swallows_escape_and_vertical_navigation() {
        assert_eq!(
            completion_key_action(CompletionKeyKind::Escape, false, false, false, false),
            CompletionKeyAction::CancelAndSwallow
        );
        assert_eq!(
            completion_key_action(CompletionKeyKind::Up, false, false, false, false),
            CompletionKeyAction::HistoryOlder
        );
        assert_eq!(
            completion_key_action(CompletionKeyKind::Down, false, false, false, false),
            CompletionKeyAction::HistoryNewer
        );
    }

    #[test]
    fn completion_key_kind_from_tab_like_maps_expected_keys() {
        assert_eq!(
            completion_key_kind_from_tab_like(true, false, false, false),
            CompletionKeyKind::Tab
        );
        assert_eq!(
            completion_key_kind_from_tab_like(false, true, false, false),
            CompletionKeyKind::Escape
        );
        assert_eq!(
            completion_key_kind_from_tab_like(false, false, true, false),
            CompletionKeyKind::Up
        );
        assert_eq!(
            completion_key_kind_from_tab_like(false, false, false, true),
            CompletionKeyKind::Down
        );
        assert_eq!(
            completion_key_kind_from_tab_like(false, false, false, false),
            CompletionKeyKind::Other
        );
    }

    #[test]
    fn trigger_assist_key_action_passes_tab_through_when_tab_completion_is_disabled() {
        use std::sync::atomic::Ordering;

        let state = taurine_core::engine::EngineState::new('>');
        state
            .inline_tab_completion_enabled
            .store(false, Ordering::Relaxed);

        assert_eq!(
            trigger_assist_key_action(&state, CompletionKeyKind::Tab, false, false, false, false),
            CompletionKeyAction::PassThrough
        );
    }

    #[test]
    fn trigger_assist_key_action_passes_history_through_when_history_is_disabled() {
        use std::sync::atomic::Ordering;

        let state = taurine_core::engine::EngineState::new('>');
        state.inline_history_enabled.store(false, Ordering::Relaxed);

        assert_eq!(
            trigger_assist_key_action(&state, CompletionKeyKind::Up, false, false, false, false),
            CompletionKeyAction::PassThrough
        );
        assert_eq!(
            trigger_assist_key_action(&state, CompletionKeyKind::Down, false, false, false, false),
            CompletionKeyAction::PassThrough
        );
    }

    #[test]
    fn trigger_assist_key_release_swallowing_respects_feature_settings() {
        use std::sync::atomic::Ordering;

        let state = taurine_core::engine::EngineState::new('>');
        assert!(should_swallow_trigger_assist_key_release(
            &state,
            CompletionKeyKind::Tab
        ));
        assert!(should_swallow_trigger_assist_key_release(
            &state,
            CompletionKeyKind::Up
        ));

        state
            .inline_tab_completion_enabled
            .store(false, Ordering::Relaxed);
        state.inline_history_enabled.store(false, Ordering::Relaxed);

        assert!(!should_swallow_trigger_assist_key_release(
            &state,
            CompletionKeyKind::Tab
        ));
        assert!(!should_swallow_trigger_assist_key_release(
            &state,
            CompletionKeyKind::Down
        ));
    }

    #[test]
    fn completion_is_inactive_after_trigger_character_is_deleted() {
        let state = Arc::new(taurine_core::engine::EngineState::new('>'));
        let mut evaluator = taurine_core::engine::Evaluator::new(state);
        for ch in ">g".chars() {
            assert_eq!(
                evaluator.process_event(if ch == ' ' {
                    taurine_core::engine::EngineEvent::ActionDelimiter
                } else {
                    taurine_core::engine::EngineEvent::Char(ch)
                }),
                None
            );
        }
        assert_eq!(
            evaluator.process_event(taurine_core::engine::EngineEvent::Backspace),
            None
        );
        assert_eq!(
            evaluator.process_event(taurine_core::engine::EngineEvent::Backspace),
            None
        );

        let evaluator = Arc::new(Mutex::new(evaluator));
        assert!(
            !completion_is_active(&evaluator),
            "hook gating must not treat deleted-trigger state as active completion"
        );
    }

    #[test]
    fn dispatch_expansion_promotes_word_trigger_history_on_success() {
        let state = Arc::new(taurine_core::engine::EngineState::new('>'));
        state.load_actions(vec![
            (
                "email".to_string(),
                taurine_core::db::crud::AutomationAction::text("team update"),
            ),
            (
                "gs".to_string(),
                taurine_core::db::crud::AutomationAction::text("git status"),
            ),
        ]);
        state.load_word_trigger_history(vec!["email".to_string(), "gs".to_string()]);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");

        let expansion = taurine_core::engine::ExpansionResult {
            delete_count: 4,
            steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
                "git status".to_string(),
            )],
            trigger: "gs".to_string(),
            undo_trigger: Some(">gs".to_string()),
            is_calculation: false,
            metric_kind: taurine_core::db::crud::AutomationMetricKind::Snippet,
            track_usage: true,
            follow_up: None,
        };

        dispatch_expansion_with(
            expansion,
            taurine_core::settings::SpinnerStyle::default(),
            rt.handle().clone(),
            state.clone(),
            move |_, _, _| crate::injector::InjectionReport {
                successful_chars: "git status".chars().count(),
                completed: true,
            },
            move |_, _, _| {},
        );

        assert_eq!(
            state.matching_word_trigger_history(""),
            vec!["gs".to_string(), "email".to_string()]
        );
    }

    #[test]
    fn trigger_assist_is_inactive_while_inline_ai_capture_mode_is_active() {
        let state = Arc::new(taurine_core::engine::EngineState::new('>'));
        let mut evaluator = taurine_core::engine::Evaluator::new(state.clone());

        for ch in ">ai".chars() {
            assert_eq!(
                evaluator.process_event(if ch == ' ' {
                    taurine_core::engine::EngineEvent::ActionDelimiter
                } else {
                    taurine_core::engine::EngineEvent::Char(ch)
                }),
                None
            );
        }

        let expansion = evaluator
            .process_event(taurine_core::engine::EngineEvent::ActionDelimiter)
            .expect("inline ai capture should start");
        assert_eq!(expansion.trigger, "ai");

        let evaluator = Arc::new(Mutex::new(evaluator));
        assert!(
            !trigger_assist_is_active(&evaluator, state.as_ref()),
            "history and completion keys must not be hijacked once AI capture is active"
        );
    }
}
