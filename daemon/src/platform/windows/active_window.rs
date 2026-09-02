use windows_sys::Win32::Foundation::{CloseHandle, MAX_PATH};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

pub fn get_active_window_info() -> Option<taurine_core::engine::ActiveWindowInfo> {
    // SAFETY: GetForegroundWindow returns a valid HWND or null.
    // GetWindowThreadProcessId, OpenProcess, QueryFullProcessImageNameW, GetClassNameW, GetWindowTextW
    // and CloseHandle are Win32 APIs, safe with null/bounds checks.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }

        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        if process_id == 0 {
            return None;
        }

        // 1. Get window class name
        let mut class_buf = [0u16; 256];
        let class_len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32);
        let class_name = if class_len > 0 {
            Some(String::from_utf16_lossy(&class_buf[..class_len as usize]))
        } else {
            None
        };

        // 2. Get window title
        let mut title_buf = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
        let title = if title_len > 0 {
            Some(String::from_utf16_lossy(&title_buf[..title_len as usize]))
        } else {
            None
        };

        // 3. Get process path & name
        let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process_handle.is_null() {
            return Some(taurine_core::engine::ActiveWindowInfo {
                title,
                class: class_name,
                exec_name: None,
                exec_path: None,
            });
        }

        let mut buffer = [0u16; MAX_PATH as usize];
        let mut size = buffer.len() as u32;
        let success = QueryFullProcessImageNameW(process_handle, 0, buffer.as_mut_ptr(), &mut size);
        CloseHandle(process_handle);

        let mut exec_path = None;
        let mut exec_name = None;

        if success != 0 && size > 0 {
            let path_str = String::from_utf16_lossy(&buffer[..size as usize]);
            exec_path = Some(path_str.clone());
            if let Some(file_name) = std::path::Path::new(&path_str).file_name() {
                exec_name = Some(file_name.to_string_lossy().to_string());
            }
        }

        Some(taurine_core::engine::ActiveWindowInfo {
            title,
            class: class_name,
            exec_name,
            exec_path,
        })
    }
}

pub fn get_active_window_label() -> Option<String> {
    let info = get_active_window_info()?;
    serde_json::to_string(&info).ok()
}

pub fn is_foreground_window_elevated_or_restricted() -> bool {
    // SAFETY: GetForegroundWindow returns a valid HWND or null.
    // GetWindowThreadProcessId, OpenProcess, and CloseHandle are Win32 APIs, safe with null checks.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }

        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        if process_id == 0 {
            return false;
        }

        let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process_handle.is_null() {
            return true;
        }

        CloseHandle(process_handle);
        false
    }
}
