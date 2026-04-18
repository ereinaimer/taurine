use evdev::{Device, EventType, KeyCode};
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
            // In a passive Wayland model, we don't grab the device to avoid needing
            // a full pass-through uinput virtual keyboard for all unhandled typing.
            // This means we can't strictly "swallow" the pause hotkey, but it keeps
            // normal typing lag-free and safe.

            let device_name = device.name().map(|s| s.to_string());
            loop {
                match device.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            // We only care about key presses and releases.
                            // event.value() == 0 is release, 1 is press, 2 is autorepeat
                            let value = event.value();
                            if value == 2 {
                                continue;
                            }
                            let is_press = value == 1;

                            if event.event_type() == EventType::KEY {
                                let key = KeyCode::new(event.code());

                                // Mouse buttons are sometimes emitted by combo devices.
                                if key == KeyCode::BTN_LEFT
                                    || key == KeyCode::BTN_RIGHT
                                    || key == KeyCode::BTN_MIDDLE
                                {
                                    if is_press {
                                        if IS_INJECTING.load(Ordering::SeqCst) {
                                            // Mouse click — always physical. Abort active injection.
                                            injector::abort_injection();
                                        } else {
                                            let mut lock = evaluator.lock().unwrap();
                                            let _ = lock.process_event(EngineEvent::Interrupt);
                                        }
                                    }
                                    continue;
                                }

                                // Process the key through XKB to maintain the layout state and get the char.
                                let engine_mode = evaluator
                                    .lock()
                                    .map(|lock| lock.state.engine_mode())
                                    .unwrap_or(EngineMode::Normal);
                                let engine_event = xkb.process_key(key, is_press, engine_mode);

                                // Check pause hotkey logic
                                // Since we don't have rdev::Event, we check our xkb and the specific key.
                                // It's a simplification, we assume the pause hotkey is something like Alt+`
                                // Pause chord logic matches the string name (e.g., BackQuote).
                                // For now, we manually check if alt is down and it's the grave key.
                                if is_press && xkb.is_alt_down() && key == KeyCode::KEY_GRAVE {
                                    let now_paused = !paused.load(Ordering::Relaxed);
                                    paused.store(now_paused, Ordering::Relaxed);
                                    if pause_notifications_enabled.load(Ordering::Relaxed) {
                                        notify::notify_pause_toggled(now_paused);
                                    }
                                    continue; // Passive listener, so we don't block it, but we handle the state.
                                }

                                if paused.load(Ordering::Relaxed) {
                                    continue;
                                }

                                if IS_INJECTING.load(Ordering::SeqCst) {
                                    // Physical keypress during injection. Abort to prevent corruption
                                    if is_press {
                                        debug!(
                                            "Physical keypress detected during injection. Aborting."
                                        );
                                        injector::abort_injection();
                                    }
                                    continue;
                                }

                                if let Some(ev) = engine_event {
                                    let mut lock = evaluator.lock().unwrap();
                                    if let Some(expansion) = lock.process_event(ev) {
                                        drop(lock);

                                        debug!("Trigger matched! Expanding: {:?}", expansion);

                                        IS_INJECTING.store(true, Ordering::SeqCst);

                                        let spinner_style_inner =
                                            spinner_style.read().map(|s| *s).unwrap_or_default();

                                        crate::hook::spawn_expansion_dispatch(
                                            expansion,
                                            spinner_style_inner,
                                            runtime_handle.clone(),
                                        );
                                    }
                                }
                            }
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
