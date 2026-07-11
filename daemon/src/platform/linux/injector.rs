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
        if frame.chars().count() == 1 && lookup.contains_key(&c) {
            crate::platform::linux::uinput::simulate_type_string(frame, lookup);
            true
        } else {
            false
        }
    }
}

pub(crate) fn modifier_alias_to_evdev_key(alias: &str) -> Option<evdev::KeyCode> {
    match alias {
        "ctrl" | "control" => Some(evdev::KeyCode::KEY_LEFTCTRL),
        "lctrl" | "leftctrl" | "leftcontrol" => Some(evdev::KeyCode::KEY_LEFTCTRL),
        "rctrl" | "rightctrl" | "rightcontrol" => Some(evdev::KeyCode::KEY_RIGHTCTRL),
        "alt" => Some(evdev::KeyCode::KEY_LEFTALT),
        "lalt" | "leftalt" | "leftoption" => Some(evdev::KeyCode::KEY_LEFTALT),
        "ralt" | "rightalt" | "rightoption" | "altgr" => Some(evdev::KeyCode::KEY_RIGHTALT),
        "shift" => Some(evdev::KeyCode::KEY_LEFTSHIFT),
        "lshift" | "leftshift" => Some(evdev::KeyCode::KEY_LEFTSHIFT),
        "rshift" | "rightshift" => Some(evdev::KeyCode::KEY_RIGHTSHIFT),
        "win" | "mod" | "super" | "meta" => Some(evdev::KeyCode::KEY_LEFTMETA),
        "lmeta" | "leftmeta" | "lwin" | "leftwin" | "leftsuper" | "leftcmd" | "leftcommand" => {
            Some(evdev::KeyCode::KEY_LEFTMETA)
        }
        "rmeta" | "rightmeta" | "rwin" | "rightwin" | "rightsuper" | "rightcmd"
        | "rightcommand" => Some(evdev::KeyCode::KEY_RIGHTMETA),
        "cmd" | "command" => Some(evdev::KeyCode::KEY_LEFTMETA),
        "opt" | "option" => Some(evdev::KeyCode::KEY_LEFTALT),
        _ => None,
    }
}

