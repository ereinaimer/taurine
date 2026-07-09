#![allow(unused_imports)]

mod clipboard;
mod gate;
mod inject;
mod simulate;

pub use gate::{
    INJECTION_ABORT, IS_INJECTING, IS_SIMULATING, InjectionFlagGuard, InjectionVisibilityGuard,
    abort_injection, spawn_guarded_injection_thread,
};

#[cfg(not(target_os = "linux"))]
pub use simulate::{consume_simulated_event, simulate_monitored};

pub use clipboard::restore_clipboard_text;

pub use inject::{
    InjectionReport, StreamingTextSession, TextSegmentInjection, inject_expansion,
    inject_text_segment, inject_undo,
};

#[cfg(test)]
mod tests;
