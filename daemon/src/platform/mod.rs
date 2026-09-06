// Platform-specific traits and abstractions

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

pub mod executor;
pub mod spinner_renderer;

#[cfg(not(target_os = "linux"))]
pub mod rdev_injector;

pub use taurine_core::keys::MouseButton;

pub trait ClipboardManager {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
    fn set_image_file(&mut self, path: &std::path::Path) -> Result<(), String>;
    fn set_html(&mut self, html: &str, plaintext: &str) -> Result<(), String>;
}

pub trait Injector {
    fn simulate_mouse_click(&self, button: MouseButton);
    fn simulate_mouse_dblclick(&self, button: MouseButton);
    fn simulate_mouse_move(&self, x: u16, y: u16);
    fn simulate_mouse_scroll(&self, delta: i32);
    fn simulate_mouse_hold(&self, button: MouseButton, hold: bool);
    fn simulate_key_alias(&self, alias: &str) -> bool;
    fn simulate_left(&self, count: usize);
    fn simulate_right(&self, count: usize);
    fn simulate_backspace(&self, count: usize);
    fn simulate_paste(&self);
    fn pre_release_modifiers(&self);
    fn try_inject_frame_raw(&self, frame: &str) -> bool;

    fn inject_atomic_text_expansion(&self, delete_count: usize, text: &str) -> bool {
        self.inject_atomic_text_expansion_with_nav(delete_count, text, 0, 0)
    }
    fn inject_atomic_text_expansion_with_nav(
        &self,
        delete_count: usize,
        text: &str,
        left_nav: usize,
        right_nav: usize,
    ) -> bool;
    fn inject_atomic_backspaces(&self, count: usize);
    fn inject_unicode_text_direct(&self, text: &str) -> bool;
    fn inject_atomic_undo(&self, backspaces: usize, text: &str) -> bool;
}

#[allow(clippy::needless_return)]
pub fn get_clipboard_manager() -> Result<impl ClipboardManager, String> {
    #[cfg(windows)]
    {
        return Ok(windows::WindowsClipboard);
    }
    #[cfg(target_os = "linux")]
    {
        return Ok(linux::LinuxClipboard);
    }
    #[cfg(target_os = "macos")]
    {
        return Ok(macos::clipboard::MacosClipboard);
    }
    #[cfg(all(not(windows), not(target_os = "linux"), not(target_os = "macos")))]
    {
        return arboard::Clipboard::new().map_err(|e| e.to_string());
    }
}

pub fn get_injector() -> &'static dyn Injector {
    #[cfg(target_os = "linux")]
    {
        &linux::injector::LinuxInjector
    }
    #[cfg(not(target_os = "linux"))]
    {
        &rdev_injector::RdevInjector
    }
}

#[cfg(windows)]
pub fn get_mouse_pos() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: GetCursorPos writes the cursor coordinates into a stack-allocated
    // POINT struct. It accepts any valid pointer to a POINT and returns 0 on
    // failure (no error info needed). The POINT is fully initialized after a
    // successful call.
    unsafe {
        if GetCursorPos(&mut point) != 0 {
            Some((point.x, point.y))
        } else {
            None
        }
    }
}

#[cfg(target_os = "macos")]
pub fn get_mouse_pos() -> Option<(i32, i32)> {
    use objc2::{class, msg_send};
    use objc2_app_kit::NSEvent;
    let point = NSEvent::mouseLocation();

    // Attempt dynamic retrieval of main screen height to convert bottom-left to top-left.
    let screen_height: f64 = unsafe {
        // SAFETY: Objective-C messaging via msg_send! requires unsafe because the
        // compiler cannot verify the selector exists or returns the correct type.
        // [NSScreen mainScreen] is a documented class method that returns the
        // primary screen (or nil if no screens are connected). The returned pointer
        // is checked for null before dereference. The `frame` selector on NSScreen
        // returns an NSRect describing the screen's dimensions in the global
        // coordinate system. All types match the framework declarations in
        // objc2_foundation.
        let nsscreen: *mut objc2::runtime::AnyObject = msg_send![class!(NSScreen), mainScreen];
        if !nsscreen.is_null() {
            let frame: objc2_foundation::NSRect = msg_send![nsscreen, frame];
            frame.size.height
        } else {
            0.0
        }
    };

    if screen_height > 0.0 {
        Some((point.x as i32, (screen_height - point.y) as i32))
    } else {
        Some((point.x as i32, point.y as i32))
    }
}

