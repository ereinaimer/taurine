use rdev::{Event, EventType, Key};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{debug, error};

use crate::injector::{self, IS_INJECTING};
use taurine_core::engine::{EngineEvent, Evaluator};

pub fn start_listener(evaluator: Arc<Mutex<Evaluator>>) {
    let callback = move |event: Event| {
        match event.event_type {
            EventType::ButtonPress(_) => {
                // Mouse click — ignore if we're mid-injection; otherwise clear the buffer.
                if IS_INJECTING.load(Ordering::SeqCst) {
                    return;
                }
                let mut lock = evaluator.lock().unwrap();
                let _ = lock.process_event(EngineEvent::Interrupt);
            }
            EventType::KeyPress(key) => {
                // All events during injection are synthetic (our own backspaces / Ctrl+V).
                // Ignore them so they don't feed back into the evaluator.
                if IS_INJECTING.load(Ordering::SeqCst) {
                    return;
                }

                let engine_event = match key {
                    Key::Escape => Some(EngineEvent::Interrupt),
                    Key::Backspace => Some(EngineEvent::Backspace),
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
                            injector::inject_payload(expansion.output, expansion.delete_count);
                            taurine_core::db::crud::record_expansion_usage(&trigger_clone);
                        });
                    }
                }
            }
            _ => {}
        }
    };

    if let Err(e) = rdev::listen(callback) {
        error!("Fatal OS global hook crash: {:?}", e);
    }
}
