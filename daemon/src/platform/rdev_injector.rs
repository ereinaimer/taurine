use crate::platform::{Injector, MouseButton};
use rdev::{EventType, Key};
use std::thread;
use std::time::Duration;

pub struct RdevInjector;

impl Injector for RdevInjector {
    fn simulate_mouse_click(&self, button: MouseButton) {
        let rdev_btn = match button {
            MouseButton::Left => rdev::Button::Left,
            MouseButton::Right => rdev::Button::Right,
            MouseButton::Middle => rdev::Button::Middle,
        };
        let _ = crate::injector::simulate_monitored(&rdev::EventType::ButtonPress(rdev_btn));
        thread::sleep(Duration::from_millis(10));
        let _ = crate::injector::simulate_monitored(&rdev::EventType::ButtonRelease(rdev_btn));
    }

    fn simulate_mouse_move(&self, x: u16, y: u16) {
        let _ = crate::injector::simulate_monitored(&rdev::EventType::MouseMove {
            x: x as f64,
            y: y as f64,
        });
    }

    fn simulate_mouse_scroll(&self, delta: i32) {
        let _ = crate::injector::simulate_monitored(&rdev::EventType::Wheel {
            delta_x: 0,
            delta_y: delta as i64,
        });
    }

    fn simulate_mouse_hold(&self, button: MouseButton, hold: bool) {
        let rdev_btn = match button {
            MouseButton::Left => rdev::Button::Left,
            MouseButton::Right => rdev::Button::Right,
            MouseButton::Middle => rdev::Button::Middle,
        };
        let event = if hold {
            rdev::EventType::ButtonPress(rdev_btn)
        } else {
            rdev::EventType::ButtonRelease(rdev_btn)
        };
        let _ = crate::injector::simulate_monitored(&event);
    }