#[cfg(target_os = "linux")]
pub fn get_mouse_pos() -> Option<(i32, i32)> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;
    let (conn, _) = x11rb::connect(None).ok()?;
    let screen = &conn.setup().roots[0];
    let reply = conn.query_pointer(screen.root).ok()?.reply().ok()?;
    Some((reply.root_x as i32, reply.root_y as i32))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn get_mouse_pos() -> Option<(i32, i32)> {
    None
}

static ACTIVE_WINDOW_INFO_CACHE: std::sync::Mutex<
    Option<(
        std::time::Instant,
        Option<taurine_core::engine::ActiveWindowInfo>,
    )>,
> = std::sync::Mutex::new(None);

const ACTIVE_WINDOW_CACHE_TTL: std::time::Duration = std::time::Duration::from_millis(50);

pub fn get_active_window_info() -> Option<taurine_core::engine::ActiveWindowInfo> {
    if let Ok(guard) = ACTIVE_WINDOW_INFO_CACHE.lock()
        && let Some((cached_at, ref info)) = *guard
        && cached_at.elapsed() < ACTIVE_WINDOW_CACHE_TTL
    {
        return info.clone();
    }

    let resolved = get_active_window_info_uncached();

    if let Ok(mut guard) = ACTIVE_WINDOW_INFO_CACHE.lock() {
        *guard = Some((std::time::Instant::now(), resolved.clone()));
    }

    resolved
}

pub fn get_active_window_label() -> Option<String> {
    let info = get_active_window_info()?;
    serde_json::to_string(&info).ok()
}

#[cfg(windows)]
fn get_active_window_info_uncached() -> Option<taurine_core::engine::ActiveWindowInfo> {
    windows::active_window::get_active_window_info()
}

#[cfg(target_os = "linux")]
fn get_active_window_info_uncached() -> Option<taurine_core::engine::ActiveWindowInfo> {
    let s = linux::toplevel::get_active_window_label()?;
    serde_json::from_str(&s).ok().or_else(|| {
        Some(taurine_core::engine::ActiveWindowInfo {
            exec_name: Some(s),
            ..Default::default()
        })
    })
}

#[cfg(target_os = "macos")]
fn get_active_window_info_uncached() -> Option<taurine_core::engine::ActiveWindowInfo> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let frontmost_app = workspace.frontmostApplication()?;

    let localized_name = frontmost_app.localizedName().map(|s| s.to_string());
    let bundle_id = frontmost_app.bundleIdentifier().map(|s| s.to_string());

    Some(taurine_core::engine::ActiveWindowInfo {
        title: None,
        class: bundle_id,
        exec_name: localized_name,
        exec_path: None,
    })
}

#[cfg(all(not(windows), not(target_os = "linux"), not(target_os = "macos")))]
fn get_active_window_info_uncached() -> Option<taurine_core::engine::ActiveWindowInfo> {
    None
}

pub fn read_clipboard_text() -> Result<String, String> {
    let mut clip = get_clipboard_manager()?;
    clip.get_text()
}

pub fn capture_active_app() -> Option<String> {
    let json = get_active_window_label()?;
    let info: taurine_core::engine::ActiveWindowInfo = serde_json::from_str(&json).ok()?;
    let exec_name = info.exec_name?;
    let key = exec_name.trim().to_lowercase();
    if key.is_empty() { None } else { Some(key) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_clipboard_text_returns_string() {
        let _ = read_clipboard_text();
    }

    #[test]
    fn test_get_active_window_label_caching() {
        let first = get_active_window_label();
        let second = get_active_window_label();
        assert_eq!(first, second);
    }
}
