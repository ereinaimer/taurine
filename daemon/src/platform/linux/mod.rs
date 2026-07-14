use self::xkb::XkbMapper;
use crate::platform::ClipboardManager;
use ::evdev::KeyCode;
use arboard::Clipboard;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub mod evdev;
pub mod fullscreen;
pub mod injector;
pub mod input_supervisor;
pub mod uinput;
pub mod xkb;

static CLIPBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);
static REVERSE_LOOKUP: OnceLock<HashMap<char, (KeyCode, bool)>> = OnceLock::new();

pub const VIRTUAL_DEVICE_NAME: &str = "Taurine Virtual Keyboard";

pub fn init() -> Result<(), String> {
    uinput::init_uinput()?;
    let mapper = XkbMapper::new().map_err(|e| format!("XKB init failed: {}", e))?;
    REVERSE_LOOKUP
        .set(mapper.get_reverse_map().clone())
        .map_err(|_| "REVERSE_LOOKUP already initialized".to_string())?;
    Ok(())
}

pub fn get_reverse_lookup() -> Option<&'static HashMap<char, (KeyCode, bool)>> {
    REVERSE_LOOKUP.get()
}

/// Safely runs a closure with the shared Linux clipboard connection.
/// Initializes the connection on-demand, returning a Result instead of panicking on failure.
/// Resets the connection cache if the closure returns an Err, allowing future retries.
pub fn with_clipboard<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut Clipboard) -> Result<R, String>,
{
    let mut lock = CLIPBOARD
        .lock()
        .map_err(|e| format!("Clipboard mutex poisoned: {}", e))?;

    if lock.is_none() {
        let clipboard = Clipboard::new().map_err(|e| format!("Failed to open clipboard: {}", e))?;
        *lock = Some(clipboard);
    }

    let clip = lock.as_mut().unwrap();
    let result = f(clip);
    if result.is_err() {
        *lock = None;
    }
    result
}

pub struct LinuxClipboard;

impl ClipboardManager for LinuxClipboard {
    fn get_text(&mut self) -> Result<String, String> {
        with_clipboard(|clip| clip.get_text().map_err(|e| e.to_string()))
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        with_clipboard(|clip| clip.set_text(text.to_owned()).map_err(|e| e.to_string()))
    }

    fn set_image(&mut self, bytes: &[u8], _mime_type: &str) -> Result<(), String> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| format!("Failed to decode image for Linux clipboard: {}", e))?;
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        with_clipboard(|clip| {
            let img_data = arboard::ImageData {
                width: width as usize,
                height: height as usize,
                bytes: std::borrow::Cow::Borrowed(&rgba),
            };
            clip.set_image(img_data).map_err(|e| e.to_string())
        })
    }
}
