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
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

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
    Err("OpenClipboard failed after exhausting retries".to_string())
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
            let byte_len = GlobalSize(h);
            if byte_len == 0 || byte_len > MAX_PAYLOAD_BYTES {
                return Ok(String::new());
            }
            let p = GlobalLock(h);
            if p.is_null() {
                return Ok(String::new());
            }
            let max_code_units = byte_len / std::mem::size_of::<u16>();
            let slice = std::slice::from_raw_parts(p as *const u16, max_code_units);
            let mut s = String::from_utf16_lossy(slice);
            let _ = GlobalUnlock(h);
            if let Some(nul) = s.find('\0') {
                s.truncate(nul);
            }
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

fn format_html() -> Result<u32, String> {
    static F: OnceLock<Result<u32, String>> = OnceLock::new();
    F.get_or_init(|| register_format("HTML Format")).clone()
}

fn format_windows_html(html: &str) -> String {
    let mut fragment = String::new();
    fragment.push_str("<!--StartFragment-->\n");
    fragment.push_str(html);
    fragment.push_str("\n<!--EndFragment-->");

    let mut body = String::new();
    body.push_str("<!DOCTYPE html>\n<html>\n<body>\n");
    body.push_str(&fragment);
    body.push_str("\n</body>\n</html>");

    let header_len = 105;
    let start_html = header_len;
    let end_html = start_html + body.len();
    let start_fragment = start_html + body.find("<!--StartFragment-->").unwrap_or_default();
    let end_fragment = start_html
        + body.find("<!--EndFragment-->").unwrap_or_default()
        + "<!--EndFragment-->".len();

    format!(
        "Version:0.9\r\n\
         StartHTML:{:010}\r\n\
         EndHTML:{:010}\r\n\
         StartFragment:{:010}\r\n\
         EndFragment:{:010}\r\n\
         {}",
        start_html, end_html, start_fragment, end_fragment, body
    )
}

