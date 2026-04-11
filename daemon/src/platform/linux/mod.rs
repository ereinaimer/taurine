use crate::platform::ClipboardManager;
use arboard::Clipboard;
use std::sync::{Mutex, OnceLock};

pub mod evdev;
pub mod security;
pub mod uinput;
pub mod xkb;

static CLIPBOARD: OnceLock<Mutex<Clipboard>> = OnceLock::new();

pub fn init() -> Result<(), String> {
    uinput::init_uinput()?;
    // For now, immediately drop privileges
    security::drop_privileges()
}

pub struct LinuxClipboard;

impl ClipboardManager for LinuxClipboard {
    fn get_text(&mut self) -> Result<String, String> {
        let mut clip = CLIPBOARD
            .get_or_init(|| Mutex::new(Clipboard::new().expect("Failed to open clipboard")))
            .lock()
            .map_err(|e| format!("Clipboard mutex poisoned: {}", e))?;
        clip.get_text().map_err(|e| e.to_string())
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        let mut clip = CLIPBOARD
            .get_or_init(|| Mutex::new(Clipboard::new().expect("Failed to open clipboard")))
            .lock()
            .map_err(|e| format!("Clipboard mutex poisoned: {}", e))?;
        clip.set_text(text.to_owned()).map_err(|e| e.to_string())
    }
}
