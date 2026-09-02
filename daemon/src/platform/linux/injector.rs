use crate::platform::{Injector, MouseButton};
use std::thread;
use std::time::Duration;

pub struct LinuxInjector;

impl Injector for LinuxInjector {
    fn simulate_mouse_click(&self, button: MouseButton) {
        let evdev_btn = match button {
            MouseButton::Left => evdev::KeyCode::BTN_LEFT,
            MouseButton::Right => evdev::KeyCode::BTN_RIGHT,
            MouseButton::Middle => evdev::KeyCode::BTN_MIDDLE,
        };
        crate::platform::linux::uinput::simulate_mouse_button(evdev_btn, true);
        thread::sleep(Duration::from_millis(10));
        crate::platform::linux::uinput::simulate_mouse_button(evdev_btn, false);
    }

    fn simulate_mouse_move(&self, x: u16, y: u16) {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        if let Ok((conn, _)) = x11rb::connect(None)
            && let Some(screen) = conn.setup().roots.first()
        {
            let _ = conn.warp_pointer(x11rb::NONE, screen.root, 0, 0, 0, 0, x as i16, y as i16);
            let _ = conn.flush();
        }
    }

    fn simulate_mouse_scroll(&self, delta: i32) {
        crate::platform::linux::uinput::simulate_mouse_scroll(delta);
    }

    fn simulate_mouse_hold(&self, button: MouseButton, hold: bool) {
        let evdev_btn = match button {
            MouseButton::Left => evdev::KeyCode::BTN_LEFT,
            MouseButton::Right => evdev::KeyCode::BTN_RIGHT,
            MouseButton::Middle => evdev::KeyCode::BTN_MIDDLE,
        };
        crate::platform::linux::uinput::simulate_mouse_button(evdev_btn, hold);
    }

    fn simulate_key_alias(&self, alias: &str) -> bool {
        let parts: Vec<&str> = alias.split('+').collect();
        if parts.is_empty() {
            return false;
        }

        let main_key_alias = parts[parts.len() - 1];
        let modifier_aliases = &parts[..parts.len() - 1];

        let main_key = match alias_to_evdev_key(main_key_alias) {
            Some(k) => k,
            None => return false,
        };

        let mut modifiers = Vec::new();
        for m in modifier_aliases {
            match modifier_alias_to_evdev_key(m) {
                Some(k) => modifiers.push(k),
                None => return false,
            }
        }

        for m in &modifiers {
            crate::platform::linux::uinput::simulate_key(*m, true);
        }
        crate::platform::linux::uinput::simulate_keypress(main_key);
        for m in modifiers.iter().rev() {
            crate::platform::linux::uinput::simulate_key(*m, false);
        }
        true
    }

    fn simulate_left(&self, count: usize) {
        for _ in 0..count {
            crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_LEFT);
        }
    }

    fn simulate_right(&self, count: usize) {
        for _ in 0..count {
            crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_RIGHT);
        }
    }

    fn simulate_backspace(&self, count: usize) {
        for _ in 0..count {
            crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_BACKSPACE);
        }
    }

    fn simulate_paste(&self) {
        crate::platform::linux::uinput::simulate_key(evdev::KeyCode::KEY_LEFTCTRL, true);
        crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_V);
        crate::platform::linux::uinput::simulate_key(evdev::KeyCode::KEY_LEFTCTRL, false);
    }

    fn pre_release_modifiers(&self) {
        let modifiers = [
            evdev::KeyCode::KEY_LEFTSHIFT,
            evdev::KeyCode::KEY_RIGHTSHIFT,
            evdev::KeyCode::KEY_LEFTCTRL,
            evdev::KeyCode::KEY_RIGHTCTRL,
            evdev::KeyCode::KEY_LEFTALT,
            evdev::KeyCode::KEY_RIGHTALT,
            evdev::KeyCode::KEY_LEFTMETA,
            evdev::KeyCode::KEY_RIGHTMETA,
        ];
        for key in &modifiers {
            crate::platform::linux::uinput::simulate_key(*key, false);
        }
    }

    fn try_inject_frame_raw(&self, frame: &str) -> bool {
        let Some(c) = frame.chars().next() else {
            return false;
        };
        let Some(lookup) = crate::platform::linux::get_reverse_lookup() else {
            return false;
        };
        let Some(key_info) = lookup.get(&c) else {
            return false;
        };
        crate::platform::linux::uinput::simulate_keypress(key_info.key);
        true
    }

    fn inject_atomic_text_expansion(&self, delete_count: usize, text: &str) -> bool {
        self.inject_atomic_text_expansion_with_nav(delete_count, text, 0, 0)
    }

    fn inject_atomic_text_expansion_with_nav(
        &self,
        delete_count: usize,
        text: &str,
        left_nav: usize,
        right_nav: usize,
    ) -> bool {
        self.simulate_backspace(delete_count);
        let ok = self.inject_unicode_text_direct(text);
        self.simulate_left(left_nav);
        self.simulate_right(right_nav);
        ok
    }

    fn inject_atomic_backspaces(&self, count: usize) {
        for _ in 0..count {
            crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_BACKSPACE);
        }
    }

    fn inject_unicode_text_direct(&self, text: &str) -> bool {
        if let Some(lookup) = crate::platform::linux::get_reverse_lookup() {
            crate::platform::linux::uinput::simulate_type_string(text, lookup);
            true
        } else {
            false
        }
    }

    fn inject_atomic_undo(&self, backspaces: usize, text: &str) -> bool {
        self.simulate_backspace(backspaces);
        self.inject_unicode_text_direct(text)
    }
}

