use evdev::{Device, EventType, InputEvent, KeyCode};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

use tracing::{debug, error, info, warn};

use super::xkb::XkbMapper;
use crate::injector::{self, IS_INJECTING};
use crate::input::hotkey::{HotkeySpec, is_pause_chord_evdev};
use crate::input::hotkey_evaluator::{
    HotkeyEvaluation, HotkeyEvaluator, logical_key_from_evdev, modifiers_from_sides,
};
use taurine_core::engine::{EngineEvent, EngineMode, Evaluator};

#[derive(Debug)]
pub(crate) struct DeviceExit {
    pub path: PathBuf,
    pub worker_id: u64,
}

static LAST_PAUSE_TOGGLE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Clone)]
pub(crate) struct ListenerContext {
    evaluator: Arc<Mutex<Evaluator>>,
    state: Arc<taurine_core::engine::EngineState>,
    paused: Arc<AtomicBool>,
    pause_notifications_enabled: Arc<AtomicBool>,
    pause_hotkey: Arc<RwLock<HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    pause_audio_enabled: Arc<AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
    pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
}

impl ListenerContext {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        evaluator: Arc<Mutex<Evaluator>>,
        state: Arc<taurine_core::engine::EngineState>,
        paused: Arc<AtomicBool>,
        pause_notifications_enabled: Arc<AtomicBool>,
        pause_hotkey: Arc<RwLock<HotkeySpec>>,
        spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
        pause_audio_enabled: Arc<AtomicBool>,
        audio_tx: tokio::sync::mpsc::Sender<bool>,
        pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
    ) -> Self {
        Self {
            evaluator,
            state,
            paused,
            pause_notifications_enabled,
            pause_hotkey,
            spinner_style,
            pause_audio_enabled,
            audio_tx,
            pause_transition_tx,
        }
    }
}

#[derive(Default)]
struct ModifierSides {
    left_ctrl: bool,
    right_ctrl: bool,
    left_shift: bool,
    right_shift: bool,
    left_alt: bool,
    right_alt: bool,
    left_meta: bool,
    right_meta: bool,
}

impl ModifierSides {
    fn update(&mut self, key: KeyCode, is_press: bool, is_release: bool) {
        match key {
            KeyCode::KEY_LEFTCTRL => update_flag(&mut self.left_ctrl, is_press, is_release),
            KeyCode::KEY_RIGHTCTRL => update_flag(&mut self.right_ctrl, is_press, is_release),
            KeyCode::KEY_LEFTSHIFT => update_flag(&mut self.left_shift, is_press, is_release),
            KeyCode::KEY_RIGHTSHIFT => update_flag(&mut self.right_shift, is_press, is_release),
            KeyCode::KEY_LEFTALT => update_flag(&mut self.left_alt, is_press, is_release),
            KeyCode::KEY_RIGHTALT => update_flag(&mut self.right_alt, is_press, is_release),
            KeyCode::KEY_LEFTMETA => update_flag(&mut self.left_meta, is_press, is_release),
            KeyCode::KEY_RIGHTMETA => update_flag(&mut self.right_meta, is_press, is_release),
            _ => {}
        }
    }

    fn ctrl_active(&self) -> bool {
        self.left_ctrl || self.right_ctrl
    }

    fn shift_active(&self) -> bool {
        self.left_shift || self.right_shift
    }

    fn alt_active(&self) -> bool {
        self.left_alt || self.right_alt
    }

    fn meta_active(&self) -> bool {
        self.left_meta || self.right_meta
    }

    fn current_modifiers(&self) -> taurine_core::keys::Modifiers {
        modifiers_from_sides(
            self.left_ctrl,
            self.right_ctrl,
            self.left_shift,
            self.right_shift,
            self.left_alt,
            self.right_alt,
            self.left_meta,
            self.right_meta,
        )
    }
}

