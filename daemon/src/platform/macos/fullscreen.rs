use std::sync::Arc;
use taurine_core::engine::EngineState;

pub fn start_listener(_state: Arc<EngineState>) {
    let _ = std::thread::Builder::new()
        .name("tau-mac-full".to_string())
        .spawn(move || {
            // macOS fullscreen detection placeholder.
        });
}
