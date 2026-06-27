#[cfg(not(windows))]
use std::thread;
#[cfg(not(windows))]
use std::time::{Duration, Instant};
#[cfg(windows)]
use taurine_core::engine::variables::system::clipboard::MAX_PAYLOAD_BYTES;
use taurine_core::engine::variables::system::clipboard::clipboard_manager;

use crate::injector::IS_INJECTING;

#[cfg(not(windows))]
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(50);
#[cfg(not(windows))]
const POLL_INTERVAL: Duration = Duration::from_millis(150);
#[cfg(not(windows))]
const INIT_RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(not(windows))]
pub fn start_listener() {
    let mut last_event = Instant::now() - DEBOUNCE_WINDOW;

    loop {
        if IS_INJECTING.load(std::sync::atomic::Ordering::Relaxed) {
            thread::sleep(POLL_INTERVAL);
            continue;
        }

        let now = Instant::now();
        if now.duration_since(last_event) < DEBOUNCE_WINDOW {
            thread::sleep(POLL_INTERVAL);
            continue;
        }
        last_event = now;

        match try_read_clipboard_text_bounded() {
            Ok(Some(text)) => {
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
pub fn start_listener() {
    use std::ptr::null;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::DataExchange::{
        AddClipboardFormatListener, RemoveClipboardFormatListener,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
        RegisterClassW, TranslateMessage, WM_DESTROY, WNDCLASSW,
    };

    const WM_CLIPBOARDUPDATE: u32 = 0x031D;
    static LAST_EVENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_CLIPBOARDUPDATE {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let last = LAST_EVENT.load(std::sync::atomic::Ordering::Relaxed);

            // 50ms debounce
            if now.saturating_sub(last) < 50 {
                return 0;
            }

            if !IS_INJECTING.load(std::sync::atomic::Ordering::Relaxed) {
                let mut attempt = 0;
                while attempt < 10 {
                    match try_read_clipboard_text_bounded() {
                        Ok(Some(text)) => {
                            let _ = clipboard_manager().record_text(text);
                            LAST_EVENT.store(now, std::sync::atomic::Ordering::Relaxed);
                            break;
                        }
                        Ok(None) => break,
                        Err(_err) => {
                            attempt += 1;
                            std::thread::sleep(std::time::Duration::from_millis(15));
                        }
                    }
                }
            }
            return 0;
        } else if message == WM_DESTROY {
            return 0;
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    let class_name: Vec<u16> = "TaurineClipboardMonitor"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let instance = unsafe { GetModuleHandleW(null()) };

    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        tracing::error!("Failed to register clipboard monitor window class");
        return;
    }

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };

    if hwnd.is_null() {
        tracing::error!("Failed to create clipboard monitor window");
        return;
    }

    if unsafe { AddClipboardFormatListener(hwnd) } == 0 {
        tracing::error!("Failed to register clipboard format listener");
        return;
    }

    tracing::info!("Windows clipboard monitor is listening for WM_CLIPBOARDUPDATE");

    let mut message = MSG::default();
    loop {
        let status = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if status > 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        } else if status == 0 {
            break;
        } else {
            tracing::error!("Windows clipboard monitor message loop failed");
            break;
        }
    }

    unsafe {
        let _ = RemoveClipboardFormatListener(hwnd);
    }
}

#[cfg(windows)]
fn try_read_clipboard_text_bounded() -> Result<Option<String>, String> {
    use std::ptr;

    use windows_sys::Win32::Foundation::{GetLastError, HGLOBAL};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;
    static PASSWORD_HINT_FORMAT: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

    fn register_optional_format(name: &str) -> u32 {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
    }

    unsafe {
        if OpenClipboard(ptr::null_mut()) == 0 {
            let err = GetLastError();
            return Err(format!("OpenClipboard failed with error {}", err));
        }

        let result = (|| {
            let password_hint_format = *PASSWORD_HINT_FORMAT
                .get_or_init(|| register_optional_format("Clipboard Viewer Ignore"));

            // Password managers mark transient/sensitive clipboard payloads with this format.
            // Drop those events before we even look at the text payload.
            if password_hint_format != 0 && IsClipboardFormatAvailable(password_hint_format) != 0 {
                return Ok(None);
            }

            // Missing CF_UNICODETEXT means the clipboard currently holds non-text payloads such as
            // images or file lists, so we ignore the event without allocating anything.
            if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
                return Ok(None);
            }

            let handle = GetClipboardData(CF_UNICODETEXT) as HGLOBAL;
            if handle.is_null() {
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

            // Drop junk payloads made only of whitespace after the bounded allocation succeeds.
            if text.trim().is_empty() {
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
