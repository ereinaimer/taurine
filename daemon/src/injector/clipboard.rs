use arboard::Clipboard;
use std::thread;
use std::time::Duration;
use tracing::error;

use crate::platform::ClipboardManager;

use super::gate::is_aborted;

/// RAII guard that requests 1ms multimedia timer resolution on Windows.
///
/// Ensures high precision for `thread::sleep` intervals (down to 1ms instead of ~15.6ms default).
/// When dropped, restores the previous OS timer resolution.
pub struct HighResolutionTimerGuard {
    #[cfg(windows)]
    _private: (),
}

impl HighResolutionTimerGuard {
    #[inline]
    pub fn acquire() -> Self {
        #[cfg(windows)]
        {
            // SAFETY: timeBeginPeriod requests a minimum timer resolution of 1 millisecond for
            // multimedia and high-precision sleep operations. It is paired with timeEndPeriod(1)
            // in Drop.
            unsafe {
                windows_sys::Win32::Media::timeBeginPeriod(1);
            }
            Self { _private: () }
        }
        #[cfg(not(windows))]
        {
            Self {}
        }
    }
}

#[cfg(windows)]
impl Drop for HighResolutionTimerGuard {
    fn drop(&mut self) {
        // SAFETY: timeEndPeriod clears the 1 millisecond timer resolution request made by
        // timeBeginPeriod(1) during acquisition.
        unsafe {
            windows_sys::Win32::Media::timeEndPeriod(1);
        }
    }
}

/// Returns the current Windows clipboard sequence number, or 0 on non-Windows platforms.
#[allow(dead_code)]
#[inline]
pub fn get_clipboard_sequence_number() -> u32 {
    #[cfg(windows)]
    {
        // SAFETY: GetClipboardSequenceNumber retrieves the current sequence number of the clipboard.
        // It has no side-effects and is thread-safe.
        unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() }
    }
    #[cfg(not(windows))]
    {
        0
    }
}

impl ClipboardManager for Clipboard {
    fn get_text(&mut self) -> Result<String, String> {
        Ok(self.get_text().unwrap_or_default())
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        Clipboard::set_text(self, text).map_err(|e| e.to_string())
    }

    fn set_image_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        let bytes = std::fs::read(path)
            .map_err(|e| format!("Failed to read image file for arboard: {}", e))?;
        let img = image::load_from_memory(&bytes)
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

    fn set_html(&mut self, _html: &str, plaintext: &str) -> Result<(), String> {
        // arboard doesn't support HTML, fallback to plaintext
        ClipboardManager::set_text(self, plaintext)
    }
}

/// Reads the user's current clipboard, writes `payload`, waits, then verifies the clipboard
/// still equals `payload`. Returns the original text for restore after paste.
///
/// If verification fails, the caller must not simulate paste (avoids injecting stale clipboard).
///
/// When `captured_gen` is non-zero, the polling loop also checks whether the injection
/// generation has advanced (another task was aborted) so the clipboard cycle can bail
/// early and release the injection mutex.
pub(super) fn prepare_clipboard_for_expansion(
    clipboard: &mut impl ClipboardManager,
    payload: &str,
    captured_gen: u64,
) -> Result<String, String> {
    let _timer_guard = HighResolutionTimerGuard::acquire();
    let original = clipboard.get_text()?;

    let is_html = taurine_core::utils::html::has_html_tags(payload);
    let expected = if is_html {
        let plaintext = taurine_core::utils::html::strip_html(payload);
        clipboard.set_html(payload, &plaintext)?;
        plaintext
    } else {
        clipboard.set_text(payload)?;
        payload.to_string()
    };

    // Poll clipboard to ensure the OS has registered the write.
    // Micro-yield for the first 3 attempts (sub-millisecond turnaround),
    // then fall back to 1ms sleeps for up to 25 attempts.
    let mut actual = String::new();
    let mut success = false;

    for attempt in 0..25 {
        if attempt > 0 {
            if attempt <= 3 {
                std::hint::spin_loop();
                std::thread::yield_now();
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }

        if captured_gen != 0 && is_aborted(captured_gen) {
            return Err("injection aborted during clipboard poll".to_string());
        }

        match clipboard.get_text() {
            Ok(ref text) if text == &expected => {
                success = true;
                break;
            }
            Ok(text) => actual = text,
            Err(_) => {}
        }
    }

    if success {
        Ok(original)
    } else {
        Err(format!(
            "clipboard verify failed after polling: expected {:?}, got {:?}",
            expected, actual
        ))
    }
}

/// Restores the user's original clipboard content.
///
/// When `captured_gen` is non-zero, the verification polling loop bails
/// early if the injection generation advances.
pub(super) fn restore_clipboard(original: &str, captured_gen: u64) {
    let _timer_guard = HighResolutionTimerGuard::acquire();
    match crate::platform::get_clipboard_manager() {
        Ok(mut clip) => {
            if let Err(e) = clip.set_text(original) {
                error!("Failed to restore clipboard: {}", e);
                return;
            }
            // Poll to verify clipboard was restored correctly.
            // Micro-yield for the first 3 attempts, then fall back to 1ms sleeps.
            let mut actual = String::new();
            let mut success = false;

            for attempt in 0..25 {
                if attempt > 0 {
                    if attempt <= 3 {
                        std::hint::spin_loop();
                        std::thread::yield_now();
                    } else {
                        thread::sleep(Duration::from_millis(1));
                    }
                }

                if captured_gen != 0 && is_aborted(captured_gen) {
                    return;
                }

                match clip.get_text() {
                    Ok(ref text) if text == original => {
                        success = true;
                        break;
                    }
                    Ok(text) => actual = text,
                    Err(_) => {}
                }
            }

            if !success {
                error!(
                    "Clipboard restore verify failed after polling: expected {:?}, got {:?}",
                    original, actual
                );
            }
        }
        Err(e) => {
            error!("Failed to get clipboard manager: {}", e);
        }
    }
}

pub fn restore_clipboard_text(original: &str) {
    restore_clipboard(original, 0);
}
