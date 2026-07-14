use arboard::Clipboard;
use std::thread;
use std::time::Duration;
use tracing::error;

use crate::platform::ClipboardManager;

impl ClipboardManager for Clipboard {
    fn get_text(&mut self) -> Result<String, String> {
        Ok(self.get_text().unwrap_or_default())
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        Clipboard::set_text(self, text).map_err(|e| e.to_string())
    }

    fn set_image(&mut self, bytes: &[u8], _mime_type: &str) -> Result<(), String> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| format!("Failed to decode image for arboard: {}", e))?;
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        let img_data = arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: std::borrow::Cow::Borrowed(&rgba),
        };
        self.set_image(img_data).map_err(|e| e.to_string())
    }
}

/// Reads the user's current clipboard, writes `payload`, waits, then verifies the clipboard
/// still equals `payload`. Returns the original text for restore after paste.
///
/// If verification fails, the caller must not simulate paste (avoids injecting stale clipboard).
pub(super) fn prepare_clipboard_for_expansion(
    clipboard: &mut impl ClipboardManager,
    payload: &str,
) -> Result<String, String> {
    let original = clipboard.get_text()?;
    clipboard.set_text(payload)?;

    // Same delay as production: OS listeners may not see the write immediately.
    thread::sleep(Duration::from_millis(25));

    match clipboard.get_text() {
        Ok(ref actual) if actual == payload => Ok(original),
        Ok(actual) => Err(format!(
            "clipboard verify failed: expected {:?}, got {:?}",
            payload, actual
        )),
        Err(e) => Err(e),
    }
}

/// Restores the user's original clipboard content.
pub(super) fn restore_clipboard(original: &str) {
    match crate::platform::get_clipboard_manager() {
        Ok(mut clip) => {
            if let Err(e) = clip.set_text(original) {
                error!("Failed to restore clipboard: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to get clipboard manager: {}", e);
        }
    }
}

pub fn restore_clipboard_text(original: &str) {
    restore_clipboard(original);
}
