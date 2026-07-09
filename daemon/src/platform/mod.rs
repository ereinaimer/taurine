// Platform-specific traits and abstractions

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

pub mod executor;
pub mod spinner_renderer;

pub trait ClipboardManager {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
}

pub trait InputHook {
    // Placeholder for Phase 2/3
}

pub trait Injector {
    // Placeholder for Phase 2/4
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
