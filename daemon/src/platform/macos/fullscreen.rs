use nite_core::engine::EngineState;
use std::sync::Arc;

pub fn start_listener(_state: Arc<EngineState>) {
    let _ = std::thread::Builder::new()
        .name("nite-mac-full".to_string())
        .spawn(move || {
            // macOS fullscreen detection placeholder.
        });
}