fn update_flag(flag: &mut bool, is_press: bool, is_release: bool) {
    if is_press {
        *flag = true;
    } else if is_release {
        *flag = false;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    state: Arc<taurine_core::engine::EngineState>,
    paused: Arc<AtomicBool>,
    pause_notifications_enabled: Arc<AtomicBool>,
    pause_hotkey: Arc<RwLock<HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    pause_audio_enabled: Arc<AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
    pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
) {
    let context = ListenerContext::new(
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

    super::input_supervisor::start(context);
}

pub(crate) fn open_keyboard_device(path: &Path) -> io::Result<Option<Device>> {
    let device = Device::open(path)?;

    let name = device.name().unwrap_or("Unknown Device");
    if name == crate::platform::linux::VIRTUAL_DEVICE_NAME {
        debug!("Ignoring Taurine virtual keyboard: {:?}", path);
        return Ok(None);
    }

    if is_keyboard_device(&device) {
        debug!("Found potential keyboard device: {} ({:?})", name, path);
        Ok(Some(device))
    } else {
        Ok(None)
    }
}

pub(crate) fn spawn_device_listener(
    path: PathBuf,
    worker_id: u64,
    mut device: Device,
    context: ListenerContext,
    exit_tx: Sender<DeviceExit>,
) -> io::Result<()> {
    let xkb = XkbMapper::new().map_err(io::Error::other)?;

    thread::Builder::new()
        .name("tau-lnx-evdev".to_string())
        .spawn(move || {
            let mut xkb = xkb;
            let mut modifier_sides = ModifierSides::default();
            let mut hotkey_evaluator = HotkeyEvaluator::new();
            let device_name = device.name().map(|s| s.to_string());
            let grab_enabled = match device.grab() {
                Ok(()) => {
                    info!(
                        "Grabbed evdev device {:?}; Linux undo backspaces can now be swallowed",
                        device_name
                    );
                    true
                }
                Err(e) => {
                    warn!(
                        "Failed to EVIOCGRAB {:?}: {}. Falling back to passive mode; Linux cannot swallow undo backspaces without an exclusive grab.",
                        device_name, e
                    );
                    false
                }
            };
            let mut swallow_next_backspace_release = false;
            let mut swallow_next_left_arrow_release = false;
            let mut swallow_next_right_arrow_release = false;

            loop {
                match device.fetch_events() {
                    Ok(events) => {
                        let mut frame = Vec::new();
                        for event in events {
                            if event.event_type() == EventType::SYNCHRONIZATION {
                                process_frame(
                                    &frame,
                                    grab_enabled,
                                    &context.evaluator,
                                    &context.state,
                                    &context.paused,
                                    &context.pause_notifications_enabled,
                                    &context.pause_hotkey,
                                    &context.spinner_style,
                                    &context.pause_audio_enabled,
                                    &context.audio_tx,
                                    &context.pause_transition_tx,
                                    &mut xkb,
                                    &mut modifier_sides,
                                    &mut hotkey_evaluator,
                                    &mut swallow_next_backspace_release,
                                    &mut swallow_next_left_arrow_release,
                                    &mut swallow_next_right_arrow_release,
                                );
                                frame.clear();
                                continue;
                            }

                            frame.push(event);
                        }

                        if !frame.is_empty() {
                            process_frame(
                                &frame,
                                grab_enabled,
                                &context.evaluator,
                                &context.state,
                                &context.paused,
                                &context.pause_notifications_enabled,
                                &context.pause_hotkey,
                                &context.spinner_style,
                                &context.pause_audio_enabled,
                                &context.audio_tx,
                                &context.pause_transition_tx,
                                &mut xkb,
                                &mut modifier_sides,
                                &mut hotkey_evaluator,
                                &mut swallow_next_backspace_release,
                                &mut swallow_next_left_arrow_release,
                                &mut swallow_next_right_arrow_release,
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Device {:?} disconnected or error: {}", device_name, e);
                        break;
                    }
                }
            }

            let _ = exit_tx.send(DeviceExit { path, worker_id });
        })
        .map(|_| ())
}

fn is_keyboard_device(device: &Device) -> bool {
    device.supported_keys().is_some_and(|keys| {
        keys.contains(KeyCode::KEY_ENTER)
            && keys.contains(KeyCode::KEY_SPACE)
            && keys.contains(KeyCode::KEY_A)
    })
}

#[allow(clippy::too_many_arguments)]
fn process_frame(
    frame: &[InputEvent],
    grab_enabled: bool,
    evaluator: &Arc<Mutex<Evaluator>>,
    state: &Arc<taurine_core::engine::EngineState>,
    paused: &Arc<AtomicBool>,
    _pause_notifications_enabled: &Arc<AtomicBool>,
    pause_hotkey: &Arc<RwLock<HotkeySpec>>,
    spinner_style: &Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    _pause_audio_enabled: &Arc<AtomicBool>,
    _audio_tx: &tokio::sync::mpsc::Sender<bool>,
    pause_transition_tx: &tokio::sync::mpsc::Sender<bool>,
    xkb: &mut XkbMapper,
    modifier_sides: &mut ModifierSides,
    hotkey_evaluator: &mut HotkeyEvaluator,
    swallow_next_backspace_release: &mut bool,
    swallow_next_left_arrow_release: &mut bool,
    swallow_next_right_arrow_release: &mut bool,
) {
    let mut swallow_frame = false;

    for event in frame {
        if event.event_type() != EventType::KEY {
            continue;
        }

        let key = KeyCode::new(event.code());
        let value = event.value();
        let is_press = value == 1;
        let is_release = value == 0;

        if value == 2 {
            continue;
        }

        modifier_sides.update(key, is_press, is_release);

        if *swallow_next_backspace_release && is_release && key == KeyCode::KEY_BACKSPACE {
            swallow_frame = true;
            *swallow_next_backspace_release = false;
            continue;
        }

        if *swallow_next_left_arrow_release && is_release && key == KeyCode::KEY_LEFT {
            swallow_frame = true;
            *swallow_next_left_arrow_release = false;
            continue;
        }

        if *swallow_next_right_arrow_release && is_release && key == KeyCode::KEY_RIGHT {
            swallow_frame = true;
            *swallow_next_right_arrow_release = false;
            continue;
        }

        if is_mouse_button(key) {
            if is_press && !IS_INJECTING.load(Ordering::SeqCst) {
                clear_undo_state(state);
                hotkey_evaluator.clear();
                let mut lock = match evaluator.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        tracing::warn!("evdev evaluator mutex poisoned; recovering");
                        poisoned.into_inner()
                    }
                };
                let _ = lock.process_event(EngineEvent::Interrupt, None);
            }
            continue;
        }

        let logical_key = logical_key_from_evdev(key);
        let engine_mode = state.engine_mode();
        let engine_event = xkb.process_key(key, is_press, engine_mode.clone());
        let modifiers = modifier_sides.current_modifiers();

        if IS_INJECTING.load(Ordering::SeqCst) {
            if is_release {
                if let Some(logical_key) = logical_key {
                    let _ = hotkey_evaluator.on_key_release(logical_key);
                }
            } else if is_press {
                injector::abort_injection();
            }
            continue;
        }

        let trigger_assist_active =
            grab_enabled && crate::hook::trigger_assist_is_active(evaluator, state.as_ref());

        let is_pause_chord = pause_hotkey
            .read()
            .ok()
            .is_some_and(|spec| is_pause_chord_evdev(key, is_press, modifiers, &spec));

        if is_pause_chord {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let last_ms = LAST_PAUSE_TOGGLE_MS.load(Ordering::Relaxed);
            if now_ms.saturating_sub(last_ms) >= 300 {
                LAST_PAUSE_TOGGLE_MS.store(now_ms, Ordering::Relaxed);

                clear_undo_state(state);
                hotkey_evaluator.clear();
                let now_paused = !paused.load(Ordering::Relaxed);
                paused.store(now_paused, Ordering::Relaxed);

                // Notify coordinator
                let _ = pause_transition_tx.try_send(now_paused);
            }
            continue;
        }

        if paused.load(Ordering::Relaxed) {
            continue;
        }

        if is_press {
            let shift_active = modifier_sides.shift_active();
            let ctrl_active = modifier_sides.ctrl_active();
            let alt_active = modifier_sides.alt_active();
            let meta_active = modifier_sides.meta_active();

            if matches!(engine_mode, EngineMode::AiCapture { .. })
                && ctrl_active
                && key == KeyCode::KEY_V
            {
                match crate::platform::read_clipboard_text() {
                    Ok(text) if !text.is_empty() => {
                        let ev = EngineEvent::Paste(text);
                        if let Ok(mut lock) = evaluator.lock() {
                            let _ = lock.process_event(ev, None);
                        }
                    }
                    _ => {}
                }
                swallow_frame = true;
                continue;
            }

            if trigger_assist_active {
                clear_undo_state(state);

                if key == KeyCode::KEY_BACKSPACE && !alt_active && !meta_active {
                    let rewrite = evaluator.lock().ok().and_then(|mut lock| {
                        if ctrl_active {
                            lock.rewrite_word_backspace_query()
                        } else {
                            lock.rewrite_backspace_query()
                        }
                    });

                    if let Some(rewrite) = rewrite {
                        let spinner_style_inner =
                            spinner_style.read().map(|s| *s).unwrap_or_default();
                        crate::hook::spawn_completion_rewrite_dispatch(
                            rewrite,
                            spinner_style_inner,
                        );
                        swallow_frame = true;
                        *swallow_next_backspace_release = true;
                        continue;
                    }
                }

                match crate::hook::trigger_assist_key_action(
                    state.as_ref(),
                    crate::hook::completion_key_kind_from_tab_like(
                        key == KeyCode::KEY_TAB,
                        key == KeyCode::KEY_ESC,
                        key == KeyCode::KEY_UP,
                        key == KeyCode::KEY_DOWN,
                    ),
                    shift_active,
                    ctrl_active,
                    alt_active,
                    meta_active,
                ) {
                    crate::hook::CompletionKeyAction::CycleForward => {
                        let rewrite = evaluator
                            .lock()
                            .ok()
                            .and_then(|mut lock| lock.cycle_completion_next());

                        if let Some(rewrite) = rewrite {
                            let spinner_style_inner =
                                spinner_style.read().map(|s| *s).unwrap_or_default();
                            crate::hook::spawn_completion_rewrite_dispatch(
                                rewrite,
                                spinner_style_inner,
                            );
                        }

                        swallow_frame = true;
                        continue;
                    }
                    crate::hook::CompletionKeyAction::CycleBackward => {
                        let rewrite = evaluator
                            .lock()
                            .ok()
                            .and_then(|mut lock| lock.cycle_completion_prev());

                        if let Some(rewrite) = rewrite {
                            let spinner_style_inner =
                                spinner_style.read().map(|s| *s).unwrap_or_default();
                            crate::hook::spawn_completion_rewrite_dispatch(
                                rewrite,
                                spinner_style_inner,
                            );
                        }

                        swallow_frame = true;
                        continue;
                    }
                    crate::hook::CompletionKeyAction::CancelAndSwallow => {
                        if let Ok(mut lock) = evaluator.lock() {
                            lock.cancel_completion();
                        }
                        swallow_frame = true;
                        continue;
                    }
                    crate::hook::CompletionKeyAction::CancelAndPassThrough => {
                        if let Ok(mut lock) = evaluator.lock() {
                            lock.cancel_completion();
                        }
                    }
                    crate::hook::CompletionKeyAction::PassThrough => {}
                }
            }

            if grab_enabled && let Some(logical_key) = logical_key {
                match hotkey_evaluator.on_key_event(state.as_ref(), true, modifiers, logical_key) {
                    HotkeyEvaluation::Matched(expansion) => {
                        debug!("Hotkey matched: {}", expansion.trigger);
                        swallow_frame = true;

                        let spinner_style_inner =
                            spinner_style.read().map(|s| *s).unwrap_or_default();

                        crate::hook::spawn_expansion_dispatch(
                            expansion,
                            spinner_style_inner,
                            state.clone(),
                        );
                        continue;
                    }
                    HotkeyEvaluation::Swallow => {
                        swallow_frame = true;
                        continue;
                    }
                    HotkeyEvaluation::NoMatch => {}
                }
            }

            let cycle_dir = if state.inline_case_transform_enabled()
                && !matches!(state.engine_mode(), EngineMode::AiCapture { .. })
                && !shift_active
                && !ctrl_active
                && !alt_active
                && !meta_active
            {
                if key == KeyCode::KEY_LEFT {
                    Some(taurine_core::engine::CycleDirection::Prev)
                } else if key == KeyCode::KEY_RIGHT {
                    Some(taurine_core::engine::CycleDirection::Next)
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(dir) = cycle_dir
                && let Some(rewrite) = state.advance_case_variant(dir)
            {
                let spinner_style_inner = spinner_style.read().map(|s| *s).unwrap_or_default();
                crate::hook::spawn_completion_rewrite_dispatch(rewrite, spinner_style_inner);
                swallow_frame = true;
                if key == KeyCode::KEY_LEFT {
                    *swallow_next_left_arrow_release = true;
                } else {
                    *swallow_next_right_arrow_release = true;
                }
                continue;
            }

            if key == KeyCode::KEY_BACKSPACE {
                if ctrl_active || alt_active || meta_active {
                    clear_undo_state(state);
                } else if let Some((trigger_string, output_length)) = take_active_undo_state(state)
                {
                    // Windows/macOS can swallow inside the hook callback itself. Linux needs
                    // EVIOCGRAB plus a uinput proxy, so we drop the grabbed frame instead.
                    spawn_undo_dispatch(trigger_string, output_length);
                    swallow_frame = true;
                    *swallow_next_backspace_release = true;
                    continue;
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
                // Invalidate on any non-modifier or combo.
                clear_undo_state(state);
            }
        } else {
            if grab_enabled
                && trigger_assist_active
                && crate::hook::should_swallow_trigger_assist_key_release(
                    state.as_ref(),
                    crate::hook::completion_key_kind_from_tab_like(
                        key == KeyCode::KEY_TAB,
                        false,
                        key == KeyCode::KEY_UP,
                        key == KeyCode::KEY_DOWN,
                    ),
                )
            {
                swallow_frame = true;
                continue;
            }

            if grab_enabled
                && let Some(logical_key) = logical_key
                && matches!(
                    hotkey_evaluator.on_key_release(logical_key),
                    HotkeyEvaluation::Swallow
                )
            {
                swallow_frame = true;
                continue;
            }
        }

        if paused.load(Ordering::Relaxed) {
            continue;
        }

        if let Some(ev) = engine_event {
            let needs_window =
                matches!(ev, EngineEvent::ActionKey) || matches!(ev, EngineEvent::Char(_));

            let active_window = if needs_window {
                crate::platform::get_active_window_label()
            } else {
                None
            };

            let is_action_key = ev == EngineEvent::ActionKey;

            let mut lock = match evaluator.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    tracing::warn!("evdev evaluator mutex poisoned; recovering");
                    poisoned.into_inner()
                }
            };
            if let Some(expansion) = lock.process_event(ev, active_window.as_deref()) {
                let state = lock.state.clone();
                drop(lock);

                debug!("Trigger matched: {}", expansion.trigger);

                let spinner_style_inner = spinner_style.read().map(|s| *s).unwrap_or_default();

                crate::hook::spawn_expansion_dispatch(expansion, spinner_style_inner, state);

                if is_action_key {
                    swallow_frame = true;
                }
            }
        }
    }

    if grab_enabled && !swallow_frame {
        // Linux swallowing happens by grabbing the real device and proxying every non-swallowed
        // frame back through uinput. Without the grab, the physical device already reaches the OS.
        crate::platform::linux::uinput::emit_batch(frame);
    }
}

fn clear_undo_state(state: &taurine_core::engine::EngineState) {
    state.clear_undo_state();
}

fn take_active_undo_state(state: &taurine_core::engine::EngineState) -> Option<(String, usize)> {
    state
        .take_active_undo_state()
        .map(|undo| (undo.trigger_string, undo.output_length))
}

fn spawn_undo_dispatch(trigger_string: String, output_length: usize) {
    if let Err(e) = thread::Builder::new()
        .name("tau-ev-undo".to_string())
        .spawn(move || injector::inject_undo(trigger_string, output_length))
    {
        error!("Failed to spawn evdev undo dispatch thread: {}", e);
    }
}

fn is_mouse_button(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::BTN_LEFT | KeyCode::BTN_RIGHT | KeyCode::BTN_MIDDLE
    )
}

fn is_modifier_key(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::KEY_LEFTSHIFT
            | KeyCode::KEY_RIGHTSHIFT
            | KeyCode::KEY_LEFTCTRL
            | KeyCode::KEY_RIGHTCTRL
            | KeyCode::KEY_LEFTALT
            | KeyCode::KEY_RIGHTALT
            | KeyCode::KEY_LEFTMETA
            | KeyCode::KEY_RIGHTMETA
    )
}

fn is_solo_modifier_press(
    key: KeyCode,
    shift_active: bool,
    ctrl_active: bool,
    alt_active: bool,
    meta_active: bool,
) -> bool {
    match key {
        KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => {
            !ctrl_active && !alt_active && !meta_active
        }
        KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => {
            !shift_active && !alt_active && !meta_active
        }
        KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => {
            !shift_active && !ctrl_active && !meta_active
        }
        KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => {
            !shift_active && !ctrl_active && !alt_active
        }
        _ => is_modifier_key(key) && !shift_active && !ctrl_active && !alt_active && !meta_active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::hotkey_evaluator::HotkeyEvaluator;
    use std::sync::RwLock;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use taurine_core::engine::{EngineState, Evaluator};

    #[test]
    fn process_frame_bypasses_hotkey_evaluation_when_is_injecting_is_true() {
        let state = Arc::new(EngineState::new());
        // Mock a basic hotkey to ensure it WOULD match if not bypassing
        state.load_hotkey_actions(vec![(
            "ctrl+shift+g".to_string(),
            taurine_core::db::crud::TriggerAction::text("test"),
        )]);

        let evaluator = Arc::new(Mutex::new(Evaluator::new(state.clone())));
        let paused = Arc::new(AtomicBool::new(false));
        let pause_notifications = Arc::new(AtomicBool::new(false));
        let pause_hotkey = Arc::new(RwLock::new(
            crate::input::hotkey::parse_pause_hotkey_setting("Alt + `").unwrap(),
        ));
        let spinner_style = Arc::new(RwLock::new(taurine_core::settings::SpinnerStyle::default()));
        let pause_audio = Arc::new(AtomicBool::new(false));
        let (audio_tx, _) = tokio::sync::mpsc::channel(1);
        let (pause_transition_tx, _) = tokio::sync::mpsc::channel(1);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _handle = rt.handle().clone();

        let mut xkb = crate::platform::linux::xkb::XkbMapper::default();
        let mut modifier_sides = ModifierSides::default();
        let mut hotkey_evaluator = HotkeyEvaluator::new();
        let mut swallow = false;
        let mut swallow_left = false;
        let mut swallow_right = false;

        let frame = vec![
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_LEFTCTRL.code(), 1),
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.code(), 1),
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_G.code(), 1),
        ];

        let _guard = crate::injector::InjectionFlagGuard::begin();

        // Process the frame while IS_INJECTING is true
        process_frame(
            &frame,
            true, // grab_enabled
            &evaluator,
            &state,
            &paused,
            &pause_notifications,
            &pause_hotkey,
            &spinner_style,
            &pause_audio,
            &audio_tx,
            &pause_transition_tx,
            &mut xkb,
            &mut modifier_sides,
            &mut hotkey_evaluator,
            &mut swallow,
            &mut swallow_left,
            &mut swallow_right,
        );

        // Verification 1: abort_injection bumped the generation
        assert!(
            crate::injector::capture_generation() > 0,
            "Physical key press during injection must bump injection generation"
        );
    }
}
