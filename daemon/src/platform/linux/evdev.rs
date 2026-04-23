use evdev::{Device, EventType, InputEvent, KeyCode};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use tokio::runtime::Handle;
use tracing::{debug, error, info, warn};

use super::xkb::XkbMapper;
use crate::hotkey::HotkeySpec;
use crate::injector::{self, IS_INJECTING};
use crate::notify;
use taurine_core::engine::{EngineEvent, EngineMode, Evaluator};

pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    paused: Arc<AtomicBool>,
    pause_notifications_enabled: Arc<AtomicBool>,
    pause_hotkey: Arc<RwLock<HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    runtime_handle: Handle,
) {
    let mut devices = vec![];
    let input_dir = "/dev/input";

    match fs::read_dir(input_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();

                if file_name.starts_with("event") {
                    match Device::open(&path) {
                        Ok(device) => {
                            let name = device.name().unwrap_or("Unknown Device");
                            if name == crate::platform::linux::VIRTUAL_DEVICE_NAME {
                                debug!("Ignoring Taurine virtual keyboard: {:?}", path);
                                continue;
                            }

                            if let Some(keys) = device.supported_keys() {
                                // Broadened keyboard detection: check for basic alphanumeric support.
                                // Most physical keyboards will have ENTER, SPACE, and KeyA.
                                if keys.contains(KeyCode::KEY_ENTER)
                                    && keys.contains(KeyCode::KEY_SPACE)
                                    && keys.contains(KeyCode::KEY_A)
                                {
                                    debug!(
                                        "Found potential keyboard device: {} ({:?})",
                                        name, path
                                    );
                                    devices.push(device);
                                }
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                            warn!(
                                "Permission denied opening {:?}. You may need to add your user to the 'input' group.",
                                path
                            );
                        }
                        Err(e) => {
                            debug!("Failed to open device {:?}: {}", path, e);
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!(
                "Failed to read {} directory: {}. Hook listener cannot start.",
                input_dir, e
            );
        }
    }

    if devices.is_empty() {
        error!("Fatal OS global hook crash: No evdev keyboard devices found.");
        return;
    }

    info!("Found {} evdev keyboard device(s)", devices.len());

    for mut device in devices {
        let evaluator = evaluator.clone();
        let paused = paused.clone();
        let pause_notifications_enabled = pause_notifications_enabled.clone();
        let _pause_hotkey = pause_hotkey.clone();
        let spinner_style = spinner_style.clone();
        let runtime_handle = runtime_handle.clone();

        thread::spawn(move || {
            let mut xkb = XkbMapper::default();
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

            loop {
                match device.fetch_events() {
                    Ok(events) => {
                        let mut frame = Vec::new();
                        for event in events {
                            if event.event_type() == EventType::SYNCHRONIZATION {
                                process_frame(
                                    &frame,
                                    grab_enabled,
                                    &evaluator,
                                    &paused,
                                    &pause_notifications_enabled,
                                    &spinner_style,
                                    &runtime_handle,
                                    &mut xkb,
                                    &mut swallow_next_backspace_release,
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
                                &evaluator,
                                &paused,
                                &pause_notifications_enabled,
                                &spinner_style,
                                &runtime_handle,
                                &mut xkb,
                                &mut swallow_next_backspace_release,
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Device {:?} disconnected or error: {}", device_name, e);
                        break;
                    }
                }
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn process_frame(
    frame: &[InputEvent],
    grab_enabled: bool,
    evaluator: &Arc<Mutex<Evaluator>>,
    paused: &Arc<AtomicBool>,
    pause_notifications_enabled: &Arc<AtomicBool>,
    spinner_style: &Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    runtime_handle: &Handle,
    xkb: &mut XkbMapper,
    swallow_next_backspace_release: &mut bool,
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

        if *swallow_next_backspace_release && is_release && key == KeyCode::KEY_BACKSPACE {
            swallow_frame = true;
            *swallow_next_backspace_release = false;
            continue;
        }

        if is_mouse_button(key) {
            if is_press {
                clear_undo_state(evaluator);

                if IS_INJECTING.load(Ordering::SeqCst) {
                    injector::abort_injection();
                } else {
                    let mut lock = evaluator.lock().unwrap();
                    let _ = lock.process_event(EngineEvent::Interrupt);
                }
            }
            continue;
        }

        let engine_mode = evaluator
            .lock()
            .map(|lock| lock.state.engine_mode())
            .unwrap_or(EngineMode::Normal);
        let engine_event = xkb.process_key(key, is_press, engine_mode);

        if is_press && xkb.is_alt_down() && key == KeyCode::KEY_GRAVE {
            clear_undo_state(evaluator);
            let now_paused = !paused.load(Ordering::Relaxed);
            paused.store(now_paused, Ordering::Relaxed);
            if pause_notifications_enabled.load(Ordering::Relaxed) {
                notify::notify_pause_toggled(now_paused);
            }
            continue;
        }

        if is_press {
            let shift_active = xkb.is_shift_down();
            let ctrl_active = xkb.is_ctrl_down();
            let alt_active = xkb.is_alt_down();
            let meta_active = xkb.is_meta_down();

            if key == KeyCode::KEY_BACKSPACE {
                if ctrl_active || alt_active || meta_active {
                    clear_undo_state(evaluator);
                } else if let Some((trigger_string, output_length)) =
                    take_active_undo_state(evaluator)
                {
                    // Windows/macOS can swallow inside the hook callback itself. Linux needs
                    // EVIOCGRAB plus a uinput proxy, so we drop the grabbed frame instead.
                    IS_INJECTING.store(true, Ordering::SeqCst);
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
                clear_undo_state(evaluator);
            }
        }

        if paused.load(Ordering::Relaxed) {
            continue;
        }

        if IS_INJECTING.load(Ordering::SeqCst) {
            if is_press {
                debug!("Physical keypress detected during injection. Aborting.");
                injector::abort_injection();
            }
            continue;
        }

        if let Some(ev) = engine_event {
            let mut lock = evaluator.lock().unwrap();
            if let Some(expansion) = lock.process_event(ev) {
                let state = lock.state.clone();
                drop(lock);

                debug!("Trigger matched! Expanding: {:?}", expansion);

                IS_INJECTING.store(true, Ordering::SeqCst);

                let spinner_style_inner = spinner_style.read().map(|s| *s).unwrap_or_default();

                crate::hook::spawn_expansion_dispatch(
                    expansion,
                    spinner_style_inner,
                    runtime_handle.clone(),
                    state,
                );
            }
        }
    }

    if grab_enabled && !swallow_frame {
        // Linux swallowing happens by grabbing the real device and proxying every non-swallowed
        // frame back through uinput. Without the grab, the physical device already reaches the OS.
        crate::platform::linux::uinput::emit_batch(frame);
    }
}

fn clear_undo_state(evaluator: &Arc<Mutex<Evaluator>>) {
    if let Ok(lock) = evaluator.lock() {
        lock.state.clear_undo_state();
    }
}

fn take_active_undo_state(evaluator: &Arc<Mutex<Evaluator>>) -> Option<(String, usize)> {
    evaluator.lock().ok().and_then(|lock| {
        lock.state
            .take_active_undo_state()
            .map(|undo| (undo.trigger_string, undo.output_length))
    })
}

fn spawn_undo_dispatch(trigger_string: String, output_length: usize) {
    thread::spawn(move || injector::inject_undo(trigger_string, output_length));
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