pub(crate) fn alias_to_evdev_key(alias: &str) -> Option<evdev::KeyCode> {
    match alias {
        "tab" => Some(evdev::KeyCode::KEY_TAB),
        "enter" | "return" => Some(evdev::KeyCode::KEY_ENTER),
        "esc" | "escape" => Some(evdev::KeyCode::KEY_ESC),
        "up" => Some(evdev::KeyCode::KEY_UP),
        "down" => Some(evdev::KeyCode::KEY_DOWN),
        "left" => Some(evdev::KeyCode::KEY_LEFT),
        "right" => Some(evdev::KeyCode::KEY_RIGHT),
        "home" => Some(evdev::KeyCode::KEY_HOME),
        "end" => Some(evdev::KeyCode::KEY_END),
        "pgup" | "pageup" => Some(evdev::KeyCode::KEY_PAGEUP),
        "pgdown" | "pagedown" => Some(evdev::KeyCode::KEY_PAGEDOWN),
        "insert" | "ins" => Some(evdev::KeyCode::KEY_INSERT),
        "backspace" => Some(evdev::KeyCode::KEY_BACKSPACE),
        "delete" | "del" => Some(evdev::KeyCode::KEY_DELETE),
        "space" => Some(evdev::KeyCode::KEY_SPACE),
        "a" => Some(evdev::KeyCode::KEY_A),
        "b" => Some(evdev::KeyCode::KEY_B),
        "c" => Some(evdev::KeyCode::KEY_C),
        "d" => Some(evdev::KeyCode::KEY_D),
        "e" => Some(evdev::KeyCode::KEY_E),
        "f" => Some(evdev::KeyCode::KEY_F),
        "g" => Some(evdev::KeyCode::KEY_G),
        "h" => Some(evdev::KeyCode::KEY_H),
        "i" => Some(evdev::KeyCode::KEY_I),
        "j" => Some(evdev::KeyCode::KEY_J),
        "k" => Some(evdev::KeyCode::KEY_K),
        "l" => Some(evdev::KeyCode::KEY_L),
        "m" => Some(evdev::KeyCode::KEY_M),
        "n" => Some(evdev::KeyCode::KEY_N),
        "o" => Some(evdev::KeyCode::KEY_O),
        "p" => Some(evdev::KeyCode::KEY_P),
        "q" => Some(evdev::KeyCode::KEY_Q),
        "r" => Some(evdev::KeyCode::KEY_R),
        "s" => Some(evdev::KeyCode::KEY_S),
        "t" => Some(evdev::KeyCode::KEY_T),
        "u" => Some(evdev::KeyCode::KEY_U),
        "v" => Some(evdev::KeyCode::KEY_V),
        "w" => Some(evdev::KeyCode::KEY_W),
        "x" => Some(evdev::KeyCode::KEY_X),
        "y" => Some(evdev::KeyCode::KEY_Y),
        "z" => Some(evdev::KeyCode::KEY_Z),
        "0" => Some(evdev::KeyCode::KEY_0),
        "1" => Some(evdev::KeyCode::KEY_1),
        "2" => Some(evdev::KeyCode::KEY_2),
        "3" => Some(evdev::KeyCode::KEY_3),
        "4" => Some(evdev::KeyCode::KEY_4),
        "5" => Some(evdev::KeyCode::KEY_5),
        "6" => Some(evdev::KeyCode::KEY_6),
        "7" => Some(evdev::KeyCode::KEY_7),
        "8" => Some(evdev::KeyCode::KEY_8),
        "9" => Some(evdev::KeyCode::KEY_9),
        "f1" => Some(evdev::KeyCode::KEY_F1),
        "f2" => Some(evdev::KeyCode::KEY_F2),
        "f3" => Some(evdev::KeyCode::KEY_F3),
        "f4" => Some(evdev::KeyCode::KEY_F4),
        "f5" => Some(evdev::KeyCode::KEY_F5),
        "f6" => Some(evdev::KeyCode::KEY_F6),
        "f7" => Some(evdev::KeyCode::KEY_F7),
        "f8" => Some(evdev::KeyCode::KEY_F8),
        "f9" => Some(evdev::KeyCode::KEY_F9),
        "f10" => Some(evdev::KeyCode::KEY_F10),
        "f11" => Some(evdev::KeyCode::KEY_F11),
        "f12" => Some(evdev::KeyCode::KEY_F12),
        "backtick" | "grave" => Some(evdev::KeyCode::KEY_GRAVE),
        "tilde" => Some(evdev::KeyCode::KEY_GRAVE),
        "minus" | "dash" => Some(evdev::KeyCode::KEY_MINUS),
        "equal" | "equals" => Some(evdev::KeyCode::KEY_EQUAL),
        "backslash" => Some(evdev::KeyCode::KEY_BACKSLASH),
        "semicolon" => Some(evdev::KeyCode::KEY_SEMICOLON),
        "quote" | "apostrophe" => Some(evdev::KeyCode::KEY_APOSTROPHE),
        "comma" => Some(evdev::KeyCode::KEY_COMMA),
        "dot" | "period" => Some(evdev::KeyCode::KEY_DOT),
        "slash" => Some(evdev::KeyCode::KEY_SLASH),
        "lbracket" | "leftbracket" => Some(evdev::KeyCode::KEY_LEFTBRACE),
        "rbracket" | "rightbracket" => Some(evdev::KeyCode::KEY_RIGHTBRACE),
        "capslock" => Some(evdev::KeyCode::KEY_CAPSLOCK),
        "numlock" => Some(evdev::KeyCode::KEY_NUMLOCK),
        "scrolllock" => Some(evdev::KeyCode::KEY_SCROLLLOCK),
        "printscreen" | "prtsc" => Some(evdev::KeyCode::KEY_SYSRQ),
        "pause" | "break" => Some(evdev::KeyCode::KEY_PAUSE),
        "ctrl" | "control" => Some(evdev::KeyCode::KEY_LEFTCTRL),
        "alt" => Some(evdev::KeyCode::KEY_LEFTALT),
        "shift" => Some(evdev::KeyCode::KEY_LEFTSHIFT),
        "win" | "mod" | "super" | "meta" => Some(evdev::KeyCode::KEY_LEFTMETA),
        _ => None,
    }
}
