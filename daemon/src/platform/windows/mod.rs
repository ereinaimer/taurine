pub mod active_window;
pub mod clipboard;
pub mod fullscreen;
pub mod pipe_security;
pub mod power;
pub mod veh;

pub use active_window::{
    is_foreground_window_elevated_or_restricted, wait_for_foreground_window_idle,
};

/// Unique signature stamped in `KEYBDINPUT.dwExtraInfo` and `MOUSEINPUT.dwExtraInfo`
/// so Nite's low-level hooks can identify and ignore its own simulated inputs.
#[cfg(target_pointer_width = "64")]
pub const NITE_EXTRA_INFO_MAGIC: usize = 0x4E49_5445_0000_0001; // NITE\0\0\0\x01
#[cfg(target_pointer_width = "32")]
pub const NITE_EXTRA_INFO_MAGIC: usize = 0x4E49_5401; // NIT\x01

pub struct WindowsClipboard;

impl crate::platform::ClipboardManager for WindowsClipboard {
    fn get_text(&mut self) -> Result<String, String> {
        clipboard::get_unicode_text()
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        clipboard::set_unicode_text_exclude_from_history(text)
    }

    fn set_image_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        clipboard::set_image_file_exclude_from_history(path)
    }

    fn set_html(&mut self, html: &str, plaintext: &str) -> Result<(), String> {
        clipboard::set_html_exclude_from_history(html, plaintext)
    }
}
