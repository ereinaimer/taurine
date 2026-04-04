use rdev::{Event, EventType, Key};
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{debug, error};

use crate::injector;
use taurine_core::engine::{EngineEvent, Evaluator};

pub fn start_listener(evaluator: Arc<Mutex<Evaluator>>) {
    let callback = move |event: Event| {
        match event.event_type {
            EventType::ButtonPress(_) => {
                // If the user clicks elsewhere, cancel whatever they were typing!
                let mut lock = evaluator.lock().unwrap();
                let _ = lock.process_event(EngineEvent::Interrupt);
            }
            EventType::KeyPress(key) => {
                let engine_event = match key {
                    Key::Escape => Some(EngineEvent::Interrupt),
                    Key::Backspace => Some(EngineEvent::Backspace),
                    Key::Space => Some(EngineEvent::Char(' ')),
                    Key::Return => Some(EngineEvent::Interrupt), // Let's count enter as an interrupt/break
                    _ => {
                        // Extract typed character intelligently based on layout
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
                        drop(lock); // VERY IMPORTANT: Unlock fast before making slow OS injection

                        debug!("Trigger matched! Processing extraction... {:?}", expansion);

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
