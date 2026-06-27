// Platform-specific traits and abstractions

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

pub mod executor;
pub mod spinner_renderer;

pub trait ClipboardManager {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
}

pub trait InputHook {
    // Placeholder for Phase 2/3
}

pub trait Injector {
    // Placeholder for Phase 2/4
}
