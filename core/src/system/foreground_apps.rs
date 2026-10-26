//! Foreground applications for the app-filter pickers: the Alt-Tab
//! set (visible top-level windows with titles), most recent first.
//! Windows enumerates the window manager; other platforms return an
//! empty list and the pickers fall back to manual `prefix:value` input.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundApp {
    /// Lowercase exe file name, e.g. `code.exe`.
    pub exe: String,
    /// Full image path when known.
    pub path: String,
    /// Window class, e.g. `Chrome_WidgetWin_1`.
    pub class: String,
    /// Visible window title.
    pub title: String,
}

/// Alt-Tab applications, Z-order (most recent first), deduped by exe.
pub fn list_foreground_apps() -> Vec<ForegroundApp> {
    #[cfg(windows)]
    return dedupe_apps(windows_impl::list());
    #[cfg(not(windows))]
    return Vec::new();
}

/// Dedupe by lowercase exe; first (most recent) entry wins.
pub fn dedupe_apps(apps: Vec<ForegroundApp>) -> Vec<ForegroundApp> {
    let mut seen = std::collections::HashSet::new();
    apps.into_iter()
        .filter(|app| seen.insert(app.exe.to_lowercase()))
        .collect()
}

#[cfg(windows)]
mod windows_impl {
    use super::ForegroundApp;
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetWindow, GetWindowLongW,
        GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    use windows::core::{BOOL, PWSTR};

    struct RawWindow {
        hwnd: HWND,
        title: String,
        class: String,
    }

    fn window_text(hwnd: HWND) -> String {
        // SAFETY: GetWindowTextLengthW with a live enumerated HWND only
        // reads the window's title; the +1 buffer holds the NUL.
        let len = unsafe { GetWindowTextLengthW(hwnd) } as usize;
        if len == 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len + 1];
        // SAFETY: buffer sized len+1 exactly; GetWindowTextW writes at
        // most len chars plus NUL and returns the count copied.
        let copied = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
        String::from_utf16_lossy(&buf[..copied.min(len)])
    }

    fn class_name(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        // SAFETY: fixed 256-wide stack buffer; GetClassNameW writes at
        // most its length and returns the count copied.
        let copied = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
        String::from_utf16_lossy(&buf[..copied.min(buf.len())])
    }

    fn ex_style(hwnd: HWND) -> u32 {
        // SAFETY: GetWindowLongW on a live HWND reads style bits only.
        unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 }
    }

    fn exe_of(hwnd: HWND) -> Option<(String, String)> {
        let mut pid = 0u32;
        // SAFETY: pid is a plain out-param; the HWND is live.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 {
            return None;
        }
        // SAFETY: OpenProcess with query-only rights never injects;
        // the handle closes on every path below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: len tracks the buffer capacity; the API updates it to
        // the chars written (sans NUL) on success.
        let ok = unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len as *mut u32,
            )
            .is_ok()
        };
        // SAFETY: CloseHandle on an owned process handle is valid once.
        unsafe { CloseHandle(handle).ok() };
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let exe = std::path::Path::new(&path)
            .file_name()?
            .to_string_lossy()
            .to_lowercase();
        Some((exe, path))
    }

    // SAFETY: EnumWindows invokes this on the calling thread with live
    // HWNDs; the lparam Vec outlives the enumeration.
    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<RawWindow>) };
        // SAFETY: IsWindowVisible only queries visibility state.
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let title = window_text(hwnd);
            if !title.trim().is_empty() {
                let style = ex_style(hwnd);
                let owned =
                    // SAFETY: GetWindow(GW_OWNER) only reads the owner link.
                    unsafe { GetWindow(hwnd, GW_OWNER).is_ok() };
                let tool = style & WS_EX_TOOLWINDOW.0 != 0;
                let app_window = style & WS_EX_APPWINDOW.0 != 0;
                // honey: the Alt-Tab rule — tool windows and owned
                // popups never list; APPWINDOW forces listing.
                if (!tool || app_window) && (!owned || app_window) {
                    out.push(RawWindow {
                        hwnd,
                        title,
                        class: class_name(hwnd),
                    });
                }
            }
        }
        BOOL(1)
    }

    pub(super) fn list() -> Vec<ForegroundApp> {
        let mut raw: Vec<RawWindow> = Vec::new();
        // SAFETY: callback only pushes into the lparam Vec, which
        // outlives this call; enumeration is synchronous.
        unsafe { EnumWindows(Some(enum_callback), LPARAM(&mut raw as *mut _ as isize)).ok() };
        raw.into_iter()
            .filter_map(|window| {
                let (exe, path) = exe_of(window.hwnd)?;
                Some(ForegroundApp {
                    exe,
                    path,
                    class: window.class,
                    title: window.title,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exe: &str, title: &str) -> ForegroundApp {
        ForegroundApp {
            exe: exe.to_string(),
            path: format!("C:\\bin\\{exe}"),
            class: "SomeClass".to_string(),
            title: title.to_string(),
        }
    }

    #[test]
    fn dedupe_keeps_most_recent_exe() {
        let apps = dedupe_apps(vec![
            app("code.exe", "new window"),
            app("notepad.exe", "notes"),
            app("CODE.EXE", "old window"),
        ]);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].title, "new window");
        assert_eq!(apps[1].exe, "notepad.exe");
    }

    #[test]
    fn dedupe_empty_stays_empty() {
        assert!(dedupe_apps(Vec::new()).is_empty());
    }
}
