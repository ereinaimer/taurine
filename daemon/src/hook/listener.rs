use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
use tracing::{debug, error, info, trace, warn};

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
// SAFETY: The CoreFoundation run loop APIs are stable system APIs.
unsafe extern "C" {
    fn CFRunLoopGetCurrent() -> *mut std::ffi::c_void;
    fn CFRunLoopStop(rl: *mut std::ffi::c_void);
}

#[cfg(target_os = "macos")]
#[derive(Copy, Clone)]
struct SendPtr(*mut std::ffi::c_void);

#[cfg(target_os = "macos")]
// SAFETY: CoreFoundation run loop references are thread-safe to send between threads.
unsafe impl Send for SendPtr {}

#[cfg(target_os = "macos")]
static MACOS_RUN_LOOP: std::sync::Mutex<Option<SendPtr>> = std::sync::Mutex::new(None);

#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType, Key};

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

#[cfg(not(target_os = "linux"))]
use super::completion::{
    completion_key_kind_from_tab_like, should_swallow_trigger_assist_key_release,
    trigger_assist_is_active, trigger_assist_key_action,
};
#[cfg(not(target_os = "linux"))]
use super::dispatch::{clear_undo_state, spawn_undo_dispatch, take_active_undo_state};
use super::dispatch::{spawn_completion_rewrite_dispatch, spawn_expansion_dispatch};

