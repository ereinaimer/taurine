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

static POWER_THREAD_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static POWER_JOIN_HANDLE: Mutex<Option<thread::JoinHandle<()>>> = Mutex::new(None);

fn event_sender() -> &'static Mutex<Option<Sender<WindowsSupervisorEvent>>> {
    static EVENT_SENDER: OnceLock<Mutex<Option<Sender<WindowsSupervisorEvent>>>> = OnceLock::new();
    EVENT_SENDER.get_or_init(|| Mutex::new(None))
}

pub fn start_listener(tx: Sender<WindowsSupervisorEvent>) -> Result<(), String> {
    let mut slot = event_sender()
        .lock()
        .map_err(|_| "Windows power/session sender lock is poisoned".to_string())?;
    *slot = Some(tx);

    let handle = thread::Builder::new()
        .name("taurine-win-power".to_string())
        .spawn(run_message_loop)
        .map_err(|error| error.to_string())?;

    if let Ok(mut lock) = POWER_JOIN_HANDLE.lock() {
        *lock = Some(handle);
    }
    Ok(())
}

fn run_message_loop() {
    // SAFETY: GetCurrentThreadId retrieves the OS thread ID of the calling thread.
    // It always succeeds and has no failure modes.
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    POWER_THREAD_ID.store(tid, std::sync::atomic::Ordering::Relaxed);

    info!("Starting Windows power/session monitor");

    let class_name = wide_null(WINDOW_CLASS_NAME);
    // SAFETY: GetModuleHandleW(null()) returns the calling process's executable
    // module handle. Passing null is documented and always succeeds. The returned
    // pseudo-handle is valid for the lifetime of the process.
    let instance = unsafe { GetModuleHandleW(null()) };
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };

    // SAFETY: RegisterClassW registers a window class. `window_class` is a fully
    // initialized WNDCLASSW with valid `lpfnWndProc` and `lpszClassName` pointers.
    // The pointer to `window_class` is a stack-local that remains valid during the
    // call. Returns 0 on failure, checked below.
    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        error!("Failed to register Windows power/session monitor window class");
        return;
    }

    // SAFETY: CreateWindowExW creates a hidden window to receive power/session
    // broadcast messages. `class_name.as_ptr()` is a valid null-terminated UTF-16
    // string registered above. `instance` is the valid module handle. The HWND is
    // checked for null before use.
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

    // SAFETY: WTSRegisterSessionNotification registers `hwnd` to receive
    // WM_WTSSESSION_CHANGE messages. `hwnd` is the valid window created above.
    // NOTIFY_FOR_THIS_SESSION limits notifications to this session. Returns 0
    // on failure, which we check below.
    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
        warn!("Failed to register Windows session notifications");
    } else {
        debug!("Registered Windows session notifications");
    }

    info!("Windows power/session monitor is listening for resume and unlock events");

    let mut message = MSG::default();
    loop {
        // SAFETY: GetMessageW retrieves messages from the calling thread's queue.
        // `&mut message` is a valid stack pointer to MSG. null HWND retrieves all
        // thread messages. Returns -1 on error, 0 on WM_QUIT, >0 otherwise.
        let status = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if status > 0 {
            // SAFETY: TranslateMessage converts virtual-key codes to character
            // messages; safe on any initialized MSG. DispatchMessageW dispatches
            // the message to the window's registered procedure. `&message` is a
            // valid stack pointer to MSG.
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

    // SAFETY: WTSUnRegisterSessionNotification unregisters `hwnd` from session
    // notifications. `hwnd` is the valid window created above. Called during
    // message loop exit while the window still exists.
    unsafe {
        let _ = WTSUnRegisterSessionNotification(hwnd);
    }
    info!("Windows power/session monitor exited");
}

// SAFETY: Win32 window procedure called by the OS. `hwnd` is the message-only
// window created in run_message_loop. Registered via WNDCLASSW.lpfnWndProc.
// Must use `system` ABI to match Win32 calling convention expectations.
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
            _ => {
                // SAFETY: Default handling for unhandled power broadcast messages.
                // `hwnd` is valid, `message`/`wparam`/`lparam` are OS-provided.
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
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
            _ => {
                // SAFETY: Default handling for unhandled session change messages.
                // `hwnd` is valid, `message`/`wparam`/`lparam` are OS-provided.
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        },
        WM_DESTROY => {
            // SAFETY: PostQuitMessage posts WM_QUIT to the calling thread's message
            // queue. Called on WM_DESTROY to exit the message loop. The exit code
            // (0) is passed to GetMessageW's return value.
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => {
            // SAFETY: Default DefWindowProcW for any unhandled message.
            // `hwnd` is valid, all parameters are OS-provided.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
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

pub fn stop_listener() {
    let tid = POWER_THREAD_ID.load(std::sync::atomic::Ordering::Relaxed);
    if tid != 0 {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        // SAFETY: PostThreadMessageW is safe to call with a valid thread ID and message.
        unsafe {
            for _ in 0..100 {
                if PostThreadMessageW(tid, WM_QUIT, 0, 0) != 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    let handle = if let Ok(mut lock) = POWER_JOIN_HANDLE.lock() {
        lock.take()
    } else {
        None
    };

    if let Some(h) = handle {
        let res = h.join();
        if let Err(e) = res {
            warn!("Error joining power listener thread: {:?}", e);
        }
    }

    if let Ok(mut slot) = event_sender().lock() {
        *slot = None;
    }
}
