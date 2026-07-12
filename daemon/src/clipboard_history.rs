#[cfg(not(windows))]
use std::thread;
#[cfg(not(windows))]
use std::time::{Duration, Instant};
use taurine_core::engine::variables::system::clip::MAX_PAYLOAD_BYTES;
use taurine_core::engine::variables::system::clip::clip_manager;

use crate::injector::IS_INJECTING;
use std::sync::atomic::AtomicBool;

pub static CLIPBOARD_SHOULD_SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
static CLIPBOARD_THREAD_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(not(windows))]
const INIT_RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(not(windows))]
pub fn start_listener() {
    #[cfg(target_os = "macos")]
    use objc2_app_kit::NSPasteboard;

    #[cfg(target_os = "macos")]
    const POLL_INTERVAL: Duration = Duration::from_millis(200);
    #[cfg(not(target_os = "macos"))]
    const POLL_INTERVAL: Duration = Duration::from_millis(350);

    let mut clip_opt: Option<arboard::Clipboard> = None;

    #[cfg(target_os = "macos")]
    // SAFETY: generalPasteboard is thread-safe and always returns a valid pasteboard instance.
    let mut last_change_count = unsafe { NSPasteboard::generalPasteboard().changeCount() };

    CLIPBOARD_SHOULD_SHUTDOWN.store(false, std::sync::atomic::Ordering::Relaxed);

    loop {
        if CLIPBOARD_SHOULD_SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        if !taurine_core::settings::get_cached_clipboard_history_enabled() {
            thread::sleep(POLL_INTERVAL);
            continue;
        }

        if IS_INJECTING.load(std::sync::atomic::Ordering::Relaxed) {
            thread::sleep(POLL_INTERVAL);
            continue;
        }

        #[cfg(target_os = "macos")]
        {
            // SAFETY: generalPasteboard is thread-safe and always returns a valid pasteboard instance.
            let current = unsafe { NSPasteboard::generalPasteboard().changeCount() };
            if current == last_change_count {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
            last_change_count = current;
        }

        let clip = match &mut clip_opt {
            Some(c) => c,
            None => match arboard::Clipboard::new() {
                Ok(c) => {
                    clip_opt = Some(c);
                    clip_opt.as_mut().unwrap()
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to initialize clipboard listener: {}. Retrying...",
                        e
                    );
                    thread::sleep(INIT_RETRY_INTERVAL);
                    continue;
                }
            },
        };

        match try_read_clipboard_text_bounded(clip) {
            Ok(Some(text)) => {
                let _ = clip_manager().record_text(text);
                thread::sleep(POLL_INTERVAL);
            }
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(error) => {
                tracing::warn!(
                    "Clipboard history listener error: {}. Resetting connection.",
                    error
                );
                clip_opt = None;
                thread::sleep(INIT_RETRY_INTERVAL);
            }
        }
    }
}

