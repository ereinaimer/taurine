use crate::hook::listener::{LISTENER_EPOCH, run_listener_once};
use crate::input::hook_health::HookHealth;
use crate::input::hotkey;
use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use taurine_core::engine::Evaluator;
use tracing::{debug, error, info, warn};

pub(super) fn windows_grab(
    callback: impl FnMut(rdev::Event) -> Option<rdev::Event> + 'static,
) -> Result<(), String> {
    use std::cell::RefCell;
    use std::time::SystemTime;
    use windows_sys::Win32::Foundation::LPARAM;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, HC_ACTION, MSG, SetWindowsHookExW, UnhookWindowsHookEx,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    thread_local! {
        // The HHOOK handle for this thread's keyboard hook.
        // *mut std::ffi::c_void because HHOOK is a pointer type.
        static TL_HHOOK: RefCell<*mut c_void> = const { RefCell::new(std::ptr::null_mut()) };

        // The event callback for this thread's hook invocation.
        #[allow(clippy::type_complexity)]
        static TL_CALLBACK: RefCell<Option<Box<dyn FnMut(rdev::Event) -> Option<rdev::Event>>>> =
            const { RefCell::new(None) };

        // Stateful keyboard decoder (dead-key handling, modifier state).
        static TL_KEYBOARD: RefCell<KeyboardDecoder> = RefCell::new(KeyboardDecoder::new());
    }

    TL_CALLBACK.with(|cb| *cb.borrow_mut() = Some(Box::new(callback)));

    // SAFETY: This is the Win32 low-level keyboard hook procedure. It is called by
    // Windows on the thread that installed the hook, so thread-local access is valid.
    // `code`, `wparam`, and `lparam` are provided by Windows and are always valid
    // for a WH_KEYBOARD_LL hook. `lparam` points to a KBDLLHOOKSTRUCT that is valid
    // for the duration of this call.
    unsafe extern "system" fn ll_keyboard_proc(code: i32, wparam: usize, lparam: LPARAM) -> isize {
        unsafe {
            if code == HC_ACTION as i32 {
                // Parse KBDLLHOOKSTRUCT: { vkCode: u32, scanCode: u32, flags: u32, time: u32, dwExtraInfo: usize }
                // SAFETY: lparam is a valid pointer to KBDLLHOOKSTRUCT for the duration of this call.
                let kbds = lparam as *const [u32; 5];
                let vk_code = (*kbds)[0] as u16;
                let scan_code = (*kbds)[1];
                let is_press =
                    matches!(wparam, w if w == WM_KEYDOWN as usize || w == WM_SYSKEYDOWN as usize);

                let key = vk_to_rdev_key(vk_code);
                let event_type = if is_press {
                    rdev::EventType::KeyPress(key)
                } else {
                    rdev::EventType::KeyRelease(key)
                };

                // Resolve the Unicode name of this keypress (for character events).
                // Only needed on KeyPress; KeyRelease never produces a character.
                let name = if is_press {
                    TL_KEYBOARD.with(|kb| {
                        // SAFETY: called from the hook proc; Windows guarantees the
                        // foreground window and keyboard layout are valid here.
                        kb.borrow_mut().get_name(vk_code as u32, scan_code)
                    })
                } else {
                    None
                };

                let event = rdev::Event {
                    event_type,
                    time: SystemTime::now(),
                    name,
                };

                // Record Low-Level Hook event for Raw Input duplicate checking
                crate::hook::raw_input::record_ll_hook_event(vk_code, is_press);

                let pass_through = TL_CALLBACK.with(|cb| {
                    cb.borrow_mut()
                        .as_mut()
                        .map(|f| f(event).is_some())
                        .unwrap_or(true)
                });

                if !pass_through {
                    // Swallowed — do NOT call next hook.
                    return 1;
                }
            }

            let hhook = TL_HHOOK.with(|h| *h.borrow());
            // SAFETY: CallNextHookEx is always safe to call from within a hook proc.
            // hhook is the handle for this thread's keyboard hook.
            CallNextHookEx(hhook, code, wparam, lparam)
        }
    }

    // SAFETY: GetCurrentThread and SetThreadPriority are safe Win32 APIs.
    // Raising priority ensures the thread avoids CPU starvation and OS hook teardowns.
    unsafe {
        windows_sys::Win32::System::Threading::SetThreadPriority(
            windows_sys::Win32::System::Threading::GetCurrentThread(),
            windows_sys::Win32::System::Threading::THREAD_PRIORITY_TIME_CRITICAL,
        );
    }

    // SAFETY: SetWindowsHookExW installs a global WH_KEYBOARD_LL hook.
    // - `ll_keyboard_proc` is a valid `extern "system"` function.
    // - hmod=null_mut() and dwThreadId=0 are correct for a global low-level hook (MSDN).
    // - The hook will fire on the thread running GetMessageW below.
    let hhook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(ll_keyboard_proc),
            std::ptr::null_mut(),
            0,
        )
    };
    if hhook.is_null() {
        return Err(format!(
            "SetWindowsHookExW(WH_KEYBOARD_LL) failed with error {}",
            // SAFETY: GetLastError() is always safe to call.
            unsafe { windows_sys::Win32::Foundation::GetLastError() }
        ));
    }

    TL_HHOOK.with(|h| *h.borrow_mut() = hhook);

    // Run the message pump. WM_QUIT (posted by send_wm_quit_to_thread in the
    // supervisor) causes GetMessageW to return 0, exiting the loop cleanly.
    // SAFETY: GetMessageW is safe to call; it blocks until a message arrives.
    // Passing null_mut() for hwnd, and 0 for min and max means all messages for this thread.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            // No TranslateMessage/DispatchMessageW needed: low-level keyboard
            // hooks are delivered directly through GetMessageW, not through
            // window messages dispatched to a WNDPROC.
        }
    }

    // Explicitly unhook before this thread exits. This is the critical difference
    // from rdev: rdev never calls UnhookWindowsHookEx, so Windows must wait for
    // the OS-level hook timeout (1 second by default) before cleaning up. Explicit
    // unhooking makes teardown deterministic and prevents Windows from treating the
    // newly-spawned hook thread's handle as stale.
    // SAFETY: hhook is the handle we registered above on this thread. Unhooking
    // from the same thread that hooked is always valid.
    unsafe {
        UnhookWindowsHookEx(hhook);
    }
    TL_HHOOK.with(|h| *h.borrow_mut() = std::ptr::null_mut());
    TL_CALLBACK.with(|cb| cb.borrow_mut().take());

    Ok(())
}

