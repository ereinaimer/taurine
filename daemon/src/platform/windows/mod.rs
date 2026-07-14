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

    fn set_image(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
        let mut clip = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let img_data = arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: std::borrow::Cow::Borrowed(rgba),
        };
        clip.set_image(img_data).map_err(|e| e.to_string())
    }
}
