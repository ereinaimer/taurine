// daemon/src/hook/raw_input.rs
use crate::input::hook_health::{HookHealth, KeyboardCaptureState};
use rdev::{Event, EventType, Key};
use std::ptr::null_mut;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread;
use taurine_core::engine::Evaluator;
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
    pub evaluator: Arc<Mutex<Evaluator>>,
    pub state: Arc<taurine_core::engine::EngineState>,
    pub paused: Arc<std::sync::atomic::AtomicBool>,
    pub pause_hotkey: Arc<RwLock<crate::input::hotkey::HotkeySpec>>,
    pub spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    pub pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
    pub left_alt_down: Arc<std::sync::atomic::AtomicBool>,
    pub right_alt_down: Arc<std::sync::atomic::AtomicBool>,
    pub left_ctrl_down: Arc<std::sync::atomic::AtomicBool>,
    pub right_ctrl_down: Arc<std::sync::atomic::AtomicBool>,
    pub left_shift_down: Arc<std::sync::atomic::AtomicBool>,
    pub right_shift_down: Arc<std::sync::atomic::AtomicBool>,
    pub left_meta_down: Arc<std::sync::atomic::AtomicBool>,
    pub right_meta_down: Arc<std::sync::atomic::AtomicBool>,
    pub hotkey_evaluator: Arc<Mutex<crate::input::hotkey_evaluator::HotkeyEvaluator>>,
    pub event_counter: Arc<std::sync::atomic::AtomicU32>,
    pub hook_health: Option<HookHealth>,
}

fn raw_input_context() -> &'static Mutex<Option<RawInputContext>> {
    static CONTEXT: OnceLock<Mutex<Option<RawInputContext>>> = OnceLock::new();
    CONTEXT.get_or_init(|| Mutex::new(None))
}

static LAST_LL_HOOK_EVENT: OnceLock<Mutex<Option<(u16, bool, std::time::Instant)>>> =
    OnceLock::new();

fn get_last_ll_hook_event() -> &'static Mutex<Option<(u16, bool, std::time::Instant)>> {
    LAST_LL_HOOK_EVENT.get_or_init(|| Mutex::new(None))
}

