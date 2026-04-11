use evdev::{Device, EventType, KeyCode};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{debug, error, info, warn};

use super::xkb::XkbMapper;
use crate::hotkey::HotkeySpec;
use crate::injector::{self, IS_INJECTING};
use crate::notify;
use taurine_core::engine::{EngineEvent, Evaluator};

pub fn start_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    paused: Arc<AtomicBool>,
    pause_notifications_enabled: Arc<AtomicBool>,
    pause_hotkey: HotkeySpec,
) {
    let mut devices = vec![];

    if let Ok(entries) = fs::read_dir("/dev/input") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .starts_with("event")
            {
                if let Ok(device) = Device::open(&path) {
                    if let Some(keys) = device.supported_keys() {
                        if keys.contains(KeyCode::KEY_ENTER) && keys.contains(KeyCode::KEY_SPACE) {
                            devices.push(device);
                        }
                    }
                }
            }
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
        let pause_hotkey = pause_hotkey.clone();

        thread::spawn(move || {
            let mut xkb = XkbMapper::default();
            // In a passive Wayland model, we don't grab the device to avoid needing
            // a full pass-through uinput virtual keyboard for all unhandled typing.
            // This means we can't strictly "swallow" the pause hotkey, but it keeps
            // normal typing lag-free and safe.

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
                                        if !IS_INJECTING.load(Ordering::SeqCst) {
                                            let mut lock = evaluator.lock().unwrap();
                                            let _ = lock.process_event(EngineEvent::Interrupt);
                                        }
                                    }
                                    continue;
                                }

                                // Process the key through XKB to maintain the layout state and get the char.
                                let engine_event = xkb.process_key(key, is_press);

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
                                    continue;
                                }

                                if let Some(ev) = engine_event {
                                    let mut lock = evaluator.lock().unwrap();
                                    if let Some(expansion) = lock.process_event(ev) {
                                        drop(lock);

                                        debug!("Trigger matched! Expanding: {:?}", expansion);

                                        IS_INJECTING.store(true, Ordering::SeqCst);

                                        thread::spawn(move || {
                                            let trigger_clone = expansion.trigger.clone();
                                            let output_len = expansion.output.chars().count();
                                            let delete_count = expansion.delete_count;
                                            let left_arrow_count = expansion.left_arrow_count;

                                            injector::inject_payload(
                                                expansion.output,
                                                expansion.delete_count,
                                                expansion.left_arrow_count,
                                            );

                                            if expansion.is_calculation {
                                                taurine_core::db::crud::record_calculation_usage(
                                                    output_len,
                                                    delete_count,
                                                    left_arrow_count,
                                                );
                                            } else {
                                                taurine_core::db::crud::record_expansion_usage(
                                                    &trigger_clone,
                                                    output_len,
                                                    delete_count,
                                                    left_arrow_count,
                                                );
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Device {:?} disconnected or error: {}", device.name(), e);
                        break;
                    }
                }
            }
        });
    }
}