    fn simulate_key_alias(&self, alias: &str) -> bool {
        let parts: Vec<&str> = alias.split('+').collect();
        if parts.is_empty() {
            return false;
        }

        // Last part is the main key; everything before it is a modifier.
        let main_key_alias = parts[parts.len() - 1];
        let modifier_aliases = &parts[..parts.len() - 1];

        let main_key = match alias_to_rdev_key(main_key_alias) {
            Some(k) => k,
            None => return false,
        };

        // Resolve modifier keys.
        let mut modifiers = Vec::new();
        for m in modifier_aliases {
            match modifier_alias_to_rdev_key(m) {
                Some(k) => modifiers.push(k),
                None => return false,
            }
        }

        // Press modifiers → press key → release key → release modifiers (reverse).
        for m in &modifiers {
            let _ = crate::injector::simulate_monitored(&EventType::KeyPress(*m));
        }
        let _ = crate::injector::simulate_monitored(&EventType::KeyPress(main_key));
        let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(main_key));
        for m in modifiers.iter().rev() {
            let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(*m));
        }
        true
    }

    fn simulate_left(&self, count: usize) {
        for _ in 0..count {
            let _ = crate::injector::simulate_monitored(&EventType::KeyPress(Key::LeftArrow));
            let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(Key::LeftArrow));
        }
    }

    fn simulate_right(&self, count: usize) {
        for _ in 0..count {
            let _ = crate::injector::simulate_monitored(&EventType::KeyPress(Key::RightArrow));
            let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(Key::RightArrow));
        }
    }

    fn simulate_backspace(&self, count: usize) {
        for _ in 0..count {
            let _ = crate::injector::simulate_monitored(&EventType::KeyPress(Key::Backspace));
            let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(Key::Backspace));
        }
    }

    fn simulate_paste(&self) {
        let modifier = if cfg!(target_os = "macos") {
            rdev::Key::MetaLeft
        } else {
            rdev::Key::ControlLeft
        };
        let _ = crate::injector::simulate_monitored(&rdev::EventType::KeyPress(modifier));
        let _ = crate::injector::simulate_monitored(&rdev::EventType::KeyPress(rdev::Key::KeyV));
        let _ = crate::injector::simulate_monitored(&rdev::EventType::KeyRelease(rdev::Key::KeyV));
        let _ = crate::injector::simulate_monitored(&rdev::EventType::KeyRelease(modifier));
    }

    fn pre_release_modifiers(&self) {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
                VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
            };
            // SAFETY: GetAsyncKeyState is a thread-safe Win32 API that reads the asynchronous
            // keystate bitmap from kernel memory with no failure modes.
            unsafe {
                if (GetAsyncKeyState(VK_SHIFT as i32) as u16 & 0x8000) != 0 {
                    let l = (GetAsyncKeyState(VK_LSHIFT as i32) as u16 & 0x8000) != 0;
                    let r = (GetAsyncKeyState(VK_RSHIFT as i32) as u16 & 0x8000) != 0;
                    if l || !r {
                        let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(
                            Key::ShiftLeft,
                        ));
                    }
                    if r {
                        let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(
                            Key::ShiftRight,
                        ));
                    }
                }

                if (GetAsyncKeyState(VK_CONTROL as i32) as u16 & 0x8000) != 0 {
                    let l = (GetAsyncKeyState(VK_LCONTROL as i32) as u16 & 0x8000) != 0;
                    let r = (GetAsyncKeyState(VK_RCONTROL as i32) as u16 & 0x8000) != 0;
                    if l || !r {
                        let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(
                            Key::ControlLeft,
                        ));
                    }
                    if r {
                        let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(
                            Key::ControlRight,
                        ));
                    }
                }

                if (GetAsyncKeyState(VK_MENU as i32) as u16 & 0x8000) != 0 {
                    let l = (GetAsyncKeyState(VK_LMENU as i32) as u16 & 0x8000) != 0;
                    let r = (GetAsyncKeyState(VK_RMENU as i32) as u16 & 0x8000) != 0;
                    if l || !r {
                        let _ =
                            crate::injector::simulate_monitored(&EventType::KeyRelease(Key::Alt));
                    }
                    if r {
                        let _ =
                            crate::injector::simulate_monitored(&EventType::KeyRelease(Key::AltGr));
                    }
                }

                if (GetAsyncKeyState(VK_LWIN as i32) as u16 & 0x8000) != 0 {
                    let _ =
                        crate::injector::simulate_monitored(&EventType::KeyRelease(Key::MetaLeft));
                }

                if (GetAsyncKeyState(VK_RWIN as i32) as u16 & 0x8000) != 0 {
                    let _ =
                        crate::injector::simulate_monitored(&EventType::KeyRelease(Key::MetaRight));
                }
            }
        }
        #[cfg(not(windows))]
        {
            let modifiers = [
                Key::ShiftLeft,
                Key::ShiftRight,
                Key::ControlLeft,
                Key::ControlRight,
                Key::Alt,
                Key::AltGr,
                Key::MetaLeft,
                Key::MetaRight,
            ];
            for key in &modifiers {
                let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(*key));
            }
        }
    }

    fn try_inject_frame_raw(&self, _frame: &str) -> bool {
        false
    }

    fn inject_atomic_text_expansion(&self, delete_count: usize, text: &str) -> bool {
        #[cfg(windows)]
        {
            let utf16_units: Vec<u16> = text.encode_utf16().collect();
            let mut inputs = Vec::with_capacity((delete_count + utf16_units.len()) * 2);
            for _ in 0..delete_count {
                inputs.push(make_backspace_input(false));
                inputs.push(make_backspace_input(true));
            }
            for unit in utf16_units {
                inputs.push(make_unicode_input(unit, false));
                inputs.push(make_unicode_input(unit, true));
            }
            if send_inputs_batch(&inputs) {
                return true;
            }
            // Fallback if SendInput was blocked by OS (e.g. UIPI or headless session)
            self.simulate_backspace(delete_count);
            self.inject_unicode_text_direct(text)
        }
        #[cfg(not(windows))]
        {
            self.simulate_backspace(delete_count);
            self.inject_unicode_text_direct(text)
        }
    }

    fn inject_atomic_backspaces(&self, count: usize) {
        if count == 0 {
            return;
        }
        #[cfg(windows)]
        {
            let mut inputs = Vec::with_capacity(count * 2);
            for _ in 0..count {
                inputs.push(make_backspace_input(false));
                inputs.push(make_backspace_input(true));
            }
            if !send_inputs_batch(&inputs) {
                self.simulate_backspace(count);
            }
        }
        #[cfg(not(windows))]
        {
            self.simulate_backspace(count);
        }
    }

    fn inject_unicode_text_direct(&self, text: &str) -> bool {
        if text.is_empty() {
            return true;
        }
        #[cfg(windows)]
        {
            let utf16_units: Vec<u16> = text.encode_utf16().collect();
            let mut inputs = Vec::with_capacity(utf16_units.len() * 2);
            for unit in utf16_units {
                inputs.push(make_unicode_input(unit, false));
                inputs.push(make_unicode_input(unit, true));
            }
            if send_inputs_batch(&inputs) {
                return true;
            }
            crate::injector::inject_text_segment(text, &None).success
        }
        #[cfg(not(windows))]
        {
            crate::injector::inject_text_segment(text, &None).success
        }
    }
}