/// Replaces clipboard contents with HTML and fallback plaintext, excluding the operation from history.
pub fn set_html_exclude_from_history(html: &str, plaintext: &str) -> Result<(), String> {
    let cf_exclude = format_exclude_monitor()?;
    let cf_history = format_can_include_history()?;
    let cf_cloud = format_can_upload_cloud()?;
    let cf_html = format_html()?;

    let windows_html = format_windows_html(html);

    open_clipboard_with_retry(5, Duration::from_millis(20))?;
    // SAFETY: Write within OpenClipboard/CloseClipboard bracket.
    // EmptyClipboard clears all formats. Payload setting allocations are managed by
    // the system when transferred via SetClipboardData. CloseClipboard releases ownership.
    unsafe {
        let result = (|| {
            if EmptyClipboard() == 0 {
                return Err(format!("EmptyClipboard failed: {}", GetLastError()));
            }
            set_clipboard_dword(cf_exclude, 1)?;
            set_clipboard_dword(cf_history, 0)?;
            set_clipboard_dword(cf_cloud, 0)?;
            set_clipboard_unicode_nul_terminated(plaintext)?;
            set_clipboard_bytes(cf_html, windows_html.as_bytes())?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

fn format_png() -> Result<u32, String> {
    static F: OnceLock<Result<u32, String>> = OnceLock::new();
    F.get_or_init(|| register_format("PNG")).clone()
}

fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
    if format == 0 {
        return Err("clipboard format id is zero".to_string());
    }
    // SAFETY: GlobalAlloc allocates GMEM_MOVEABLE memory of the exact byte length.
    // The HGLOBAL is checked for null. ptr::copy_nonoverlapping copies the raw bytes
    // into the locked memory region. SetClipboardData transfers ownership to clipboard.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) as HGLOBAL;
        if h.is_null() {
            return Err(format!("GlobalAlloc (bytes) failed: {}", GetLastError()));
        }
        let p = GlobalLock(h);
        if p.is_null() {
            let _ = GlobalFree(h);
            return Err(format!("GlobalLock (bytes) failed: {}", GetLastError()));
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
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

#[repr(C, packed)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_xpels_per_meter: i32,
    bi_ypels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

fn create_dib_payload(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let header = BitmapInfoHeader {
        bi_size: 40,
        bi_width: width as i32,
        bi_height: -(height as i32), // Negative height = top-down DIB (matches RGBA layout)
        bi_planes: 1,
        bi_bit_count: 32,
        bi_compression: 0, // BI_RGB (uncompressed)
        bi_size_image: width * height * 4,
        bi_xpels_per_meter: 0,
        bi_ypels_per_meter: 0,
        bi_clr_used: 0,
        bi_clr_important: 0,
    };

    let mut payload = Vec::with_capacity(40 + (width * height * 4) as usize);
    // SAFETY: Transmuting BitmapInfoHeader to bytes is safe as it is repr(C, packed)
    // and contains only plain-old-data integers.
    let header_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            &header as *const BitmapInfoHeader as *const u8,
            std::mem::size_of::<BitmapInfoHeader>(),
        )
    };
    payload.extend_from_slice(header_bytes);

    // Swap red and blue channels (RGBA -> BGRA) for Windows DIB format
    for chunk in rgba.as_chunks::<4>().0 {
        payload.push(chunk[2]); // B
        payload.push(chunk[1]); // G
        payload.push(chunk[0]); // R
        payload.push(chunk[3]); // A
    }

    payload
}

pub fn set_image_file_exclude_from_history(path: &std::path::Path) -> Result<(), String> {
    let cf_exclude = format_exclude_monitor()?;
    let cf_history = format_can_include_history()?;
    let cf_cloud = format_can_upload_cloud()?;

    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read image file: {}", e))?;
    let path_str = path.to_string_lossy();

    // Check if it's a PNG
    let is_png = path_str.ends_with(".png");

    // Decode to RGBA first, so we have the fallback DIB format
    let img =
        image::load_from_memory(&bytes).map_err(|e| format!("Failed to decode image: {}", e))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    // Create DIB payload (standard device-independent bitmap)
    let dib_payload = create_dib_payload(&rgba, width, height);
    const CF_DIB: u32 = 8;
    const CF_HDROP: u32 = 15;

    // Create DROPFILES payload for CF_HDROP
    let mut path_buffer = Vec::new();
    let wide: Vec<u16> = path_str.encode_utf16().collect();
    path_buffer.extend_from_slice(&wide);
    path_buffer.push(0); // single null
    path_buffer.push(0); // double null

    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::Shell::DROPFILES;

    let dropfiles_size = std::mem::size_of::<DROPFILES>();
    let hdrop_len = dropfiles_size + (path_buffer.len() * std::mem::size_of::<u16>());

    let mut hdrop_payload = Vec::with_capacity(hdrop_len);
    let dropfiles = DROPFILES {
        pFiles: dropfiles_size as u32,
        pt: POINT { x: 0, y: 0 },
        fNC: 0,
        fWide: 1, // UTF-16
    };
    let dropfiles_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(&dropfiles as *const DROPFILES as *const u8, dropfiles_size)
    };
    hdrop_payload.extend_from_slice(dropfiles_bytes);

    // Append wide path bytes
    let path_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            path_buffer.as_ptr() as *const u8,
            path_buffer.len() * std::mem::size_of::<u16>(),
        )
    };
    hdrop_payload.extend_from_slice(path_bytes);

    open_clipboard_with_retry(5, Duration::from_millis(20))?;
    // SAFETY: Clipboard write within OpenClipboard/CloseClipboard bracket.
    // EmptyClipboard() clears clipboard. Exclude flags, CF_HDROP, raw PNG bytes,
    // and standard CF_DIB bytes are written. CloseClipboard releases ownership.
    unsafe {
        let result = (|| {
            if EmptyClipboard() == 0 {
                return Err(format!("EmptyClipboard failed: {}", GetLastError()));
            }
            set_clipboard_dword(cf_exclude, 1)?;
            set_clipboard_dword(cf_history, 0)?;
            set_clipboard_dword(cf_cloud, 0)?;

            // 1. Write CF_HDROP (file path drop)
            set_clipboard_bytes(CF_HDROP, &hdrop_payload)?;

            // 2. If it's a PNG, write the high-fidelity PNG format
            if is_png {
                let cf_png = format_png()?;
                set_clipboard_bytes(cf_png, &bytes)?;
            }

            // 3. Write standard CF_DIB format
            set_clipboard_bytes(CF_DIB, &dib_payload)?;

            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}
