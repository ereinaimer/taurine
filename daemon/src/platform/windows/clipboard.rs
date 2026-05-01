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
    let id = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
    if id == 0 {
        return Err(format!(
            "RegisterClipboardFormatW({name:?}) failed: {}",
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
        if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
            return Ok(());
        }
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
