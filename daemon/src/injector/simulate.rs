use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(not(target_os = "linux"))]
use rdev::{EventType, Key, simulate};

use super::gate::IS_SIMULATING;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseButton {
    Left,
    Right,
    Middle,
}

#[cfg(not(target_os = "linux"))]
pub(super) fn simulate_mouse_click(button: MouseButton) {
    let rdev_btn = match button {
        MouseButton::Left => rdev::Button::Left,
        MouseButton::Right => rdev::Button::Right,
        MouseButton::Middle => rdev::Button::Middle,
    };
    let _ = simulate_monitored(&rdev::EventType::ButtonPress(rdev_btn));
    thread::sleep(Duration::from_millis(10));
    let _ = simulate_monitored(&rdev::EventType::ButtonRelease(rdev_btn));
}

#[cfg(not(target_os = "linux"))]
pub(super) fn simulate_mouse_move(x: u16, y: u16) {
    let _ = simulate_monitored(&rdev::EventType::MouseMove {
        x: x as f64,
        y: y as f64,
    });
}

#[cfg(not(target_os = "linux"))]
pub(super) fn simulate_mouse_scroll(delta: i32) {
    let _ = simulate_monitored(&rdev::EventType::Wheel {
        delta_x: 0,
        delta_y: delta as i64,
    });
}

#[cfg(not(target_os = "linux"))]
pub(super) fn simulate_mouse_hold(button: MouseButton, hold: bool) {
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
    let _ = simulate_monitored(&event);
}

#[cfg(target_os = "linux")]
pub(super) fn simulate_mouse_click(button: MouseButton) {
    let evdev_btn = match button {
        MouseButton::Left => evdev::KeyCode::BTN_LEFT,
        MouseButton::Right => evdev::KeyCode::BTN_RIGHT,
        MouseButton::Middle => evdev::KeyCode::BTN_MIDDLE,
    };
    crate::platform::linux::uinput::simulate_mouse_button(evdev_btn, true);
    thread::sleep(Duration::from_millis(10));
    crate::platform::linux::uinput::simulate_mouse_button(evdev_btn, false);
}

#[cfg(target_os = "linux")]
pub(super) fn simulate_mouse_move(x: u16, y: u16) {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;
    if let Ok((conn, _)) = x11rb::connect(None)
        && let Some(screen) = conn.setup().roots.first()
    {
        let _ = conn.warp_pointer(x11rb::NONE, screen.root, 0, 0, 0, 0, x as i16, y as i16);
        let _ = conn.flush();
    }
}

#[cfg(target_os = "linux")]
pub(super) fn simulate_mouse_scroll(delta: i32) {
    crate::platform::linux::uinput::simulate_mouse_scroll(delta);
}

#[cfg(target_os = "linux")]
pub(super) fn simulate_mouse_hold(button: MouseButton, hold: bool) {
    let evdev_btn = match button {
        MouseButton::Left => evdev::KeyCode::BTN_LEFT,
        MouseButton::Right => evdev::KeyCode::BTN_RIGHT,
        MouseButton::Middle => evdev::KeyCode::BTN_MIDDLE,
    };
    crate::platform::linux::uinput::simulate_mouse_button(evdev_btn, hold);
}

#[cfg(not(target_os = "linux"))]
#[derive(Clone)]
pub(super) struct SimulatedEvent {
    pub(super) event: EventType,
    pub(super) queued_at: Instant,
}

#[cfg(not(target_os = "linux"))]
const SIMULATED_EVENT_TTL: Duration = Duration::from_millis(100);

#[cfg(not(target_os = "linux"))]
pub(super) fn simulated_events() -> &'static Mutex<VecDeque<SimulatedEvent>> {
    static Q: OnceLock<Mutex<VecDeque<SimulatedEvent>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(VecDeque::new()))
}

#[cfg(not(target_os = "linux"))]
fn prune_expired_simulated_events(queue: &mut VecDeque<SimulatedEvent>) {
    while queue
        .front()
        .is_some_and(|entry| entry.queued_at.elapsed() > SIMULATED_EVENT_TTL)
    {
        queue.pop_front();
    }
}

#[cfg(not(target_os = "linux"))]
pub fn consume_simulated_event(event: &EventType) -> bool {
    let Ok(mut queue) = simulated_events().lock() else {
        return false;
    };

    prune_expired_simulated_events(&mut queue);

    if queue.front().is_some_and(|entry| entry.event == *event) {
        queue.pop_front();
        true
    } else {
        false
    }
}

/// Wrapped version of `rdev::simulate` that maintains the `IS_SIMULATING` flag.
#[cfg(not(target_os = "linux"))]
pub fn simulate_monitored(event: &EventType) -> Result<(), rdev::SimulateError> {
    if let Ok(mut queue) = simulated_events().lock() {
        prune_expired_simulated_events(&mut queue);
        queue.push_back(SimulatedEvent {
            event: *event,
            queued_at: Instant::now(),
        });
    }

    IS_SIMULATING.store(true, Ordering::SeqCst);
    let res = simulate(event);
    IS_SIMULATING.store(false, Ordering::SeqCst);

    if res.is_err()
        && let Ok(mut queue) = simulated_events().lock()
    {
        prune_expired_simulated_events(&mut queue);
        if queue.front().is_some_and(|entry| entry.event == *event) {
            queue.pop_front();
        }
    }

    res
}

