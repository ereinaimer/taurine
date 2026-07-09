use std::sync::Arc;
use std::sync::atomic::Ordering;
use taurine_core::engine::EngineState;
use tracing::error;
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_FOREGROUND, GWL_STYLE,
    GetClassNameW, GetClientRect, GetForegroundWindow, GetMessageW, GetWindowLongW, GetWindowRect,
    MSG, OBJID_WINDOW, SetTimer, TranslateMessage, WINEVENT_OUTOFCONTEXT, WS_CAPTION,
};

thread_local! {
    static ENGINE_STATE: std::cell::RefCell<Option<Arc<EngineState>>> = const { std::cell::RefCell::new(None) };
}

pub fn start_listener(state: Arc<EngineState>) {
    std::thread::spawn(move || {
        ENGINE_STATE.with(|s| *s.borrow_mut() = Some(state));

        // SAFETY: All Win32 calls in this block operate on the current thread's
        // message queue. SetWinEventHook registers callback functions for system
        // events (foreground window changes and object location changes). The
        // null hmod argument (third param) means the callback lives in this module.
        // SetTimer creates a 100ms timer with a callback function. GetMessageW,
        // TranslateMessage, and DispatchMessageW drive the message loop.
        // `win_event_proc` and `timer_proc` are safe `extern "system"` functions.
        // `std::mem::zeroed()` is safe for MSG (it is a POD struct). All HWND
        // null checks are done before use. The WinEvent hooks are never explicitly
        // unhooked — they are automatically removed when the thread terminates.
        unsafe {
            let hook_foreground = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );

            let hook_location = SetWinEventHook(
                EVENT_OBJECT_LOCATIONCHANGE,
                EVENT_OBJECT_LOCATIONCHANGE,
                std::ptr::null_mut(),
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );

            if hook_foreground.is_null() || hook_location.is_null() {
                error!("Failed to set Windows event hooks for fullscreen detection.");
                return;
            }

            // Fallback polling timer (runs every 100ms) to handle edge cases
            // where WinEvents are silently dropped or delayed.
            SetTimer(std::ptr::null_mut(), 0, 100, Some(timer_proc));

            check_fullscreen_state();

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

// SAFETY: WinEvent callback invoked by the OS when a WinEvent is fired. The
// function is registered via SetWinEventHook and must use `system` ABI. `hwnd`
// is checked for null before use. GetForegroundWindow always returns a valid
// handle or null with no error path.
unsafe extern "system" fn win_event_proc(
    _hwineventhook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    idobject: i32,
    _idchild: i32,
    _ideventthread: u32,
    _dwmsceventtime: u32,
) {
    if hwnd.is_null() || idobject != OBJID_WINDOW {
        return;
    }

    if event == EVENT_OBJECT_LOCATIONCHANGE {
        // SAFETY: GetForegroundWindow returns a handle to the foreground window
        // (or null). No error state; always safe to call from any thread.
        let fg = unsafe { GetForegroundWindow() };
        if fg != hwnd {
            return;
        }
    }

    check_fullscreen_state();
}

// SAFETY: Timer callback invoked by the OS message loop. Registered via SetTimer.
// Must use `system` ABI. The parameters are ignored — only the polling side-effect
// (check_fullscreen_state) is needed. The callback is always safe to call from
// the thread that created the timer.
unsafe extern "system" fn timer_proc(_hwnd: HWND, _msg: u32, _id_event: usize, _time: u32) {
    check_fullscreen_state();
}

fn check_fullscreen_state() {
    let is_fullscreen = check_borderless_fullscreen();

    ENGINE_STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            state
                .is_os_fullscreen
                .store(is_fullscreen, Ordering::Relaxed);
        }
    });
}

fn check_borderless_fullscreen() -> bool {
    // SAFETY: All Win32 calls here operate on the foreground window retrieved
    // by GetForegroundWindow. GetClassNameW writes into a stack buffer (256
    // u16) — we pass the buffer size to prevent overflow. GetWindowLongW reads
    // window styles (safe read-only). GetWindowRect, GetClientRect, and
    // ClientToScreen write to stack-local RECT/POINT structs initialized with
    // zeroed(). MonitorFromWindow and GetMonitorInfoW retrieve monitor info.
    // Null HWNDs are checked before each call. All buffer sizes are accurate.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }

        let mut class_name = [0u16; 256];
        let len = GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32);
        if len > 0 {
            let class_str = String::from_utf16_lossy(&class_name[..len as usize]);
            if class_str == "Progman" || class_str == "WorkerW" || class_str == "Shell_TrayWnd" {
                return false;
            }
        }

        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        if (style & WS_CAPTION) == WS_CAPTION {
            // A window with a title bar/border is not an exclusive fullscreen application,
            // even if it is maximized and the user has auto-hide taskbar enabled.
            return false;
        }

        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return false;
        }

        let hmonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;

        if GetMonitorInfoW(hmonitor, &mut mi) == 0 {
            return false;
        }

        // Fast bounds check to exclude windows that don't cover the screen at all
        if rect.left > mi.rcMonitor.left
            || rect.top > mi.rcMonitor.top
            || rect.right < mi.rcMonitor.right
            || rect.bottom < mi.rcMonitor.bottom
        {
            return false;
        }

        // For borderless fullscreen, the client area must cover the entire monitor.
        // This distinguishes true fullscreen from a maximized window (which leaves room for the taskbar).
        let mut client_rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut client_rect) == 0 {
            return false;
        }

        let mut pt: POINT = std::mem::zeroed();
        ClientToScreen(hwnd, &mut pt);

        let client_left = pt.x;
        let client_top = pt.y;
        let client_right = pt.x + client_rect.right;
        let client_bottom = pt.y + client_rect.bottom;

        client_left <= mi.rcMonitor.left
            && client_top <= mi.rcMonitor.top
            && client_right >= mi.rcMonitor.right
            && client_bottom >= mi.rcMonitor.bottom
    }
}
