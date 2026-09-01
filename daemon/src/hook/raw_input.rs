// daemon/src/hook/raw_input.rs
use crate::input::hook_health::HookHealth;

use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};
use std::thread;
use tracing::{error, info};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::{
    GetRawInputData, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK,
    RIM_TYPEKEYBOARD, RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostQuitMessage,
    RegisterClassW, TranslateMessage, WM_DESTROY, WM_INPUT, WNDCLASSW,
};

const WINDOW_CLASS_NAME: &str = "TaurineRawInputMonitor";

static RAW_INPUT_THREAD_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static RAW_INPUT_JOIN_HANDLE: Mutex<Option<thread::JoinHandle<()>>> = Mutex::new(None);

pub struct RawInputContext {
    pub hook_health: Option<HookHealth>,
    pub supervisor_tx:
        Option<std::sync::mpsc::Sender<crate::hook::supervisor::WindowsSupervisorEvent>>,
}

fn raw_input_context() -> &'static Mutex<Option<RawInputContext>> {
    static CONTEXT: OnceLock<Mutex<Option<RawInputContext>>> = OnceLock::new();
    CONTEXT.get_or_init(|| Mutex::new(None))
}

/// Start raw input listener thread
pub fn start_raw_input_listener(ctx: RawInputContext) -> Result<(), String> {
    let mut slot = raw_input_context()
        .lock()
        .map_err(|_| "Raw Input context lock poisoned".to_string())?;
    *slot = Some(ctx);

    let handle = thread::Builder::new()
        .name("tau-raw-input".to_string())
        .spawn(run_raw_input_message_loop)
        .map_err(|e| e.to_string())?;

    if let Ok(mut lock) = RAW_INPUT_JOIN_HANDLE.lock() {
        *lock = Some(handle);
    }
    Ok(())
}

/// Stop raw input listener thread
pub fn stop_raw_input_listener() {
    if let Ok(mut slot) = raw_input_context().lock() {
        *slot = None;
    }

    let tid = RAW_INPUT_THREAD_ID.load(std::sync::atomic::Ordering::Relaxed);
    if tid != 0 {
        // SAFETY: PostThreadMessageW posts a thread message to the raw input thread.
        // We post WM_QUIT to break the message loop.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                tid,
                windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT,
                0,
                0,
            );
        }
        RAW_INPUT_THREAD_ID.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    if let Ok(mut lock) = RAW_INPUT_JOIN_HANDLE.lock()
        && let Some(handle) = lock.take()
    {
        let _ = handle.join();
    }
}

fn run_raw_input_message_loop() {
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    RAW_INPUT_THREAD_ID.store(tid, std::sync::atomic::Ordering::Relaxed);

    info!("Starting Raw Input keyboard monitor");

    let class_name = wide_null(WINDOW_CLASS_NAME);
    let instance = unsafe { GetModuleHandleW(null_mut()) };
    let window_class = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(raw_input_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: null_mut(),
        lpszClassName: class_name.as_ptr(),
    };

    let class_atom = unsafe { RegisterClassW(&window_class) };
    if class_atom == 0 {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if err != 1410 {
            // 1410 is ERROR_CLASS_ALREADY_EXISTS
            error!(
                "Failed to register Raw Input monitor window class: error {}",
                err
            );
            return;
        }
    }

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            windows_sys::Win32::UI::WindowsAndMessaging::HWND_MESSAGE,
            0 as windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
            instance,
            null_mut(),
        )
    };

    if hwnd.is_null() {
        error!("Failed to create Message-Only Window for Raw Input");
        return;
    }

    // RIDEV_INPUTSINK enables receiving raw input even when the window does not have focus (e.g. UAC/secure desktop).
    let device = RAWINPUTDEVICE {
        usUsagePage: 0x01, // Generic Desktop Controls
        usUsage: 0x06,     // Keyboard
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };

    let register_result = unsafe {
        RegisterRawInputDevices(&device, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
    };

    if register_result == 0 {
        error!("Failed to register Raw Input device");
        return;
    }

    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    info!("Stopped Raw Input keyboard monitor");
}

unsafe extern "system" fn raw_input_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_INPUT => {
            let mut size: u32 = 0;
            let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;

            let result = unsafe {
                GetRawInputData(
                    lparam as windows_sys::Win32::UI::Input::HRAWINPUT,
                    RID_INPUT,
                    null_mut(),
                    &mut size,
                    header_size,
                )
            };

            if result == u32::MAX || size == 0 {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }

            let mut stack_buffer = [0u8; 256];
            let mut heap_buffer: Vec<u8>;
            let (raw_ptr, read_result) = if (size as usize) <= stack_buffer.len() {
                let res = unsafe {
                    GetRawInputData(
                        lparam as windows_sys::Win32::UI::Input::HRAWINPUT,
                        RID_INPUT,
                        stack_buffer.as_mut_ptr() as *mut std::ffi::c_void,
                        &mut size,
                        header_size,
                    )
                };
                (stack_buffer.as_ptr() as *const RAWINPUT, res)
            } else {
                heap_buffer = vec![0u8; size as usize];
                let res = unsafe {
                    GetRawInputData(
                        lparam as windows_sys::Win32::UI::Input::HRAWINPUT,
                        RID_INPUT,
                        heap_buffer.as_mut_ptr() as *mut std::ffi::c_void,
                        &mut size,
                        header_size,
                    )
                };
                (heap_buffer.as_ptr() as *const RAWINPUT, res)
            };

            if read_result == u32::MAX {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }

            let raw = raw_ptr;
            unsafe {
                if (*raw).header.dwType == RIM_TYPEKEYBOARD {
                    let keyboard = &(*raw).data.keyboard;
                    let flags = keyboard.Flags as u32;
                    let is_press = (flags & 0x01) == 0;

                    if is_press
                        && let Ok(slot) = raw_input_context().lock()
                        && let Some(ref ctx) = *slot
                        && let Some(ref health) = ctx.hook_health
                    {
                        // Check if consecutive physical keypresses were missed by the low-level hook
                        // with at least 300ms grace window since the last acknowledged hook event.
                        if health.check_raw_input_keystroke_and_evaluate(true, 300, 3) {
                            if !crate::platform::windows::is_foreground_window_elevated_or_restricted() {
                                if let Some(ref tx) = ctx.supervisor_tx {
                                    let _ = tx.send(
                                        crate::hook::supervisor::WindowsSupervisorEvent::HookUnresponsive,
                                    );
                                }
                                health.mark_recovery_signal(
                                    "raw input watchdog: low-level hook unresponsive (3 consecutive missed inputs)",
                                );
                            } else {
                                tracing::debug!("raw input watchdog: suppressing recovery for elevated/restricted foreground window");
                            }
                        }
                    }
                }
            }
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn wide_null(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