#[cfg(not(target_os = "linux"))]
pub(super) static LISTENER_EPOCH: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

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
pub(super) fn run_listener_once(
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
) -> Result<u64, String> {
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
    let my_epoch = LISTENER_EPOCH.load(Ordering::SeqCst);

    let callback = move |event: Event| -> Option<Event> {
        if LISTENER_EPOCH.load(Ordering::Relaxed) != my_epoch {
            return Some(event);
        }

        #[cfg(windows)]
        if matches!(
            event.event_type,
            EventType::KeyPress(Key::Unknown(255)) | EventType::KeyRelease(Key::Unknown(255))
        ) {
            if let Some(health) = callback_health.as_ref() {
                health.record_keyboard_event();
            }
            return None;
        }

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
            clear_undo_state(state.as_ref());
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
                clear_undo_state(state.as_ref());
                if let Ok(mut lock) = hotkey_evaluator.lock() {
                    lock.clear();
                }
                if paused.load(Ordering::Relaxed) {
                    return Some(event);
                }
                let _ = with_evaluator_lock(&evaluator, "button_interrupt", |lock| {
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

                if trigger_assist_is_active(&evaluator, state.as_ref()) {
                    clear_undo_state(state.as_ref());

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
                        super::completion::CompletionKeyAction::CycleForward => {
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
                        super::completion::CompletionKeyAction::CycleBackward => {
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
                        super::completion::CompletionKeyAction::HistoryOlder => {
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
                        super::completion::CompletionKeyAction::HistoryNewer => {
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
                        super::completion::CompletionKeyAction::CancelAndSwallow => {
                            let _ = with_evaluator_lock(
                                &evaluator,
                                "cancel_completion_swallow",
                                |lock| {
                                    lock.cancel_completion();
                                },
                            );
                            return None;
                        }
                        super::completion::CompletionKeyAction::CancelAndPassThrough => {
                            let _ = with_evaluator_lock(
                                &evaluator,
                                "cancel_completion_pass_through",
                                |lock| {
                                    lock.cancel_completion();
                                },
                            );
                        }
                        super::completion::CompletionKeyAction::PassThrough => {}
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
                    let needs_window = matches!(ev, EngineEvent::ActionDelimiter)
                        || (matches!(ev, EngineEvent::Char(_))
                            && (state.instant_expand.load(Ordering::Relaxed)
                                || state.triggerless_mode.load(Ordering::Relaxed)));

                    let active_window = if needs_window {
                        crate::platform::get_active_window_label()
                    } else {
                        None
                    };

                    if let Some((expansion, state)) =
                        with_evaluator_lock(&evaluator, "process_engine_event", |lock| {
                            lock.process_event(ev, active_window.as_deref())
                                .map(|expansion| {
                                    let state = lock.state.clone();
                                    (expansion, state)
                                })
                        })
                        .flatten()
                    {
                        debug!("Trigger matched: {}", expansion.trigger);

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

    #[cfg(target_os = "macos")]
    {
        // SAFETY: CFRunLoopGetCurrent always returns a valid run loop reference for the current thread.
        let rl = unsafe { CFRunLoopGetCurrent() };
        if let Ok(mut lock) = MACOS_RUN_LOOP.lock() {
            *lock = Some(SendPtr(rl));
        }
    }

    info!("Hook listener entering rdev::grab");
    #[cfg(windows)]
    windows_grab(callback).map_err(|error| format!("{error:?}"))?;
    #[cfg(not(windows))]
    rdev::grab(callback).map_err(|error| format!("{error:?}"))?;
    Ok(my_epoch)
}

/// Windows-specific keyboard hook that owns its HHOOK on a per-thread basis.
///
/// This replaces `rdev::grab` on Windows to eliminate rdev 0.5.3's global mutable
/// static race condition. rdev stores the hook handle in `static mut HOOK: HHOOK`
/// which is shared across all threads in the process. When the supervisor tears down
/// one listener thread and immediately spawns a new one, both threads write to and
/// read from this global simultaneously, producing an Access Violation (SEH exception)
/// that bypasses Rust's `catch_unwind` and crashes the daemon.
///
/// This implementation:
/// - Stores `HHOOK` in a `thread_local!` — each listener thread owns its own handle.
/// - Calls `UnhookWindowsHookEx` explicitly before the thread exits, so Windows
///   removes the hook cleanly rather than waiting for the 1-second OS timeout.
/// - Drops the mouse hook entirely; we only need keyboard events.
/// - Inlines key-name resolution via `ToUnicodeEx` so we do not rely on rdev internals.
#[cfg(windows)]
fn windows_grab<F>(callback: F) -> Result<(), String>
where
    F: FnMut(rdev::Event) -> Option<rdev::Event> + 'static,
{
    use std::cell::RefCell;
    use std::time::SystemTime;
    use windows_sys::Win32::Foundation::LPARAM;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, GetKeyboardState, ToUnicodeEx, VIRTUAL_KEY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, HC_ACTION, MSG,
        SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    thread_local! {
        // The HHOOK handle for this thread's keyboard hook.
        // *mut std::ffi::c_void because HHOOK is a pointer type.
        static TL_HHOOK: RefCell<*mut std::ffi::c_void> = const { RefCell::new(std::ptr::null_mut()) };

        // The event callback for this thread's hook invocation.
        #[allow(clippy::type_complexity)]
        static TL_CALLBACK: RefCell<Option<Box<dyn FnMut(rdev::Event) -> Option<rdev::Event>>>> =
            const { RefCell::new(None) };

        // Stateful keyboard decoder (dead-key handling, modifier state).
        static TL_KEYBOARD: RefCell<KeyboardDecoder> = RefCell::new(KeyboardDecoder::new());
    }

    TL_CALLBACK.with(|cb| *cb.borrow_mut() = Some(Box::new(callback)));

    // SAFETY: This is the Win32 low-level keyboard hook procedure. It is called by
    // Windows on the thread that installed the hook, so thread-local access is valid.
    // `code`, `wparam`, and `lparam` are provided by Windows and are always valid
    // for a WH_KEYBOARD_LL hook. `lparam` points to a KBDLLHOOKSTRUCT that is valid
    // for the duration of this call.
    unsafe extern "system" fn ll_keyboard_proc(code: i32, wparam: usize, lparam: LPARAM) -> isize {
        unsafe {
            if code == HC_ACTION as i32 {
                // Parse KBDLLHOOKSTRUCT: { vkCode: u32, scanCode: u32, flags: u32, time: u32, dwExtraInfo: usize }
                // SAFETY: lparam is a valid pointer to KBDLLHOOKSTRUCT for the duration of this call.
                let kbds = lparam as *const [u32; 5];
                let vk_code = (*kbds)[0] as u16;
                let scan_code = (*kbds)[1];
                let is_press =
                    matches!(wparam, w if w == WM_KEYDOWN as usize || w == WM_SYSKEYDOWN as usize);

                let key = vk_to_rdev_key(vk_code);
                let event_type = if is_press {
                    rdev::EventType::KeyPress(key)
                } else {
                    rdev::EventType::KeyRelease(key)
                };

                // Resolve the Unicode name of this keypress (for character events).
                // Only needed on KeyPress; KeyRelease never produces a character.
                let name = if is_press {
                    TL_KEYBOARD.with(|kb| {
                        // SAFETY: called from the hook proc; Windows guarantees the
                        // foreground window and keyboard layout are valid here.
                        kb.borrow_mut().get_name(vk_code as u32, scan_code)
                    })
                } else {
                    None
                };

                let event = rdev::Event {
                    event_type,
                    time: SystemTime::now(),
                    name,
                };

                let pass_through = TL_CALLBACK.with(|cb| {
                    cb.borrow_mut()
                        .as_mut()
                        .map(|f| f(event).is_some())
                        .unwrap_or(true)
                });

                if !pass_through {
                    // Swallowed — do NOT call next hook.
                    return 1;
                }
            }

            let hhook = TL_HHOOK.with(|h| *h.borrow());
            // SAFETY: CallNextHookEx is always safe to call from within a hook proc.
            // hhook is the handle for this thread's keyboard hook.
            CallNextHookEx(hhook, code, wparam, lparam)
        }
    }

    // SAFETY: SetWindowsHookExW installs a global WH_KEYBOARD_LL hook.
    // - `ll_keyboard_proc` is a valid `extern "system"` function.
    // - hmod=null_mut() and dwThreadId=0 are correct for a global low-level hook (MSDN).
    // - The hook will fire on the thread running GetMessageW below.
    let hhook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(ll_keyboard_proc),
            std::ptr::null_mut(),
            0,
        )
    };
    if hhook.is_null() {
        return Err(format!(
            "SetWindowsHookExW(WH_KEYBOARD_LL) failed with error {}",
            // SAFETY: GetLastError() is always safe to call.
            unsafe { windows_sys::Win32::Foundation::GetLastError() }
        ));
    }

    TL_HHOOK.with(|h| *h.borrow_mut() = hhook);

    // Run the message pump. WM_QUIT (posted by send_wm_quit_to_thread in the
    // supervisor) causes GetMessageW to return 0, exiting the loop cleanly.
    // SAFETY: GetMessageW is safe to call; it blocks until a message arrives.
    // Passing null_mut() for hwnd, and 0 for min and max means all messages for this thread.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            // No TranslateMessage/DispatchMessageW needed: low-level keyboard
            // hooks are delivered directly through GetMessageW, not through
            // window messages dispatched to a WNDPROC.
        }
    }

    // Explicitly unhook before this thread exits. This is the critical difference
    // from rdev: rdev never calls UnhookWindowsHookEx, so Windows must wait for
    // the OS-level hook timeout (1 second by default) before cleaning up. Explicit
    // unhooking makes teardown deterministic and prevents Windows from treating the
    // newly-spawned hook thread's handle as stale.
    // SAFETY: hhook is the handle we registered above on this thread. Unhooking
    // from the same thread that hooked is always valid.
    unsafe {
        UnhookWindowsHookEx(hhook);
    }
    TL_HHOOK.with(|h| *h.borrow_mut() = std::ptr::null_mut());
    TL_CALLBACK.with(|cb| cb.borrow_mut().take());

    Ok(())
}

/// Decoder that resolves Windows virtual-key codes to Unicode strings using
/// `ToUnicodeEx`, handling dead keys and modifier state the same way rdev does.
#[cfg(windows)]
struct KeyboardDecoder {
    last_code: u32,
    last_scan_code: u32,
    last_state: [u8; 256],
    last_is_dead: bool,
}

#[cfg(windows)]
impl KeyboardDecoder {
    fn new() -> Self {
        Self {
            last_code: 0,
            last_scan_code: 0,
            last_state: [0u8; 256],
            last_is_dead: false,
        }
    }

    // SAFETY: Must be called from within the WH_KEYBOARD_LL hook proc, where
    // GetForegroundWindow, GetWindowThreadProcessId, GetKeyboardLayout, and
    // GetKeyboardState are all valid to call.
    unsafe fn get_name(&mut self, vk_code: u32, scan_code: u32) -> Option<String> {
        use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            GetKeyboardLayout, GetKeyboardState, ToUnicodeEx,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

        unsafe {
            let mut state = [0u8; 256];
            let fg_window = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg_window, std::ptr::null_mut());
            let current_thread = GetCurrentThreadId();

            // Attach to the foreground window's input thread to retrieve its synchronized keyboard state.
            // If attachment fails (e.g. if the foreground window is in a different desktop or elevated),
            // fallback to the current thread's state.
            let attached = fg_thread != 0
                && fg_thread != current_thread
                && AttachThreadInput(current_thread, fg_thread, 1) != 0;

            let ok = GetKeyboardState(state.as_mut_ptr());

            if attached {
                AttachThreadInput(current_thread, fg_thread, 0);
            }

            if ok == 0 {
                return None;
            }
            self.last_state = state;

            // SAFETY: GetKeyboardLayout returns the layout for the given thread.
            let layout = GetKeyboardLayout(fg_thread);

            let mut buf = [0u16; 8];
            // SAFETY: ToUnicodeEx writes at most `buf.len()` UTF-16 code units.
            let len = ToUnicodeEx(
                vk_code,
                scan_code,
                self.last_state.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i32 - 1,
                0,
                layout,
            );

            let mut is_dead = false;
            let result = match len {
                0 => None,
                -1 => {
                    is_dead = true;
                    // Clear dead key from the ToUnicodeEx state machine.
                    // SAFETY: same guarantees as above.
                    let mut clear_buf = [0u16; 8];
                    let mut empty_state = [0u8; 256];
                    let mut clear_len = -1i32;
                    while clear_len < 0 {
                        clear_len = ToUnicodeEx(
                            vk_code,
                            scan_code,
                            empty_state.as_mut_ptr(),
                            clear_buf.as_mut_ptr(),
                            clear_buf.len() as i32,
                            0,
                            layout,
                        );
                    }
                    None
                }
                n if n > 0 => String::from_utf16(&buf[..n as usize]).ok(),
                _ => None,
            };

            // Replay the previous dead key if one was pending, matching rdev behavior.
            if self.last_code != 0 && self.last_is_dead {
                let mut replay_buf = [0u16; 8];
                // SAFETY: same guarantees; replaying dead key through ToUnicodeEx.
                ToUnicodeEx(
                    self.last_code,
                    self.last_scan_code,
                    self.last_state.as_mut_ptr(),
                    replay_buf.as_mut_ptr(),
                    replay_buf.len() as i32,
                    0,
                    layout,
                );
                self.last_code = 0;
            } else {
                self.last_code = vk_code;
                self.last_scan_code = scan_code;
                self.last_is_dead = is_dead;
            }

            result
        }
    }
}

/// Map a Windows virtual-key code to an `rdev::Key`, matching rdev's keycodes.rs table.
/// Unknown VK codes become `Key::Unknown(vk_code as u32)`.
#[cfg(windows)]
fn vk_to_rdev_key(vk: u16) -> rdev::Key {
    use rdev::Key;
    match vk {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => Key::Return,
        0x13 => Key::Pause,
        0x14 => Key::CapsLock,
        0x1B => Key::Escape,
        0x20 => Key::Space,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::LeftArrow,
        0x26 => Key::UpArrow,
        0x27 => Key::RightArrow,
        0x28 => Key::DownArrow,
        0x2C => Key::PrintScreen,
        0x2D => Key::Insert,
        0x2E => Key::Delete,
        0x30 => Key::Num0,
        0x31 => Key::Num1,
        0x32 => Key::Num2,
        0x33 => Key::Num3,
        0x34 => Key::Num4,
        0x35 => Key::Num5,
        0x36 => Key::Num6,
        0x37 => Key::Num7,
        0x38 => Key::Num8,
        0x39 => Key::Num9,
        0x41 => Key::KeyA,
        0x42 => Key::KeyB,
        0x43 => Key::KeyC,
        0x44 => Key::KeyD,
        0x45 => Key::KeyE,
        0x46 => Key::KeyF,
        0x47 => Key::KeyG,
        0x48 => Key::KeyH,
        0x49 => Key::KeyI,
        0x4A => Key::KeyJ,
        0x4B => Key::KeyK,
        0x4C => Key::KeyL,
        0x4D => Key::KeyM,
        0x4E => Key::KeyN,
        0x4F => Key::KeyO,
        0x50 => Key::KeyP,
        0x51 => Key::KeyQ,
        0x52 => Key::KeyR,
        0x53 => Key::KeyS,
        0x54 => Key::KeyT,
        0x55 => Key::KeyU,
        0x56 => Key::KeyV,
        0x57 => Key::KeyW,
        0x58 => Key::KeyX,
        0x59 => Key::KeyY,
        0x5A => Key::KeyZ,
        0x5B => Key::MetaLeft,
        0x60 => Key::Kp0,
        0x61 => Key::Kp1,
        0x62 => Key::Kp2,
        0x63 => Key::Kp3,
        0x64 => Key::Kp4,
        0x65 => Key::Kp5,
        0x66 => Key::Kp6,
        0x67 => Key::Kp7,
        0x68 => Key::Kp8,
        0x69 => Key::Kp9,
        0x6A => Key::KpMultiply,
        0x6B => Key::KpPlus,
        0x6D => Key::KpMinus,
        0x6E => Key::KpDelete,
        0x6F => Key::KpDivide,
        0x70 => Key::F1,
        0x71 => Key::F2,
        0x72 => Key::F3,
        0x73 => Key::F4,
        0x74 => Key::F5,
        0x75 => Key::F6,
        0x76 => Key::F7,
        0x77 => Key::F8,
        0x78 => Key::F9,
        0x79 => Key::F10,
        0x7A => Key::F11,
        0x7B => Key::F12,
        0x90 => Key::NumLock,
        0x91 => Key::ScrollLock,
        0xA0 => Key::ShiftLeft,
        0xA1 => Key::ShiftRight,
        0xA2 => Key::ControlLeft,
        0xA3 => Key::ControlRight,
        0xA4 => Key::Alt,
        0xA5 => Key::AltGr,
        0xBA => Key::SemiColon,
        0xBB => Key::Equal,
        0xBC => Key::Comma,
        0xBD => Key::Minus,
        0xBE => Key::Dot,
        0xBF => Key::Slash,
        0xC0 => Key::BackQuote,
        0xDB => Key::LeftBracket,
        0xDC => Key::BackSlash,
        0xDD => Key::RightBracket,
        0xDE => Key::Quote,
        0xE2 => Key::IntlBackslash,
        vk => Key::Unknown(vk as u32),
    }
}

#[allow(dead_code)]
pub fn stop_listener() {
    #[cfg(target_os = "macos")]
    {
        if let Ok(mut lock) = MACOS_RUN_LOOP.lock()
            && let Some(SendPtr(rl)) = lock.take()
        {
            // SAFETY: CFRunLoopStop is safe to call from any thread with a valid CFRunLoopRef.
            unsafe {
                CFRunLoopStop(rl);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::input_supervisor::stop();
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_windows_hook_listener(
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
    supervisor_tx: std::sync::mpsc::Sender<super::supervisor::WindowsSupervisorEvent>,
) -> super::supervisor::ListenerHandle {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    hook_health.mark_listener_started();
    info!("Starting supervised Windows hook listener thread");
    let listener_health = hook_health.clone();
    let fallback_tx = supervisor_tx.clone();

    let (thread_id_tx, thread_id_rx) = std::sync::mpsc::channel::<u32>();

    let spawn_result = std::thread::Builder::new()
        .name("tau-hook-listn".to_string())
        .spawn(move || {
            // SAFETY: GetCurrentThreadId() returns the OS thread ID of the calling
            // thread. It always succeeds, has no failure mode, and requires no
            // arguments. The returned DWORD is valid for the lifetime of the thread
            // and is used later by the supervisor to post WM_QUIT during teardown.
            let os_thread_id = unsafe { GetCurrentThreadId() };
            if thread_id_tx.send(os_thread_id).is_err() {
                warn!(
                    "Supervisor dropped the thread-id channel; listener thread is abandoning itself"
                );
                return;
            }

            // SAFETY: GetCurrentThread() returns a pseudo-handle for the current thread
            // which always succeeds. SetThreadPriority boosts the priority of this thread
            // to THREAD_PRIORITY_HIGHEST so that DWM and the OS scheduler prioritize delivering
            // events to the hook, preventing silent hook termination due to timeout under load.
            unsafe {
                let current_thread = windows_sys::Win32::System::Threading::GetCurrentThread();
                if windows_sys::Win32::System::Threading::SetThreadPriority(
                    current_thread,
                    windows_sys::Win32::System::Threading::THREAD_PRIORITY_HIGHEST,
                ) == 0
                {
                    warn!("Failed to boost hook listener thread priority");
                } else {
                    info!("Successfully boosted hook listener thread priority to HIGHEST");
                }
            }

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

            let (exit_error, exit_epoch) = match result {
                Ok(Ok(epoch)) => (None, Some(epoch)),
                Ok(Err(error)) => (Some(error), None),
                Err(_) => (Some("Windows hook listener panicked".to_string()), None),
            };

            if let Some(ref error) = exit_error {
                error!(error = %error, "Windows hook listener is exiting");
            } else {
                warn!("Windows hook listener returned unexpectedly without an error");
            }

            let current_epoch = LISTENER_EPOCH.load(Ordering::SeqCst);
            let is_evicted = exit_epoch.is_some_and(|epoch| epoch != current_epoch);
            if is_evicted {
                info!(
                    exit_epoch = exit_epoch.unwrap(),
                    current_epoch,
                    "Listener was evicted by recovery; suppressing stale exit notification"
                );
                return;
            }

            if let Err(error) =
                supervisor_tx.send(super::supervisor::WindowsSupervisorEvent::ListenerExited {
                    error: exit_error,
                })
            {
                error!(
                    error = %error,
                    "Failed to notify hook supervisor that the listener exited"
                );
            }
        });

    let join = match spawn_result {
        Ok(handle) => handle,
        Err(error) => {
            let message = format!("Failed to spawn Windows hook listener thread: {error}");
            hook_health.mark_listener_exit(Some(message.clone()));
            error!(error = %message, "Unable to spawn Windows hook listener");
            let _ = fallback_tx.send(super::supervisor::WindowsSupervisorEvent::ListenerExited {
                error: Some(message),
            });
            return super::supervisor::ListenerHandle {
                join: std::thread::Builder::new()
                    .spawn(|| {})
                    .expect("infallible no-op thread spawn"),
                thread_id: 0,
            };
        }
    };

    let thread_id = match thread_id_rx.recv() {
        Ok(id) => id,
        Err(_) => {
            // The listener thread was killed by the OS (e.g. an SEH/structured
            // exception during SetWindowsHookEx right after wakeup) before it
            // could send its thread ID. catch_unwind does not catch SEH on
            // Windows, so the thread drops its stack and the sender is gone.
            // Recover gracefully: mark exit and send ListenerExited so the
            // supervisor retries instead of this .expect() panicking the
            // supervisor thread and crashing the daemon.
            error!(
                "Listener thread terminated before sending OS thread ID; \
                 sending ListenerExited for supervisor retry"
            );
            let msg = "thread killed before sending OS thread ID (SEH on wakeup)".to_string();
            hook_health.mark_listener_exit(Some(msg.clone()));
            let _ = fallback_tx.send(super::supervisor::WindowsSupervisorEvent::ListenerExited {
                error: Some(msg),
            });
            return super::supervisor::ListenerHandle { join, thread_id: 0 };
        }
    };

    super::supervisor::ListenerHandle { join, thread_id }
}

pub(super) fn with_evaluator_lock<T>(
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