#[cfg(windows)]
fn make_backspace_input(key_up: bool) -> windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_BACK,
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_BACK,
                wScan: 0,
                dwFlags: if key_up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(windows)]
fn make_unicode_input(
    code_unit: u16,
    key_up: bool,
) -> windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: code_unit,
                dwFlags: if key_up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(windows)]
fn send_inputs_batch(inputs: &[windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT]) -> bool {
    if inputs.is_empty() {
        return true;
    }
    // SAFETY: SendInput is a standard Win32 call. `inputs` is an initialized contiguous slice
    // of INPUT structures whose lifetime is valid for the duration of the call.
    // `size_of::<INPUT>()` matches the layout expected by Windows.
    unsafe {
        let count = inputs.len() as u32;
        let sent = windows_sys::Win32::UI::Input::KeyboardAndMouse::SendInput(
            count,
            inputs.as_ptr(),
            std::mem::size_of::<windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT>() as i32,
        );
        sent == count
    }
}

pub(crate) fn modifier_alias_to_rdev_key(alias: &str) -> Option<Key> {
    use taurine_core::keys::Modifier;
    let modifier = Modifier::from_alias(alias)?;
    match modifier {
        Modifier::Ctrl | Modifier::LeftCtrl => Some(Key::ControlLeft),
        Modifier::RightCtrl => Some(Key::ControlRight),
        Modifier::Shift | Modifier::LeftShift => Some(Key::ShiftLeft),
        Modifier::RightShift => Some(Key::ShiftRight),
        Modifier::Alt | Modifier::LeftAlt => Some(Key::Alt),
        Modifier::RightAlt => Some(Key::AltGr),
        Modifier::Meta | Modifier::LeftMeta => Some(Key::MetaLeft),
        Modifier::RightMeta => Some(Key::MetaRight),
    }
}

