//! Windows-only clipboard helpers: UTF-16 text paste plus cloud-clipboard exclusion flags.
//!
//! See [Cloud Clipboard and Clipboard History Formats](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats).

use std::mem;
use std::ptr;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, HANDLE, HGLOBAL};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

/// Standard `CF_UNICODETEXT` — full Unicode including emoji and non-Latin scripts.
const CF_UNICODETEXT: u32 = 13;

fn register_format(name: &str) -> Result<u32, String> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: RegisterClipboardFormatW takes a pointer to a null-terminated
    // UTF-16 string. `wide.as_ptr()` is the Vec's backing buffer, valid for
    // the call's duration. Returns a clipboard format identifier (nonzero)
    // or 0 on failure, checked below.
    let id = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
    if id == 0 {
        return Err(format!(
            "RegisterClipboardFormatW({name:?}) failed: {}",
            // SAFETY: GetLastError() retrieves the calling thread's last error
            // code. Always succeeds and has no failure mode.
            unsafe { GetLastError() }
        ));
    }
    Ok(id)
}

fn format_exclude_monitor() -> Result<u32, String> {
    static F: OnceLock<Result<u32, String>> = OnceLock::new();
    F.get_or_init(|| register_format("ExcludeClipboardContentFromMonitorProcessing"))
        .clone()
}

fn format_can_include_history() -> Result<u32, String> {
    static F: OnceLock<Result<u32, String>> = OnceLock::new();
    F.get_or_init(|| register_format("CanIncludeInClipboardHistory"))
        .clone()
}

fn format_can_upload_cloud() -> Result<u32, String> {
    static F: OnceLock<Result<u32, String>> = OnceLock::new();
    F.get_or_init(|| register_format("CanUploadToCloudClipboard"))
        .clone()
}

fn set_clipboard_dword(format: u32, value: u32) -> Result<(), String> {
    if format == 0 {
        return Err("clipboard format id is zero".to_string());
    }
    // SAFETY: GlobalAlloc allocates movable global memory. The returned HGLOBAL
    // is checked for null. GlobalLock returns a pointer to the locked memory
    // block (null on failure). ptr::write writes a u32 value into the locked
    // memory through a valid aligned pointer. GlobalUnlock unlocks the memory.
    // SetClipboardData takes ownership of the HGLOBAL on success; if it fails
    // we free the handle ourselves via GlobalFree to avoid leaks.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, mem::size_of::<u32>()) as HGLOBAL;
        if h.is_null() {
            return Err(format!("GlobalAlloc failed: {}", GetLastError()));
        }
        let p = GlobalLock(h);
        if p.is_null() {
            let _ = GlobalFree(h);
            return Err(format!("GlobalLock failed: {}", GetLastError()));
        }
        ptr::write(p as *mut u32, value);
        let _ = GlobalUnlock(h);

        if SetClipboardData(format, h as HANDLE).is_null() {
            let _ = GlobalFree(h);
            return Err(format!(
                "SetClipboardData(format {format}) failed: {}",
                GetLastError()
            ));
        }
    }
    Ok(())
}

fn set_clipboard_unicode_nul_terminated(text: &str) -> Result<(), String> {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = units.len() * mem::size_of::<u16>();
    // SAFETY: GlobalAlloc allocates movable global memory of `byte_len` bytes.
    // The HGLOBAL is checked for null. ptr::copy_nonoverlapping copies the UTF-16
    // data into the locked memory region. The source (`units.as_ptr()`) has at
    // least `units.len()` elements. SetClipboardData takes ownership on success;
    // on failure we free via GlobalFree.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, byte_len) as HGLOBAL;
        if h.is_null() {
            return Err(format!("GlobalAlloc (text) failed: {}", GetLastError()));
        }
        let p = GlobalLock(h);
        if p.is_null() {
            let _ = GlobalFree(h);
            return Err(format!("GlobalLock (text) failed: {}", GetLastError()));
        }
        ptr::copy_nonoverlapping(units.as_ptr(), p as *mut u16, units.len());
        let _ = GlobalUnlock(h);

        if SetClipboardData(CF_UNICODETEXT, h as HANDLE).is_null() {
            let _ = GlobalFree(h);
            return Err(format!(
                "SetClipboardData(CF_UNICODETEXT) failed: {}",
                GetLastError()
            ));
        }
    }
    Ok(())
}

