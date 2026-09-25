// Platform-specific traits and abstractions

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

pub mod circuit_breaker;
pub mod executor;
pub mod panic;
pub mod spinner_renderer;

#[cfg(not(target_os = "linux"))]
pub mod rdev_injector;

pub use taurine_core::keys::MouseButton;

/// True only when the user explicitly opts into host-affecting tests.
/// Default test runs must never touch keyboard, clipboard, windows,
/// audio, mic, or spawned processes.
pub fn host_tests_allowed() -> bool {
    matches!(
        std::env::var("TAURINE_ALLOW_HOST_INPUT").as_deref(),
        Ok("1") | Ok("true")
    )
}

pub trait ClipboardManager {
    fn get_text(&mut self) -> Result<String, String>;
    fn set_text(&mut self, text: &str) -> Result<(), String>;
    fn set_image_file(&mut self, path: &std::path::Path) -> Result<(), String>;
    fn set_html(&mut self, html: &str, plaintext: &str) -> Result<(), String>;
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

/// In-memory clipboard used only in tests so the default suite never
/// reads or overwrites the host clipboard.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct FakeClipboard {
    text: std::sync::Mutex<String>,
    ops: std::sync::Mutex<Vec<&'static str>>,
}

#[cfg(test)]
impl FakeClipboard {
    fn record_op(&self, op: &'static str) {
        if let Ok(mut ops) = self.ops.lock() {
            ops.push(op);
        }
    }

    /// Snapshot of recorded ops for assertions.
    #[allow(dead_code)]
    pub fn ops_snapshot(&self) -> Vec<&'static str> {
        self.ops.lock().map(|ops| ops.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
impl ClipboardManager for FakeClipboard {
    fn get_text(&mut self) -> Result<String, String> {
        self.record_op("get_text");
        Ok(self.text.lock().map(|t| t.clone()).unwrap_or_default())
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.record_op("set_text");
        if let Ok(mut guard) = self.text.lock() {
            *guard = text.to_string();
        }
        Ok(())
    }

    fn set_image_file(&mut self, _path: &std::path::Path) -> Result<(), String> {
        self.record_op("set_image");
        Ok(())
    }

    fn set_html(&mut self, _html: &str, plaintext: &str) -> Result<(), String> {
        self.record_op("set_html");
        if let Ok(mut guard) = self.text.lock() {
            *guard = plaintext.to_string();
        }
        Ok(())
    }
}

/// Recording injector used only in tests so the default suite never calls
/// `SendInput` / `rdev::simulate` / `uinput` on the host.
#[cfg(test)]
#[derive(Debug)]
pub struct RecordingInjector {
    calls: std::sync::Mutex<Vec<String>>,
}

#[cfg(test)]
impl Default for RecordingInjector {
    fn default() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl RecordingInjector {
    fn record(&self, call: String) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(call);
        }
    }

    /// Snapshot of recorded calls for assertions.
    #[allow(dead_code)]
    pub fn recorded(&self) -> Vec<String> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Clear recorded calls between assertions.
    #[allow(dead_code)]
    pub fn clear(&self) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.clear();
        }
    }
}

#[cfg(test)]
impl Injector for RecordingInjector {
    fn simulate_mouse_click(&self, button: MouseButton) {
        self.record(format!("mouse_click:{button:?}"));
    }

    fn simulate_mouse_move(&self, x: u16, y: u16) {
        self.record(format!("mouse_move:{x},{y}"));
    }

    fn simulate_mouse_scroll(&self, delta: i32) {
        self.record(format!("mouse_scroll:{delta}"));
    }

    fn simulate_mouse_hold(&self, button: MouseButton, hold: bool) {
        self.record(format!("mouse_hold:{button:?},{hold}"));
    }

    fn simulate_key_alias(&self, alias: &str) -> bool {
        self.record(format!("key_alias:{alias}"));
        true
    }

    fn simulate_left(&self, count: usize) {
        self.record(format!("left:{count}"));
    }

    fn simulate_right(&self, count: usize) {
        self.record(format!("right:{count}"));
    }

    fn simulate_backspace(&self, count: usize) {
        self.record(format!("backspace:{count}"));
    }

    fn simulate_paste(&self) {
        self.record("paste".to_string());
    }

    fn pre_release_modifiers(&self) {
        self.record("pre_release_modifiers".to_string());
    }

    fn try_inject_frame_raw(&self, frame: &str) -> bool {
        self.record(format!("frame:{frame}"));
        false
    }

    fn inject_atomic_text_expansion_with_nav(
        &self,
        delete_count: usize,
        text: &str,
        left_nav: usize,
        right_nav: usize,
    ) -> bool {
        self.record(format!(
            "expand:{delete_count}:{text}:{left_nav}:{right_nav}"
        ));
        true
    }

    fn inject_atomic_backspaces(&self, count: usize) {
        self.record(format!("backspaces:{count}"));
    }

    fn inject_unicode_text_direct(&self, text: &str) -> bool {
        self.record(format!("unicode:{text}"));
        true
    }

    fn inject_atomic_undo(&self, backspaces: usize, text: &str) -> bool {
        self.record(format!("undo:{backspaces}:{text}"));
        true
    }
}

#[cfg(test)]
static FAKE_INJECTOR: RecordingInjector = RecordingInjector {
    calls: std::sync::Mutex::new(Vec::new()),
};

/// Test accessor for assertions on recorded injection calls.
#[cfg(test)]
pub fn test_injector() -> &'static RecordingInjector {
    &FAKE_INJECTOR
}

#[allow(clippy::needless_return)]
pub fn get_clipboard_manager() -> Result<impl ClipboardManager, String> {
    #[cfg(test)]
    {
        return Ok(FakeClipboard::default());
    }
    #[cfg(all(not(test), windows))]
    {
        return Ok(windows::WindowsClipboard);
    }
    #[cfg(all(not(test), target_os = "linux"))]
    {
        return Ok(linux::LinuxClipboard);
    }
    #[cfg(all(not(test), target_os = "macos"))]
    {
        return Ok(macos::clipboard::MacosClipboard);
    }
    #[cfg(all(
        not(test),
        not(windows),
        not(target_os = "linux"),
        not(target_os = "macos")
    ))]
    {
        return arboard::Clipboard::new().map_err(|e| e.to_string());
    }
}

pub fn get_injector() -> &'static dyn Injector {
    #[cfg(test)]
    {
        &FAKE_INJECTOR
    }
    #[cfg(all(not(test), target_os = "linux"))]
    {
        &linux::injector::LinuxInjector
    }
    #[cfg(all(not(test), not(target_os = "linux")))]
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
        // Hermetic: cfg(test) clipboard is FakeClipboard, never the host clipboard.
        let mut clip = get_clipboard_manager().expect("fake clipboard");
        clip.set_text("hermetic payload").expect("set");
        assert_eq!(clip.get_text().expect("get"), "hermetic payload");
        let _ = read_clipboard_text();
    }

    #[test]
    fn test_host_gate_defaults_to_closed() {
        // Guard against accidental opt-in: host tests run only with explicit env.
        assert!(!host_tests_allowed() || std::env::var("TAURINE_ALLOW_HOST_INPUT").is_ok());
    }

    #[test]
    fn test_get_active_window_label_caching() {
        let first = get_active_window_label();
        let second = get_active_window_label();
        assert_eq!(first, second);
    }
}
