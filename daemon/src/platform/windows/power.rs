use std::ptr::null;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::thread;

use tracing::{debug, error, info, warn};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PBT_APMRESUMEAUTOMATIC,
    PBT_APMRESUMESUSPEND, PostQuitMessage, RegisterClassW, TranslateMessage, WM_DESTROY,
    WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW,
};

use crate::hook::WindowsSupervisorEvent;

const WINDOW_CLASS_NAME: &str = "TaurinePowerSessionMonitor";
const WTS_SESSION_LOGON: u32 = 0x5;
const WTS_SESSION_UNLOCK: u32 = 0x8;

fn event_sender() -> &'static Mutex<Option<Sender<WindowsSupervisorEvent>>> {
    static EVENT_SENDER: OnceLock<Mutex<Option<Sender<WindowsSupervisorEvent>>>> = OnceLock::new();
    EVENT_SENDER.get_or_init(|| Mutex::new(None))
}

pub fn start_listener(tx: Sender<WindowsSupervisorEvent>) -> Result<(), String> {
    let mut slot = event_sender()
        .lock()
        .map_err(|_| "Windows power/session sender lock is poisoned".to_string())?;
    *slot = Some(tx);

    thread::Builder::new()
        .name("taurine-win-power".to_string())
        .spawn(run_message_loop)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn run_message_loop() {
    info!("Starting Windows power/session monitor");

    let class_name = wide_null(WINDOW_CLASS_NAME);
    let instance = unsafe { GetModuleHandleW(null()) };
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };

    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        error!("Failed to register Windows power/session monitor window class");
        return;
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
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        error!("Failed to create Windows power/session monitor window");
        return;
    }

    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
        warn!("Failed to register Windows session notifications");
    } else {
        debug!("Registered Windows session notifications");
    }

    info!("Windows power/session monitor is listening for resume and unlock events");

    let mut message = MSG::default();
    loop {
        let status = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if status > 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        } else if status == 0 {
            break;
        } else {
            error!("Windows power/session monitor message loop failed");
            break;
        }
    }

    unsafe {
        let _ = WTSUnRegisterSessionNotification(hwnd);
    }
    info!("Windows power/session monitor exited");
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_POWERBROADCAST => match wparam as u32 {
            PBT_APMRESUMEAUTOMATIC => {
                info!("Detected Windows automatic resume event");
                send_event(WindowsSupervisorEvent::ResumeAutomatic);
                1
            }
            PBT_APMRESUMESUSPEND => {
                info!("Detected Windows resume-from-suspend event");
                send_event(WindowsSupervisorEvent::ResumeFromSuspend);
                1
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        },
        WM_WTSSESSION_CHANGE => match wparam as u32 {
            WTS_SESSION_UNLOCK => {
                info!("Detected Windows session unlock event");
                send_event(WindowsSupervisorEvent::SessionUnlock);
                0
            }
            WTS_SESSION_LOGON => {
                info!("Detected Windows session logon event");
                send_event(WindowsSupervisorEvent::SessionLogon);
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        },
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn send_event(event: WindowsSupervisorEvent) {
    let Ok(slot) = event_sender().lock() else {
        warn!("Windows power/session event dropped because the sender lock is poisoned");
        return;
    };

    if let Some(tx) = slot.as_ref() {
        if let Err(error) = tx.send(event) {
            warn!(error = %error, "Failed to forward Windows power/session event");
        }
    } else {
        debug!("Windows power/session monitor event ignored because no supervisor is active");
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