/// Opens the clipboard with up to `max_retries` attempts, sleeping `retry_delay` between each.
/// This is the standard Win32 pattern for clipboard contention (ERROR_ACCESS_DENIED = 5).
fn open_clipboard_with_retry(max_retries: u32, retry_delay: Duration) -> Result<(), String> {
    for attempt in 0..=max_retries {
        // SAFETY: OpenClipboard with null HWND opens the clipboard for the
        // current thread. Returns 0 on failure (e.g., another app holds it open).
        // GetLastError retrieves the specific error code. Retry loop handles
        // the common ERROR_ACCESS_DENIED contention case.
        if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
            return Ok(());
        }
        // SAFETY: GetLastError has no failure mode; returns thread-local error.
        let err = unsafe { GetLastError() };
        if attempt < max_retries {
            thread::sleep(retry_delay);
        } else {
            return Err(format!("OpenClipboard failed: {}", err));
        }
    }
    unreachable!()
}

/// Reads `CF_UNICODETEXT` without taking ownership (matches arboard empty-on-missing).
pub fn get_unicode_text() -> Result<String, String> {
    open_clipboard_with_retry(5, Duration::from_millis(20))?;
    // SAFETY: Clipboard access within OpenClipboard/CloseClipboard bracket.
    // IsClipboardFormatAvailable checks for CF_UNICODETEXT presence without
    // locking. GetClipboardData returns a handle owned by the clipboard (do
    // not free). GlobalLock provides read-only access to the handle's memory.
    // The pointer iteration is bounded by a 16 MiB safety cap to prevent
    // unbounded reads. String::from_utf16_lossy handles replacement chars for
    // any invalid sequences. CloseClipboard releases the clipboard so other
    // apps can access it.
    unsafe {
        let result = (|| {
            if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
                return Ok(String::new());
            }
            let h = GetClipboardData(CF_UNICODETEXT) as HGLOBAL;
            if h.is_null() {
                return Ok(String::new());
            }
            let p = GlobalLock(h);
            if p.is_null() {
                return Ok(String::new());
            }
            let mut len = 0usize;
            let mut q = p as *const u16;
            while *q != 0 {
                len += 1;
                q = q.add(1);
                if len > 16 * 1024 * 1024 {
                    break;
                }
            }
            let slice = std::slice::from_raw_parts(p as *const u16, len);
            let s = String::from_utf16_lossy(slice);
            let _ = GlobalUnlock(h);
            Ok(s)
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Replaces clipboard contents with UTF-16 text and marks the operation so Windows clipboard
/// history / cloud clipboard should not record this clip (expansion payload or restore).
pub fn set_unicode_text_exclude_from_history(text: &str) -> Result<(), String> {
    let cf_exclude = format_exclude_monitor()?;
    let cf_history = format_can_include_history()?;
    let cf_cloud = format_can_upload_cloud()?;

    open_clipboard_with_retry(5, Duration::from_millis(20))?;
    // SAFETY: Clipboard write within OpenClipboard/CloseClipboard bracket.
    // EmptyClipboard() clears all clipboard data for this opener. Each
    // set_clipboard_dword call allocates GMEM_MOVEABLE memory via GlobalAlloc
    // and transfers ownership to the clipboard via SetClipboardData.
    // set_clipboard_unicode_nul_terminated writes the visible UTF-16 text
    // payload. Errors at any step leak out via the closure's Result. The
    // CloseClipboard call in the outer scope releases the clipboard.
    unsafe {
        let result = (|| {
            if EmptyClipboard() == 0 {
                return Err(format!("EmptyClipboard failed: {}", GetLastError()));
            }
            // ExcludeClipboardContentFromMonitorProcessing + DWORD flags (Delphi / Win32 samples).
            set_clipboard_dword(cf_exclude, 1)?;
            set_clipboard_dword(cf_history, 0)?;
            set_clipboard_dword(cf_cloud, 0)?;
            set_clipboard_unicode_nul_terminated(text)?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}