/// **Pre-Release**: Explicitly release all modifier keys before starting an injection
/// sequence. This ensures the OS modifier state is neutral, even if the user is still
/// physically holding a key from the trigger typing process.
#[cfg(not(target_os = "linux"))]
pub(super) fn pre_release_modifiers() {
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
        let _ = simulate_monitored(&EventType::KeyRelease(*key));
    }
}

/// **Pre-Release** for Linux via uinput.
#[cfg(target_os = "linux")]
pub(super) fn pre_release_modifiers() {
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

/// Simulates a key press (optionally with modifier keys held).
///
/// Supports combo syntax via `+` separator: `ctrl+a`, `shift+tab`, `ctrl+shift+end`.
/// Returns `true` if all parts were recognized and simulated.
#[cfg(not(target_os = "linux"))]
pub(super) fn simulate_key_alias(alias: &str) -> bool {
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
        let _ = simulate_monitored(&EventType::KeyPress(*m));
    }
    let _ = simulate_monitored(&EventType::KeyPress(main_key));
    let _ = simulate_monitored(&EventType::KeyRelease(main_key));
    for m in modifiers.iter().rev() {
        let _ = simulate_monitored(&EventType::KeyRelease(*m));
    }
    true
}

/// Simulates a key press (optionally with modifier keys held) on Linux.
#[cfg(target_os = "linux")]
pub(super) fn simulate_key_alias(alias: &str) -> bool {
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

/// Resolves a modifier alias to an rdev Key.
#[cfg(not(target_os = "linux"))]
pub(super) fn modifier_alias_to_rdev_key(alias: &str) -> Option<Key> {
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
        // macOS aliases
        "cmd" | "command" => Some(Key::MetaLeft),
        "opt" | "option" => Some(Key::Alt),
        _ => None,
    }
}

/// Resolves a modifier alias to an evdev KeyCode.
#[cfg(target_os = "linux")]
fn modifier_alias_to_evdev_key(alias: &str) -> Option<evdev::KeyCode> {
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

/// Maps a key alias string to an rdev Key.
#[cfg(not(target_os = "linux"))]
pub(super) fn alias_to_rdev_key(alias: &str) -> Option<Key> {
    match alias {
        // Navigation
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
        // Editing
        "backspace" => Some(Key::Backspace),
        "delete" | "del" => Some(Key::Delete),
        "space" => Some(Key::Space),
        // Alphabets
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
        // Numbers
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
        // Function keys
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
        // Special characters
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
        // Lock keys
        "capslock" => Some(Key::CapsLock),
        "numlock" => Some(Key::NumLock),
        "scrolllock" => Some(Key::ScrollLock),
        "printscreen" | "prtsc" => Some(Key::PrintScreen),
        "pause" | "break" => Some(Key::Pause),
        // Modifiers (as standalone keys)
        "ctrl" | "control" => Some(Key::ControlLeft),
        "alt" => Some(Key::Alt),
        "shift" => Some(Key::ShiftLeft),
        "win" | "mod" | "super" | "meta" => Some(Key::MetaLeft),
        _ => None,
    }
}

/// Maps a key alias string to an evdev KeyCode.
#[cfg(target_os = "linux")]
fn alias_to_evdev_key(alias: &str) -> Option<evdev::KeyCode> {
    match alias {
        // Navigation
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
        // Editing
        "backspace" => Some(evdev::KeyCode::KEY_BACKSPACE),
        "delete" | "del" => Some(evdev::KeyCode::KEY_DELETE),
        "space" => Some(evdev::KeyCode::KEY_SPACE),
        // Alphabets
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
        // Numbers
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
        // Function keys
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
        // Special characters
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
        // Lock keys
        "capslock" => Some(evdev::KeyCode::KEY_CAPSLOCK),
        "numlock" => Some(evdev::KeyCode::KEY_NUMLOCK),
        "scrolllock" => Some(evdev::KeyCode::KEY_SCROLLLOCK),
        "printscreen" | "prtsc" => Some(evdev::KeyCode::KEY_SYSRQ),
        "pause" | "break" => Some(evdev::KeyCode::KEY_PAUSE),
        // Modifiers (as standalone keys)
        "ctrl" | "control" => Some(evdev::KeyCode::KEY_LEFTCTRL),
        "alt" => Some(evdev::KeyCode::KEY_LEFTALT),
        "shift" => Some(evdev::KeyCode::KEY_LEFTSHIFT),
        "win" | "mod" | "super" | "meta" => Some(evdev::KeyCode::KEY_LEFTMETA),
        _ => None,
    }
}
