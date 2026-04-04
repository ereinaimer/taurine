use rdev::{Event, EventType, Key};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{debug, error};

use crate::injector::{self, IS_INJECTING};
use taurine_core::engine::{EngineEvent, Evaluator};

pub fn start_listener(evaluator: Arc<Mutex<Evaluator>>) {
    let callback = move |event: Event| {
        // Drop all synthetic events we ourselves generate to avoid feedback loops.
        if IS_INJECTING.load(Ordering::SeqCst) {
            return;
        }

        match event.event_type {
            EventType::ButtonPress(_) => {
                // Mouse click → cancel any in-progress sequence.
                let mut lock = evaluator.lock().unwrap();
                let _ = lock.process_event(EngineEvent::Interrupt);
            }
            EventType::KeyPress(key) => {
                let engine_event = match key {
                    Key::Escape => Some(EngineEvent::Interrupt),
                    Key::Backspace => Some(EngineEvent::Backspace),
                    Key::Space => Some(EngineEvent::Char(' ')),
                    // Enter is also treated as a hard interrupt — it submits a form
                    // or moves to a new line, which breaks any trigger sequence.
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
                        // Release the lock immediately — injection is slow and we
                        // must not block the global OS hook thread.
                        drop(lock);

                        debug!("Trigger matched! Expanding: {:?}", expansion);

                        thread::spawn(move || {
                            injector::inject_payload(expansion.payload, expansion.delete_count);
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
