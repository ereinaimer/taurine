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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

pub trait ClipboardManager {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
    fn set_image(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), String>;
}

pub trait InputHook {
    // Placeholder for Phase 2/3
}

pub trait Injector {
    fn simulate_mouse_click(&self, button: MouseButton);
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

#[cfg(windows)]
pub fn get_active_window_label() -> Option<String> {
    windows::active_window::get_active_window_label()
}

#[cfg(target_os = "linux")]
pub fn get_active_window_label() -> Option<String> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return None;
    }
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let net_active_window = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let active_window = conn
        .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()
        .and_then(|mut iter| iter.next())?;

    if active_window == 0 {
        return None;
    }

    // 1. Get class name (from WM_CLASS)
    let wm_class = conn
        .intern_atom(false, b"WM_CLASS")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let class_reply = conn
        .get_property(false, active_window, wm_class, AtomEnum::STRING, 0, 1024)
        .ok()?
        .reply()
        .ok()?;

    let class_name = if !class_reply.value.is_empty() {
        // WM_CLASS contains null-separated strings: instance and class.
        // We split by null and take the second part (class name) if available,
        // otherwise the first part.
        let parts: Vec<&str> = std::str::from_utf8(&class_reply.value)
            .ok()?
            .split('\0')
            .filter(|s| !s.is_empty())
            .collect();
        parts
            .get(1)
            .or_else(|| parts.first())
            .map(|s| s.to_string())
    } else {
        None
    };

    // 2. Get exec name / process name if possible
    // (Note: on X11 there's no standard property for binary path, but WM_CLASS class name is often the exec name)
    let exec_name = class_name.clone();

    // 3. Get window title (first try _NET_WM_NAME, fallback to WM_NAME)
    let net_wm_name = conn
        .intern_atom(false, b"_NET_WM_NAME")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let utf8_string = conn
        .intern_atom(false, b"UTF8_STRING")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let mut title_reply = conn
        .get_property(false, active_window, net_wm_name, utf8_string, 0, 1024)
        .ok()?
        .reply()
        .ok()?;

    if title_reply.value.is_empty() {
        let wm_name = conn.intern_atom(false, b"WM_NAME").ok()?.reply().ok()?.atom;
        title_reply = conn
            .get_property(false, active_window, wm_name, AtomEnum::STRING, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
    }

    let title = if !title_reply.value.is_empty() {
        std::str::from_utf8(&title_reply.value)
            .ok()
            .map(|s| s.to_string())
    } else {
        None
    };

    let info = taurine_core::engine::ActiveWindowInfo {
        title,
        class: class_name,
        exec_name,
        exec_path: None,
    };

    serde_json::to_string(&info).ok()
}

#[cfg(target_os = "macos")]
pub fn get_active_window_label() -> Option<String> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let frontmost_app = workspace.frontmostApplication()?;

    let localized_name = frontmost_app.localizedName().map(|s| s.to_string());
    let bundle_id = frontmost_app.bundleIdentifier().map(|s| s.to_string());

    let info = taurine_core::engine::ActiveWindowInfo {
        title: None,
        class: bundle_id,
        exec_name: localized_name,
        exec_path: None,
    };

    serde_json::to_string(&info).ok()
}

#[cfg(all(not(windows), not(target_os = "linux"), not(target_os = "macos")))]
pub fn get_active_window_label() -> Option<String> {
    None
}