#[cfg(windows)]
pub fn start_listener() {
    // SAFETY: GetCurrentThreadId retrieves the OS thread ID of the calling thread.
    // It always succeeds and has no failure modes.
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    CLIPBOARD_THREAD_ID.store(tid, std::sync::atomic::Ordering::Relaxed);
    CLIPBOARD_SHOULD_SHUTDOWN.store(false, std::sync::atomic::Ordering::Relaxed);

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

    // SAFETY: Win32 window procedure called by the OS on the thread's message queue.
    // `hwnd` is a valid window handle created by CreateWindowExW below. The function
    // pointer is registered via WNDCLASSW.lpfnWndProc and must have the `system` ABI.
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

            if !taurine_core::settings::get_cached_clipboard_history_enabled() {
                return 0;
            }

            if !IS_INJECTING.load(std::sync::atomic::Ordering::Relaxed) {
                let mut attempt = 0;
                while attempt < 10 {
                    match try_read_clipboard_text_bounded() {
                        Ok(Some(text)) => {
                            let _ = clip_manager().record_text(text);
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
        // SAFETY: DefWindowProcW forwards unhandled messages to the default window
        // procedure. `hwnd` is valid (created below) and `message`/`wparam`/`lparam`
        // are the parameters passed by the OS to this window procedure.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    let class_name: Vec<u16> = "TaurineClipboardMonitor"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: GetModuleHandleW(null()) returns the handle of the calling process's
    // executable (.exe) module. Passing null is documented and always succeeds for
    // the current process. The returned pseudo-handle is valid for the lifetime of
    // the process and must not be freed.
    let instance = unsafe { GetModuleHandleW(null()) };

    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };

    // SAFETY: RegisterClassW registers a window class for this process.
    // `window_class` is a fully initialized WNDCLASSW on the stack with valid
    // `lpfnWndProc` and `lpszClassName` pointers. The pointer to `window_class`
    // must remain valid for the call duration (it is a stack-local). Returns 0
    // on failure, which we check below.
    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        tracing::error!("Failed to register clipboard monitor window class");
        return;
    }

    // SAFETY: CreateWindowExW creates a message-only window (HWND_MESSAGE) that
    // receives clipboard notifications. `class_name.as_ptr()` points to a valid
    // null-terminated UTF-16 string registered above via RegisterClassW.
    // `instance` is the valid module handle from GetModuleHandleW. The returned
    // HWND is checked for null (failure) before use.
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

    // SAFETY: AddClipboardFormatListener registers `hwnd` to receive
    // WM_CLIPBOARDUPDATE messages. `hwnd` is a valid message-only window created
    // above. Returns 0 on failure, which we check.
    if unsafe { AddClipboardFormatListener(hwnd) } == 0 {
        tracing::error!("Failed to register clipboard format listener");
        return;
    }

    tracing::info!("Windows clipboard monitor is listening for WM_CLIPBOARDUPDATE");

    let mut message = MSG::default();
    loop {
        if CLIPBOARD_SHOULD_SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        // SAFETY: GetMessageW retrieves a message from the calling thread's message
        // queue. `&mut message` is a valid pointer to a MSG struct on the stack.
        // Passing null for hwnd means all messages for this thread are retrieved.
        // Returns -1 on error, 0 on WM_QUIT, >0 otherwise.
        let status = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if status > 0 {
            // SAFETY: TranslateMessage converts virtual-key messages to character
            // messages; it is safe on any valid MSG. DispatchMessageW sends the
            // message to the window procedure registered for the target HWND in
            // `message`. `&message` is a valid stack pointer. Both accept any
            // initialized MSG.
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

    // SAFETY: RemoveClipboardFormatListener unregisters `hwnd` from clipboard
    // update notifications. `hwnd` is the valid window created above. The call
    // is safe even during shutdown because the window still exists at this point.
    unsafe {
        let _ = RemoveClipboardFormatListener(hwnd);
    }
}

pub fn stop_listener() {
    CLIPBOARD_SHOULD_SHUTDOWN.store(true, std::sync::atomic::Ordering::Relaxed);
    #[cfg(windows)]
    {
        let tid = CLIPBOARD_THREAD_ID.load(std::sync::atomic::Ordering::Relaxed);
        if tid != 0 {
            use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
            // SAFETY: PostThreadMessageW is safe to call with a valid thread ID and message.
            unsafe {
                for _ in 0..100 {
                    if PostThreadMessageW(tid, WM_QUIT, 0, 0) != 0 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
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
        RegisterClipboardFormatW,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;
    static PASSWORD_HINT_FORMAT: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

    fn register_optional_format(name: &str) -> u32 {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: RegisterClipboardFormatW takes a pointer to a null-terminated
        // UTF-16 string. `wide.as_ptr()` points to the Vec's backing buffer which
        // is valid and lives for the duration of the call. The returned clipboard
        // format identifier is valid until the process exits. Returns 0 on failure.
        unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
    }

    // SAFETY: This entire block operates on the Win32 clipboard API. OpenClipboard
    // with a null HWND opens the clipboard for the current process. GetClipboardData
    // returns a handle to clipboard data owned by the system — we must not free it.
    // GlobalLock/GlobalUnlock provide read-only access to the HGLOBAL memory. The
    // pointer from GlobalLock is valid while the clipboard is open and the handle
    // is not freed. We bound reads by GlobalSize and cap at MAX_PAYLOAD_BYTES to
    // prevent unbounded allocation.
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

#[cfg(not(windows))]
fn try_read_clipboard_text_bounded(
    clip: &mut arboard::Clipboard,
) -> Result<Option<String>, String> {
    match clip.get_text() {
        Ok(text) => {
            if text.len() > MAX_PAYLOAD_BYTES {
                Ok(None)
            } else {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(text))
                }
            }
        }
        Err(arboard::Error::ContentNotReady) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "linux")]
    fn linux_bounded_reader_handles_init_failure() {
        let clip = arboard::Clipboard::new();
        match clip {
            Ok(mut c) => {
                let _ = super::try_read_clipboard_text_bounded(&mut c);
            }
            Err(_) => {
                // Expected on headless environments without a display server
            }
        }
    }
}
