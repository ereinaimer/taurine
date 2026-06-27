use std::sync::Arc;
use taurine_core::engine::EngineState;

pub fn start_listener(_state: Arc<EngineState>) {
    std::thread::spawn(move || {
        // macOS fullscreen detection placeholder.
    });
}
