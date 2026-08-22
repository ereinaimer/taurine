mod clipboard;
mod gate;
mod inject;
mod simulate;

pub use gate::{
    IS_INJECTING, InjectionFlagGuard, InjectionVisibilityGuard, abort_injection,
    capture_generation, init_injection_pool, is_aborted, spawn_guarded_injection_thread,
};

#[cfg(not(target_os = "linux"))]
pub use simulate::{consume_simulated_event, simulate_monitored};

#[cfg(test)]
#[cfg(not(target_os = "linux"))]
pub use simulate::{clear_simulated_events_for_test, enqueue_simulated_event_for_test};

pub use clipboard::restore_clipboard_text;

pub use inject::{
    InjectionReport, StreamingTextSession, inject_expansion, inject_text_segment, inject_undo,
};

#[cfg(test)]
mod tests;