/// Decoder that resolves Windows virtual-key codes to Unicode strings using
/// `ToUnicodeEx`, handling dead keys and modifier state the same way rdev does.
struct KeyboardDecoder {
    last_code: u32,
    last_scan_code: u32,
    last_state: Box<[u8; 256]>,
    last_is_dead: bool,
}

impl KeyboardDecoder {
    fn new() -> Self {
        Self {
            last_code: 0,
            last_scan_code: 0,
            last_state: Box::new([0u8; 256]),
            last_is_dead: false,
        }
    }

    // SAFETY: Must be called from within the WH_KEYBOARD_LL hook proc.
    unsafe fn get_name(&mut self, vk_code: u32, scan_code: u32) -> Option<String> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, GetKeyState, GetKeyboardState, VK_CAPITAL, VK_CONTROL, VK_MENU,
            VK_SHIFT,
        };

        let mut state = [0u8; 256];
        unsafe { GetKeyboardState(state.as_mut_ptr()) };

        unsafe {
            let shift = GetAsyncKeyState(VK_SHIFT as i32);
            state[VK_SHIFT as usize] = if (shift & 0x8000_u16 as i16) != 0 {
                0x80
            } else {
                0
            };

            let ctrl = GetAsyncKeyState(VK_CONTROL as i32);
            state[VK_CONTROL as usize] = if (ctrl & 0x8000_u16 as i16) != 0 {
                0x80
            } else {
                0
            };

            let alt = GetAsyncKeyState(VK_MENU as i32);
            state[VK_MENU as usize] = if (alt & 0x8000_u16 as i16) != 0 {
                0x80
            } else {
                0
            };

            let caps = GetKeyState(VK_CAPITAL as i32);
            state[VK_CAPITAL as usize] = if (caps & 0x0001) != 0 { 0x01 } else { 0 };
        }

        unsafe { self.get_name_with_state(vk_code, scan_code, &state) }
    }

    // SAFETY: Must be called from a thread where GetKeyboardLayout is valid.
    unsafe fn get_name_with_state(
        &mut self,
        vk_code: u32,
        scan_code: u32,
        state: &[u8; 256],
    ) -> Option<String> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, ToUnicodeEx};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

        unsafe {
            *self.last_state = *state;

            let fg_window = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg_window, std::ptr::null_mut());

            // SAFETY: GetKeyboardLayout returns the layout for the given thread.
            let layout = GetKeyboardLayout(fg_thread);

            let mut buf = [0u16; 8];
            // SAFETY: ToUnicodeEx writes at most `buf.len()` UTF-16 code units.
            let len = ToUnicodeEx(
                vk_code,
                scan_code,
                self.last_state.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i32 - 1,
                0,
                layout,
            );

            let mut is_dead = false;
            let result = match len {
                0 => None,
                -1 => {
                    is_dead = true;
                    // Clear dead key from the ToUnicodeEx state machine.
                    // SAFETY: same guarantees as above.
                    let mut clear_buf = [0u16; 8];
                    let mut empty_state = [0u8; 256];
                    let mut clear_len = -1i32;
                    let mut retries = 0;
                    while clear_len < 0 && retries < 32 {
                        clear_len = ToUnicodeEx(
                            vk_code,
                            scan_code,
                            empty_state.as_mut_ptr(),
                            clear_buf.as_mut_ptr(),
                            clear_buf.len() as i32,
                            0,
                            layout,
                        );
                        retries += 1;
                    }
                    None
                }
                n if n > 0 => String::from_utf16(&buf[..n as usize]).ok(),
                _ => None,
            };

            // Replay the previous dead key if one was pending, matching rdev behavior.
            if self.last_code != 0 && self.last_is_dead {
                let mut replay_buf = [0u16; 8];
                // SAFETY: same guarantees; replaying dead key through ToUnicodeEx.
                ToUnicodeEx(
                    self.last_code,
                    self.last_scan_code,
                    self.last_state.as_mut_ptr(),
                    replay_buf.as_mut_ptr(),
                    replay_buf.len() as i32,
                    0,
                    layout,
                );
                self.last_code = 0;
            } else {
                self.last_code = vk_code;
                self.last_scan_code = scan_code;
                self.last_is_dead = is_dead;
            }

            result
        }
    }
}