pub fn record_ll_hook_event(vk_code: u16, is_press: bool) {
    if let Ok(mut lock) = get_last_ll_hook_event().lock() {
        *lock = Some((vk_code, is_press, std::time::Instant::now()));
    }
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
        // We post WM_DESTROY (or WM_QUIT) to break the message loop.
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

            let mut buffer = vec![0u8; size as usize];
            let read_result = unsafe {
                GetRawInputData(
                    lparam as windows_sys::Win32::UI::Input::HRAWINPUT,
                    RID_INPUT,
                    buffer.as_mut_ptr() as *mut std::ffi::c_void,
                    &mut size,
                    header_size,
                )
            };

            if read_result == u32::MAX {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }

            let raw = buffer.as_ptr() as *const RAWINPUT;
            unsafe {
                if (*raw).header.dwType == RIM_TYPEKEYBOARD {
                    let keyboard = &(*raw).data.keyboard;
                    let vk_code = keyboard.VKey;
                    let flags = keyboard.Flags as u32;
                    let is_press = (flags & 0x01) == 0;

                    // Deduplicate against the Low-Level Hook's synchronous events
                    let is_dup = if let Ok(lock) = get_last_ll_hook_event().lock() {
                        if let Some((last_vk, last_press, last_time)) = *lock {
                            last_vk == vk_code
                                && last_press == is_press
                                && last_time.elapsed() < std::time::Duration::from_millis(20)
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if is_dup {
                        return 0;
                    }

                    if let Ok(slot) = raw_input_context().lock()
                        && let Some(ref ctx) = *slot
                    {
                        // Check if the Low-Level Hook is healthy.
                        // If the hook is healthy, we ignore the Raw Input event entirely to prevent duplicates.
                        let state = ctx
                            .hook_health
                            .as_ref()
                            .map(|h| h.snapshot().keyboard_capture_state())
                            .unwrap_or(KeyboardCaptureState::Unknown);

                        if state == KeyboardCaptureState::Healthy {
                            return 0;
                        }

                        let key = vk_to_rdev_key(vk_code);
                        let event_type = if is_press {
                            EventType::KeyPress(key)
                        } else {
                            EventType::KeyRelease(key)
                        };

                        let event = Event {
                            event_type,
                            time: std::time::SystemTime::now(),
                            name: None,
                        };

                        // Process the keyboard event synchronously on the Raw Input thread
                        let _ = crate::hook::listener::process_keyboard_event(
                            event,
                            &ctx.evaluator,
                            &ctx.state,
                            &ctx.paused,
                            &ctx.pause_hotkey,
                            &ctx.spinner_style,
                            &ctx.pause_transition_tx,
                            &ctx.left_alt_down,
                            &ctx.right_alt_down,
                            &ctx.left_ctrl_down,
                            &ctx.right_ctrl_down,
                            &ctx.left_shift_down,
                            &ctx.right_shift_down,
                            &ctx.left_meta_down,
                            &ctx.right_meta_down,
                            &ctx.hotkey_evaluator,
                            &ctx.event_counter,
                        );
                    }
                }
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn vk_to_rdev_key(vk: u16) -> Key {
    match vk {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => Key::Return,
        0x10 => Key::ShiftLeft,
        0x11 => Key::ControlLeft,
        0x12 => Key::Alt,
        0x14 => Key::CapsLock,
        0x1B => Key::Escape,
        0x20 => Key::Space,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::LeftArrow,
        0x26 => Key::UpArrow,
        0x27 => Key::RightArrow,
        0x28 => Key::DownArrow,
        0x2D => Key::Insert,
        0x2E => Key::Delete,
        0x30 => Key::Num0,
        0x31 => Key::Num1,
        0x32 => Key::Num2,
        0x33 => Key::Num3,
        0x34 => Key::Num4,
        0x35 => Key::Num5,
        0x36 => Key::Num6,
        0x37 => Key::Num7,
        0x38 => Key::Num8,
        0x39 => Key::Num9,
        0x41 => Key::KeyA,
        0x42 => Key::KeyB,
        0x43 => Key::KeyC,
        0x44 => Key::KeyD,
        0x45 => Key::KeyE,
        0x46 => Key::KeyF,
        0x47 => Key::KeyG,
        0x48 => Key::KeyH,
        0x49 => Key::KeyI,
        0x4A => Key::KeyJ,
        0x4B => Key::KeyK,
        0x4C => Key::KeyL,
        0x4D => Key::KeyM,
        0x4E => Key::KeyN,
        0x4F => Key::KeyO,
        0x50 => Key::KeyP,
        0x51 => Key::KeyQ,
        0x52 => Key::KeyR,
        0x53 => Key::KeyS,
        0x54 => Key::KeyT,
        0x55 => Key::KeyU,
        0x56 => Key::KeyV,
        0x57 => Key::KeyW,
        0x58 => Key::KeyX,
        0x59 => Key::KeyY,
        0x5A => Key::KeyZ,
        0x5B => Key::MetaLeft,
        0x60 => Key::Kp0,
        0x61 => Key::Kp1,
        0x62 => Key::Kp2,
        0x63 => Key::Kp3,
        0x64 => Key::Kp4,
        0x65 => Key::Kp5,
        0x66 => Key::Kp6,
        0x67 => Key::Kp7,
        0x68 => Key::Kp8,
        0x69 => Key::Kp9,
        0x6A => Key::KpMultiply,
        0x6B => Key::KpPlus,
        0x6D => Key::KpMinus,
        0x6E => Key::KpDelete,
        0x6F => Key::KpDivide,
        0x70 => Key::F1,
        0x71 => Key::F2,
        0x72 => Key::F3,
        0x73 => Key::F4,
        0x74 => Key::F5,
        0x75 => Key::F6,
        0x76 => Key::F7,
        0x77 => Key::F8,
        0x78 => Key::F9,
        0x79 => Key::F10,
        0x7A => Key::F11,
        0x7B => Key::F12,
        0x90 => Key::NumLock,
        0x91 => Key::ScrollLock,
        0xA0 => Key::ShiftLeft,
        0xA1 => Key::ShiftRight,
        0xA2 => Key::ControlLeft,
        0xA3 => Key::ControlRight,
        0xA4 => Key::Alt,
        0xA5 => Key::AltGr,
        0xBA => Key::SemiColon,
        0xBB => Key::Equal,
        0xBC => Key::Comma,
        0xBD => Key::Minus,
        0xBE => Key::Dot,
        0xBF => Key::Slash,
        0xC0 => Key::BackQuote,
        0xDB => Key::LeftBracket,
        0xDC => Key::BackSlash,
        0xDD => Key::RightBracket,
        0xDE => Key::Quote,
        0xE2 => Key::IntlBackslash,
        vk => Key::Unknown(vk as u32),
    }
}
