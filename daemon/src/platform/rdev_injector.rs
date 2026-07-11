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

    fn try_inject_frame_raw(&self, _frame: &str) -> bool {
        false
    }
}

pub(crate) fn modifier_alias_to_rdev_key(alias: &str) -> Option<Key> {
    match alias {
        "ctrl" | "control" => Some(Key::ControlLeft),
        "lctrl" | "leftctrl" | "leftcontrol" => Some(Key::ControlLeft),
        "rctrl" | "rightctrl" | "rightcontrol" => Some(Key::ControlRight),
        "alt" => Some(Key::Alt),
        "lalt" | "leftalt" | "leftoption" => Some(Key::Alt),
        "ralt" | "rightalt" | "rightoption" | "altgr" => Some(Key::AltGr),
        "shift" => Some(Key::ShiftLeft),
        "lshift" | "leftshift" => Some(Key::ShiftLeft),
        "rshift" | "rightshift" => Some(Key::ShiftRight),
        "win" | "mod" | "super" | "meta" => Some(Key::MetaLeft),
        "lmeta" | "leftmeta" | "lwin" | "leftwin" | "leftsuper" | "leftcmd" | "leftcommand" => {
            Some(Key::MetaLeft)
        }
        "rmeta" | "rightmeta" | "rwin" | "rightwin" | "rightsuper" | "rightcmd"
        | "rightcommand" => Some(Key::MetaRight),
        "cmd" | "command" => Some(Key::MetaLeft),
        "opt" | "option" => Some(Key::Alt),
        _ => None,
    }
}

pub(crate) fn alias_to_rdev_key(alias: &str) -> Option<Key> {
    match alias {
        "tab" => Some(Key::Tab),
        "enter" | "return" => Some(Key::Return),
        "esc" | "escape" => Some(Key::Escape),
        "up" => Some(Key::UpArrow),
        "down" => Some(Key::DownArrow),
        "left" => Some(Key::LeftArrow),
        "right" => Some(Key::RightArrow),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pgup" | "pageup" => Some(Key::PageUp),
        "pgdown" | "pagedown" => Some(Key::PageDown),
        "insert" | "ins" => Some(Key::Insert),
        "backspace" => Some(Key::Backspace),
        "delete" | "del" => Some(Key::Delete),
        "space" => Some(Key::Space),
        "a" => Some(Key::KeyA),
        "b" => Some(Key::KeyB),
        "c" => Some(Key::KeyC),
        "d" => Some(Key::KeyD),
        "e" => Some(Key::KeyE),
        "f" => Some(Key::KeyF),
        "g" => Some(Key::KeyG),
        "h" => Some(Key::KeyH),
        "i" => Some(Key::KeyI),
        "j" => Some(Key::KeyJ),
        "k" => Some(Key::KeyK),
        "l" => Some(Key::KeyL),
        "m" => Some(Key::KeyM),
        "n" => Some(Key::KeyN),
        "o" => Some(Key::KeyO),
        "p" => Some(Key::KeyP),
        "q" => Some(Key::KeyQ),
        "r" => Some(Key::KeyR),
        "s" => Some(Key::KeyS),
        "t" => Some(Key::KeyT),
        "u" => Some(Key::KeyU),
        "v" => Some(Key::KeyV),
        "w" => Some(Key::KeyW),
        "x" => Some(Key::KeyX),
        "y" => Some(Key::KeyY),
        "z" => Some(Key::KeyZ),
        "0" => Some(Key::Num0),
        "1" => Some(Key::Num1),
        "2" => Some(Key::Num2),
        "3" => Some(Key::Num3),
        "4" => Some(Key::Num4),
        "5" => Some(Key::Num5),
        "6" => Some(Key::Num6),
        "7" => Some(Key::Num7),
        "8" => Some(Key::Num8),
        "9" => Some(Key::Num9),
        "f1" => Some(Key::F1),
        "f2" => Some(Key::F2),
        "f3" => Some(Key::F3),
        "f4" => Some(Key::F4),
        "f5" => Some(Key::F5),
        "f6" => Some(Key::F6),
        "f7" => Some(Key::F7),
        "f8" => Some(Key::F8),
        "f9" => Some(Key::F9),
        "f10" => Some(Key::F10),
        "f11" => Some(Key::F11),
        "f12" => Some(Key::F12),
        "backtick" | "grave" => Some(Key::BackQuote),
        "tilde" => Some(Key::BackQuote),
        "minus" | "dash" => Some(Key::Minus),
        "equal" | "equals" => Some(Key::Equal),
        "backslash" => Some(Key::BackSlash),
        "semicolon" => Some(Key::SemiColon),
        "quote" | "apostrophe" => Some(Key::Quote),
        "comma" => Some(Key::Comma),
        "dot" | "period" => Some(Key::Dot),
        "slash" => Some(Key::Slash),
        "lbracket" | "leftbracket" => Some(Key::LeftBracket),
        "rbracket" | "rightbracket" => Some(Key::RightBracket),
        "capslock" => Some(Key::CapsLock),
        "numlock" => Some(Key::NumLock),
        "scrolllock" => Some(Key::ScrollLock),
        "printscreen" | "prtsc" => Some(Key::PrintScreen),
        "pause" | "break" => Some(Key::Pause),
        "ctrl" | "control" => Some(Key::ControlLeft),
        "alt" => Some(Key::Alt),
        "shift" => Some(Key::ShiftLeft),
        "win" | "mod" | "super" | "meta" => Some(Key::MetaLeft),
        _ => None,
    }
}