/// Map a Windows virtual-key code to an `rdev::Key`, matching rdev's keycodes.rs table.
/// Unknown VK codes become `Key::Unknown(vk_code as u32)`.
fn vk_to_rdev_key(vk: u16) -> rdev::Key {
    use rdev::Key;
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_windows_hook_listener(
    evaluator: Arc<Mutex<Evaluator>>,
    state: Arc<taurine_core::engine::EngineState>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    pause_notifications_enabled: Arc<std::sync::atomic::AtomicBool>,
    pause_hotkey: Arc<RwLock<hotkey::HotkeySpec>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    pause_audio_enabled: Arc<std::sync::atomic::AtomicBool>,
    audio_tx: tokio::sync::mpsc::Sender<bool>,
    pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
    hook_health: HookHealth,
    supervisor_tx: std::sync::mpsc::Sender<crate::hook::supervisor::WindowsSupervisorEvent>,
) -> crate::hook::supervisor::ListenerHandle {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    hook_health.mark_listener_started();
    info!("Starting supervised Windows hook listener thread");
    let listener_health = hook_health.clone();
    let fallback_tx = supervisor_tx.clone();

    let (thread_id_tx, thread_id_rx) = std::sync::mpsc::channel::<u32>();

    let spawn_result = std::thread::Builder::new()
        .name("tau-hook-listn".to_string())
        .spawn(move || {
            // SAFETY: GetCurrentThreadId() returns the OS thread ID of the calling
            // thread. It always succeeds, has no failure mode, and requires no
            // arguments. The returned DWORD is valid for the lifetime of the thread
            // and is used later by the supervisor to post WM_QUIT during teardown.
            let os_thread_id = unsafe { GetCurrentThreadId() };
            if thread_id_tx.send(os_thread_id).is_err() {
                warn!(
                    "Supervisor dropped the thread-id channel; listener thread is abandoning itself"
                );
                return;
            }

            // SAFETY: GetCurrentThread() returns a pseudo-handle for the current thread
            // which always succeeds. SetThreadPriority boosts the priority of this thread
            // to THREAD_PRIORITY_HIGHEST so that DWM and the OS scheduler prioritize delivering
            // events to the hook, preventing silent hook termination due to timeout under load.
            unsafe {
                let current_thread = windows_sys::Win32::System::Threading::GetCurrentThread();
                if windows_sys::Win32::System::Threading::SetThreadPriority(
                    current_thread,
                    windows_sys::Win32::System::Threading::THREAD_PRIORITY_HIGHEST,
                ) == 0
                {
                    warn!("Failed to boost hook listener thread priority");
                } else {
                    debug!("Successfully boosted hook listener thread priority to HIGHEST");
                }
            }

            let result = catch_unwind(AssertUnwindSafe(|| {
                run_listener_once(
                    evaluator,
                    state,
                    paused,
                    pause_notifications_enabled,
                    pause_hotkey,
                    spinner_style,
                    pause_audio_enabled,
                    audio_tx,
                    pause_transition_tx,
                    Some(listener_health.clone()),
                )
            }));

            let (exit_error, exit_epoch) = match result {
                Ok(Ok(epoch)) => (None, Some(epoch)),
                Ok(Err(error)) => (Some(error), None),
                Err(_) => (Some("Windows hook listener panicked".to_string()), None),
            };

            if let Some(ref error) = exit_error {
                error!(error = %error, "Windows hook listener is exiting");
            } else {
                warn!("Windows hook listener returned unexpectedly without an error");
            }

            let current_epoch = LISTENER_EPOCH.load(Ordering::SeqCst);
            let is_evicted = exit_epoch.is_some_and(|epoch| epoch != current_epoch);
            if is_evicted {
                info!(
                    exit_epoch = exit_epoch.unwrap_or(current_epoch),
                    current_epoch,
                    "Listener was evicted by recovery; suppressing stale exit notification"
                );
                return;
            }

            if let Err(error) = supervisor_tx.send(
                crate::hook::supervisor::WindowsSupervisorEvent::ListenerExited {
                    error: exit_error,
                },
            ) {
                error!(
                    error = %error,
                    "Failed to notify hook supervisor that the listener exited"
                );
            }
        });

    let join = match spawn_result {
        Ok(handle) => handle,
        Err(error) => {
            let message = format!("Failed to spawn Windows hook listener thread: {error}");
            hook_health.mark_listener_exit(Some(message.clone()));
            error!(error = %message, "Unable to spawn Windows hook listener");
            let _ = fallback_tx.send(
                crate::hook::supervisor::WindowsSupervisorEvent::ListenerExited {
                    error: Some(message),
                },
            );
            return crate::hook::supervisor::ListenerHandle {
                join: None,
                thread_id: 0,
            };
        }
    };

    let thread_id = match thread_id_rx.recv() {
        Ok(id) => id,
        Err(_) => {
            // The listener thread was killed by the OS (e.g. an SEH/structured
            // exception during SetWindowsHookEx right after wakeup) before it
            // could send its thread ID. catch_unwind does not catch SEH on
            // Windows, so the thread drops its stack and the sender is gone.
            // Recover gracefully: mark exit and send ListenerExited so the
            // supervisor retries instead of failing to return a join handle.
            error!(
                "Listener thread terminated before sending OS thread ID; \
                 sending ListenerExited for supervisor retry"
            );
            let msg = "thread killed before sending OS thread ID (SEH on wakeup)".to_string();
            hook_health.mark_listener_exit(Some(msg.clone()));
            let _ = fallback_tx.send(
                crate::hook::supervisor::WindowsSupervisorEvent::ListenerExited {
                    error: Some(msg),
                },
            );
            return crate::hook::supervisor::ListenerHandle {
                join: Some(join),
                thread_id: 0,
            };
        }
    };

    crate::hook::supervisor::ListenerHandle {
        join: Some(join),
        thread_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT;

    #[test]
    fn test_keyboard_decoder_get_name_unshifted() {
        let mut decoder = KeyboardDecoder::new();
        let state = [0u8; 256]; // No modifiers pressed
        // 0x41 is 'A'. scan_code 0x1E
        let name = unsafe { decoder.get_name_with_state(0x41, 0x1E, &state) };
        assert_eq!(name.as_deref(), Some("a"));
    }

    #[test]
    fn test_keyboard_decoder_get_name_shifted() {
        let mut decoder = KeyboardDecoder::new();
        let mut state = [0u8; 256];
        state[VK_SHIFT as usize] = 0x80; // Mock Shift key down

        // 0xBB is '='. Shift + '=' is '+'
        let name = unsafe { decoder.get_name_with_state(0xBB, 0x0D, &state) };
        assert_eq!(name.as_deref(), Some("+"));
    }
}