pub(crate) fn alias_to_rdev_key(alias: &str) -> Option<Key> {
    use taurine_core::keys::LogicalKey;
    let key = LogicalKey::from_alias(alias)?;
    match key {
        LogicalKey::Letter(c) => match c {
            'a' => Some(Key::KeyA),
            'b' => Some(Key::KeyB),
            'c' => Some(Key::KeyC),
            'd' => Some(Key::KeyD),
            'e' => Some(Key::KeyE),
            'f' => Some(Key::KeyF),
            'g' => Some(Key::KeyG),
            'h' => Some(Key::KeyH),
            'i' => Some(Key::KeyI),
            'j' => Some(Key::KeyJ),
            'k' => Some(Key::KeyK),
            'l' => Some(Key::KeyL),
            'm' => Some(Key::KeyM),
            'n' => Some(Key::KeyN),
            'o' => Some(Key::KeyO),
            'p' => Some(Key::KeyP),
            'q' => Some(Key::KeyQ),
            'r' => Some(Key::KeyR),
            's' => Some(Key::KeyS),
            't' => Some(Key::KeyT),
            'u' => Some(Key::KeyU),
            'v' => Some(Key::KeyV),
            'w' => Some(Key::KeyW),
            'x' => Some(Key::KeyX),
            'y' => Some(Key::KeyY),
            'z' => Some(Key::KeyZ),
            _ => None,
        },
        LogicalKey::Digit(d) | LogicalKey::NumpadDigit(d) => match d {
            0 => Some(Key::Num0),
            1 => Some(Key::Num1),
            2 => Some(Key::Num2),
            3 => Some(Key::Num3),
            4 => Some(Key::Num4),
            5 => Some(Key::Num5),
            6 => Some(Key::Num6),
            7 => Some(Key::Num7),
            8 => Some(Key::Num8),
            9 => Some(Key::Num9),
            _ => None,
        },
        LogicalKey::Enter => Some(Key::Return),
        LogicalKey::Escape => Some(Key::Escape),
        LogicalKey::Tab => Some(Key::Tab),
        LogicalKey::Space => Some(Key::Space),
        LogicalKey::Backspace => Some(Key::Backspace),
        LogicalKey::Delete => Some(Key::Delete),
        LogicalKey::Up => Some(Key::UpArrow),
        LogicalKey::Down => Some(Key::DownArrow),
        LogicalKey::Left => Some(Key::LeftArrow),
        LogicalKey::Right => Some(Key::RightArrow),
        LogicalKey::Home => Some(Key::Home),
        LogicalKey::End => Some(Key::End),
        LogicalKey::PageUp => Some(Key::PageUp),
        LogicalKey::PageDown => Some(Key::PageDown),
        LogicalKey::Insert => Some(Key::Insert),
        LogicalKey::Function(n) => match n {
            1 => Some(Key::F1),
            2 => Some(Key::F2),
            3 => Some(Key::F3),
            4 => Some(Key::F4),
            5 => Some(Key::F5),
            6 => Some(Key::F6),
            7 => Some(Key::F7),
            8 => Some(Key::F8),
            9 => Some(Key::F9),
            10 => Some(Key::F10),
            11 => Some(Key::F11),
            12 => Some(Key::F12),
            _ => None,
        },
        LogicalKey::Backquote => Some(Key::BackQuote),
        LogicalKey::Minus => Some(Key::Minus),
        LogicalKey::Equal => Some(Key::Equal),
        LogicalKey::Backslash => Some(Key::BackSlash),
        LogicalKey::Semicolon => Some(Key::SemiColon),
        LogicalKey::Quote => Some(Key::Quote),
        LogicalKey::Comma => Some(Key::Comma),
        LogicalKey::Period => Some(Key::Dot),
        LogicalKey::Slash => Some(Key::Slash),
        LogicalKey::LeftBracket => Some(Key::LeftBracket),
        LogicalKey::RightBracket => Some(Key::RightBracket),
        LogicalKey::CapsLock => Some(Key::CapsLock),
        LogicalKey::NumLock => Some(Key::NumLock),
        LogicalKey::ScrollLock => Some(Key::ScrollLock),
        LogicalKey::PrintScreen => Some(Key::PrintScreen),
        LogicalKey::Pause => Some(Key::Pause),
        LogicalKey::Modifier(m) => match m {
            taurine_core::keys::Modifier::Ctrl | taurine_core::keys::Modifier::LeftCtrl => {
                Some(Key::ControlLeft)
            }
            taurine_core::keys::Modifier::RightCtrl => Some(Key::ControlRight),
            taurine_core::keys::Modifier::Shift | taurine_core::keys::Modifier::LeftShift => {
                Some(Key::ShiftLeft)
            }
            taurine_core::keys::Modifier::RightShift => Some(Key::ShiftRight),
            taurine_core::keys::Modifier::Alt | taurine_core::keys::Modifier::LeftAlt => {
                Some(Key::Alt)
            }
            taurine_core::keys::Modifier::RightAlt => Some(Key::AltGr),
            taurine_core::keys::Modifier::Meta | taurine_core::keys::Modifier::LeftMeta => {
                Some(Key::MetaLeft)
            }
            taurine_core::keys::Modifier::RightMeta => Some(Key::MetaRight),
        },
    }
}
