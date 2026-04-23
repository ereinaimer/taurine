use std::thread;
use std::time::Duration;
use taurine_core::engine::variables::system::clipboard::{MAX_PAYLOAD_BYTES, clipboard_manager};

use crate::injector::IS_INJECTING;

const POLL_INTERVAL: Duration = Duration::from_millis(150);
const INIT_RETRY_INTERVAL: Duration = Duration::from_secs(1);

pub fn start_listener() {
    loop {
        if IS_INJECTING.load(std::sync::atomic::Ordering::Relaxed) {
            thread::sleep(POLL_INTERVAL);
            continue;
        }

        match try_read_clipboard_text_bounded() {
            Ok(Some(text)) => {
                // Deduplication still happens inside the manager under the write lock after the
                // payload passed the bounded size probe and text-only filter.
                let _ = clipboard_manager().record_text(text);
                thread::sleep(POLL_INTERVAL);
            }
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(error) => {
                tracing::warn!("Clipboard history listener unavailable: {}", error);
                thread::sleep(INIT_RETRY_INTERVAL);
            }
        }
    }
}

#[cfg(windows)]
fn try_read_clipboard_text_bounded() -> Result<Option<String>, String> {
    use std::ptr;

    use windows_sys::Win32::Foundation::{GetLastError, HGLOBAL};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;

    unsafe {
        if OpenClipboard(ptr::null_mut()) == 0 {
            return Err(format!("OpenClipboard failed: {}", GetLastError()));
        }

        let result = (|| {
            let handle = GetClipboardData(CF_UNICODETEXT) as HGLOBAL;

            // Missing CF_UNICODETEXT means the clipboard currently holds non-text payloads such as
            // images or file lists, so we ignore the event without allocating anything.
            if handle.is_null() || IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
                return Ok(None);
            }

            // Bound the read using clipboard metadata before locking or decoding the payload into a
            // Rust `String`, which prevents unbounded allocations on huge clipboard contents.
            let byte_len = GlobalSize(handle);
            if byte_len == 0 {
                return Ok(None);
            }
            if byte_len > MAX_PAYLOAD_BYTES {
                return Ok(None);
            }

            let ptr = GlobalLock(handle) as *const u16;
            if ptr.is_null() {
                return Ok(None);
            }

            let code_units = (byte_len / std::mem::size_of::<u16>()).saturating_sub(1);
            let slice = std::slice::from_raw_parts(ptr, code_units);
            let mut text = String::from_utf16_lossy(slice);
            let _ = GlobalUnlock(handle);

            if let Some(nul) = text.find('\0') {
                text.truncate(nul);
            }

            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(text))
            }
        })();

        let _ = CloseClipboard();
        result
    }
}

#[cfg(target_os = "linux")]
fn try_read_clipboard_text_bounded() -> Result<Option<String>, String> {
    static WARNED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

    let _ = WARNED.get_or_init(|| {
        tracing::warn!(
            "Clipboard history is disabled on Linux until a bounded clipboard read path is wired; refusing to call arboard::get_text() because it allocates before the size cap."
        );
    });

    Ok(None)
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn try_read_clipboard_text_bounded() -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "linux")]
    fn linux_bounded_reader_is_fail_closed() {
        assert_eq!(super::try_read_clipboard_text_bounded().unwrap(), None);
    }
}
