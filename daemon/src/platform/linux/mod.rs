use self::xkb::XkbMapper;
use crate::platform::ClipboardManager;
use ::evdev::KeyCode;
use arboard::Clipboard;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub mod evdev;
pub mod input_supervisor;
pub mod security;
pub mod uinput;
pub mod xkb;

static CLIPBOARD: OnceLock<Mutex<Clipboard>> = OnceLock::new();
static REVERSE_LOOKUP: OnceLock<HashMap<char, (KeyCode, bool)>> = OnceLock::new();

pub const VIRTUAL_DEVICE_NAME: &str = "Taurine Virtual Keyboard";

pub fn init() -> Result<(), String> {
    uinput::init_uinput()?;
    let mapper = XkbMapper::new().map_err(|e| format!("XKB init failed: {}", e))?;
    REVERSE_LOOKUP
        .set(mapper.get_reverse_map().clone())
        .map_err(|_| "REVERSE_LOOKUP already initialized".to_string())?;
    // For now, immediately drop privileges
    security::drop_privileges()
}

pub fn get_reverse_lookup() -> Option<&'static HashMap<char, (KeyCode, bool)>> {
    REVERSE_LOOKUP.get()
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
