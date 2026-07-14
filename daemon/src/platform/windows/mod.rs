pub mod active_window;
pub mod clipboard;
pub mod fullscreen;
pub mod power;

pub struct WindowsClipboard;

impl crate::platform::ClipboardManager for WindowsClipboard {
    fn get_text(&mut self) -> Result<String, String> {
        clipboard::get_unicode_text()
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        clipboard::set_unicode_text_exclude_from_history(text)
    }

    fn set_image(&mut self, bytes: &[u8], mime_type: &str) -> Result<(), String> {
        clipboard::set_image_bytes_exclude_from_history(bytes, mime_type)
    }
}