pub(crate) fn modifier_alias_to_evdev_key(alias: &str) -> Option<evdev::KeyCode> {
    use taurine_core::keys::Modifier;
    let modifier = Modifier::from_alias(alias)?;
    match modifier {
        Modifier::Ctrl | Modifier::LeftCtrl => Some(evdev::KeyCode::KEY_LEFTCTRL),
        Modifier::RightCtrl => Some(evdev::KeyCode::KEY_RIGHTCTRL),
        Modifier::Shift | Modifier::LeftShift => Some(evdev::KeyCode::KEY_LEFTSHIFT),
        Modifier::RightShift => Some(evdev::KeyCode::KEY_RIGHTSHIFT),
        Modifier::Alt | Modifier::LeftAlt => Some(evdev::KeyCode::KEY_LEFTALT),
        Modifier::RightAlt => Some(evdev::KeyCode::KEY_RIGHTALT),
        Modifier::Meta | Modifier::LeftMeta => Some(evdev::KeyCode::KEY_LEFTMETA),
        Modifier::RightMeta => Some(evdev::KeyCode::KEY_RIGHTMETA),
    }
}

pub(crate) fn alias_to_evdev_key(alias: &str) -> Option<evdev::KeyCode> {
    use taurine_core::keys::LogicalKey;
    let key = LogicalKey::from_alias(alias)?;
    match key {
        LogicalKey::Letter(c) => match c {
            'a' => Some(evdev::KeyCode::KEY_A),
            'b' => Some(evdev::KeyCode::KEY_B),
            'c' => Some(evdev::KeyCode::KEY_C),
            'd' => Some(evdev::KeyCode::KEY_D),
            'e' => Some(evdev::KeyCode::KEY_E),
            'f' => Some(evdev::KeyCode::KEY_F),
            'g' => Some(evdev::KeyCode::KEY_G),
            'h' => Some(evdev::KeyCode::KEY_H),
            'i' => Some(evdev::KeyCode::KEY_I),
            'j' => Some(evdev::KeyCode::KEY_J),
            'k' => Some(evdev::KeyCode::KEY_K),
            'l' => Some(evdev::KeyCode::KEY_L),
            'm' => Some(evdev::KeyCode::KEY_M),
            'n' => Some(evdev::KeyCode::KEY_N),
            'o' => Some(evdev::KeyCode::KEY_O),
            'p' => Some(evdev::KeyCode::KEY_P),
            'q' => Some(evdev::KeyCode::KEY_Q),
            'r' => Some(evdev::KeyCode::KEY_R),
            's' => Some(evdev::KeyCode::KEY_S),
            't' => Some(evdev::KeyCode::KEY_T),
            'u' => Some(evdev::KeyCode::KEY_U),
            'v' => Some(evdev::KeyCode::KEY_V),
            'w' => Some(evdev::KeyCode::KEY_W),
            'x' => Some(evdev::KeyCode::KEY_X),
            'y' => Some(evdev::KeyCode::KEY_Y),
            'z' => Some(evdev::KeyCode::KEY_Z),
            _ => None,
        },
        LogicalKey::Digit(d) | LogicalKey::NumpadDigit(d) => match d {
            0 => Some(evdev::KeyCode::KEY_0),
            1 => Some(evdev::KeyCode::KEY_1),
            2 => Some(evdev::KeyCode::KEY_2),
            3 => Some(evdev::KeyCode::KEY_3),
            4 => Some(evdev::KeyCode::KEY_4),
            5 => Some(evdev::KeyCode::KEY_5),
            6 => Some(evdev::KeyCode::KEY_6),
            7 => Some(evdev::KeyCode::KEY_7),
            8 => Some(evdev::KeyCode::KEY_8),
            9 => Some(evdev::KeyCode::KEY_9),
            _ => None,
        },
        LogicalKey::Enter => Some(evdev::KeyCode::KEY_ENTER),
        LogicalKey::Escape => Some(evdev::KeyCode::KEY_ESC),
        LogicalKey::Tab => Some(evdev::KeyCode::KEY_TAB),
        LogicalKey::Space => Some(evdev::KeyCode::KEY_SPACE),
        LogicalKey::Backspace => Some(evdev::KeyCode::KEY_BACKSPACE),
        LogicalKey::Delete => Some(evdev::KeyCode::KEY_DELETE),
        LogicalKey::Up => Some(evdev::KeyCode::KEY_UP),
        LogicalKey::Down => Some(evdev::KeyCode::KEY_DOWN),
        LogicalKey::Left => Some(evdev::KeyCode::KEY_LEFT),
        LogicalKey::Right => Some(evdev::KeyCode::KEY_RIGHT),
        LogicalKey::Home => Some(evdev::KeyCode::KEY_HOME),
        LogicalKey::End => Some(evdev::KeyCode::KEY_END),
        LogicalKey::PageUp => Some(evdev::KeyCode::KEY_PAGEUP),
        LogicalKey::PageDown => Some(evdev::KeyCode::KEY_PAGEDOWN),
        LogicalKey::Insert => Some(evdev::KeyCode::KEY_INSERT),
        LogicalKey::Function(n) => match n {
            1 => Some(evdev::KeyCode::KEY_F1),
            2 => Some(evdev::KeyCode::KEY_F2),
            3 => Some(evdev::KeyCode::KEY_F3),
            4 => Some(evdev::KeyCode::KEY_F4),
            5 => Some(evdev::KeyCode::KEY_F5),
            6 => Some(evdev::KeyCode::KEY_F6),
            7 => Some(evdev::KeyCode::KEY_F7),
            8 => Some(evdev::KeyCode::KEY_F8),
            9 => Some(evdev::KeyCode::KEY_F9),
            10 => Some(evdev::KeyCode::KEY_F10),
            11 => Some(evdev::KeyCode::KEY_F11),
            12 => Some(evdev::KeyCode::KEY_F12),
            _ => None,
        },
        LogicalKey::Backquote => Some(evdev::KeyCode::KEY_GRAVE),
        LogicalKey::Minus => Some(evdev::KeyCode::KEY_MINUS),
        LogicalKey::Equal => Some(evdev::KeyCode::KEY_EQUAL),
        LogicalKey::Backslash => Some(evdev::KeyCode::KEY_BACKSLASH),
        LogicalKey::Semicolon => Some(evdev::KeyCode::KEY_SEMICOLON),
        LogicalKey::Quote => Some(evdev::KeyCode::KEY_APOSTROPHE),
        LogicalKey::Comma => Some(evdev::KeyCode::KEY_COMMA),
        LogicalKey::Period => Some(evdev::KeyCode::KEY_DOT),
        LogicalKey::Slash => Some(evdev::KeyCode::KEY_SLASH),
        LogicalKey::LeftBracket => Some(evdev::KeyCode::KEY_LEFTBRACE),
        LogicalKey::RightBracket => Some(evdev::KeyCode::KEY_RIGHTBRACE),
        LogicalKey::CapsLock => Some(evdev::KeyCode::KEY_CAPSLOCK),
        LogicalKey::NumLock => Some(evdev::KeyCode::KEY_NUMLOCK),
        LogicalKey::ScrollLock => Some(evdev::KeyCode::KEY_SCROLLLOCK),
        LogicalKey::PrintScreen => Some(evdev::KeyCode::KEY_SYSRQ),
        LogicalKey::Pause => Some(evdev::KeyCode::KEY_PAUSE),
        LogicalKey::Modifier(m) => match m {
            taurine_core::keys::Modifier::Ctrl | taurine_core::keys::Modifier::LeftCtrl => {
                Some(evdev::KeyCode::KEY_LEFTCTRL)
            }
            taurine_core::keys::Modifier::RightCtrl => Some(evdev::KeyCode::KEY_RIGHTCTRL),
            taurine_core::keys::Modifier::Shift | taurine_core::keys::Modifier::LeftShift => {
                Some(evdev::KeyCode::KEY_LEFTSHIFT)
            }
            taurine_core::keys::Modifier::RightShift => Some(evdev::KeyCode::KEY_RIGHTSHIFT),
            taurine_core::keys::Modifier::Alt | taurine_core::keys::Modifier::LeftAlt => {
                Some(evdev::KeyCode::KEY_LEFTALT)
            }
            taurine_core::keys::Modifier::RightAlt => Some(evdev::KeyCode::KEY_RIGHTALT),
            taurine_core::keys::Modifier::Meta | taurine_core::keys::Modifier::LeftMeta => {
                Some(evdev::KeyCode::KEY_LEFTMETA)
            }
            taurine_core::keys::Modifier::RightMeta => Some(evdev::KeyCode::KEY_RIGHTMETA),
        },
    }
}
