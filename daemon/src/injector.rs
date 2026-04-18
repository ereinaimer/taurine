#[cfg(any(windows, target_os = "linux"))]
use crate::platform::ClipboardManager;
use arboard::Clipboard;
#[cfg(not(target_os = "linux"))]
use rdev::{EventType, Key, simulate};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;
#[cfg(not(target_os = "linux"))]
use tracing::warn;
use tracing::{debug, error};

use taurine_core::engine::shell::ScriptBehavior;
use taurine_core::engine::variables::ExpansionStep;

/// Abstraction so clipboard ordering (read original → set payload → verify → restore) can be
/// unit-tested without the OS clipboard or `simulate()`.
impl crate::platform::ClipboardManager for Clipboard {
    fn get_text(&mut self) -> Result<String, String> {
        Ok(self.get_text().unwrap_or_default())
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        Clipboard::set_text(self, text).map_err(|e| e.to_string())
    }
}

/// Reads the user's current clipboard, writes `payload`, waits, then verifies the clipboard
/// still equals `payload`. Returns the original text for restore after paste.
///
/// If verification fails, the caller must not simulate paste (avoids injecting stale clipboard).
fn prepare_clipboard_for_expansion(
    clipboard: &mut impl crate::platform::ClipboardManager,
    payload: &str,
) -> Result<String, String> {
    let original = clipboard.get_text()?;
    clipboard.set_text(payload)?;

    // Same delay as production: OS listeners may not see the write immediately.
    thread::sleep(Duration::from_millis(25));

    match clipboard.get_text() {
        Ok(ref actual) if actual == payload => Ok(original),
        Ok(actual) => Err(format!(
            "clipboard verify failed: expected {:?}, got {:?}",
            payload, actual
        )),
        Err(e) => Err(e),
    }
}

/// Serializes clipboard set / paste / restore across overlapping injections. Without this,
/// a second expansion can overwrite the clipboard before the first paste is processed, so the
/// target app pastes the wrong payload or the restored clipboard (stale paste).
fn inject_mutex() -> &'static Mutex<()> {
    static M: OnceLock<Mutex<()>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(()))
}

/// Set to `true` by the hook thread the moment an expansion is dispatched, and
/// cleared here at the end of injection. This ensures all synthetic keystrokes
/// (backspaces, Ctrl+V) are invisible to the evaluator with zero race window.
pub static IS_INJECTING: AtomicBool = AtomicBool::new(false);

/// Set to `true` by the hook thread when a physical keypress is detected during
/// an active injection. The injection loop checks this between steps
/// and aborts early if set, restoring the clipboard and releasing modifiers.
pub static INJECTION_ABORT: AtomicBool = AtomicBool::new(false);

/// Set to `true` momentarily while we are simulating a keystroke. The hook thread
/// checks this to distinguish physical from synthetic keyboard events.
#[allow(dead_code)]
pub static IS_SIMULATING: AtomicBool = AtomicBool::new(false);

/// Wrapped version of `rdev::simulate` that maintains the `IS_SIMULATING` flag.
#[cfg(not(target_os = "linux"))]
pub fn simulate_monitored(event: &EventType) -> Result<(), rdev::SimulateError> {
    IS_SIMULATING.store(true, Ordering::SeqCst);
    let res = simulate(event);
    IS_SIMULATING.store(false, Ordering::SeqCst);
    res
}

/// Request an abort of the currently running injection.
///
/// Safe to call from any thread. The injection loop polls this flag between
/// steps and will stop, restore the clipboard, and do a Panic Release.
pub fn abort_injection() {
    INJECTION_ABORT.store(true, Ordering::SeqCst);
}

/// Implicit inter-step delay (ms) for OS reliability between sequential actions.
#[cfg(target_os = "linux")]
const INTER_STEP_DELAY_MS: u64 = 15;
#[cfg(not(target_os = "linux"))]
const INTER_STEP_DELAY_MS: u64 = 10;

/// Sends n Backspace keystrokes with inter-key sleeps so the OS registers
/// each one individually.
fn erase_trigger(delete_count: usize) {
    debug!("Injecting {} backspaces", delete_count);
    for _ in 0..delete_count {
        #[cfg(target_os = "linux")]
        {
            crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_BACKSPACE);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = simulate_monitored(&EventType::KeyPress(Key::Backspace));
            let _ = simulate_monitored(&EventType::KeyRelease(Key::Backspace));
        }
        thread::sleep(Duration::from_millis(3));
    }
}

fn simulate_paste() {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::uinput::simulate_key(evdev::KeyCode::KEY_LEFTCTRL, true);
        crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_V);
        crate::platform::linux::uinput::simulate_key(evdev::KeyCode::KEY_LEFTCTRL, false);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let modifier = if cfg!(target_os = "macos") {
            Key::MetaLeft
        } else {
            Key::ControlLeft
        };
        let _ = simulate_monitored(&EventType::KeyPress(modifier));
        let _ = simulate_monitored(&EventType::KeyPress(Key::KeyV));
        let _ = simulate_monitored(&EventType::KeyRelease(Key::KeyV));
        let _ = simulate_monitored(&EventType::KeyRelease(modifier));
    }
}

/// **Pre-Release**: Explicitly release all modifier keys before starting an injection
/// sequence. This ensures the OS modifier state is neutral, even if the user is still
/// physically holding a key from the trigger typing process.
#[cfg(not(target_os = "linux"))]
fn pre_release_modifiers() {
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
fn pre_release_modifiers() {
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
fn simulate_key_alias(alias: &str) -> bool {
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
fn simulate_key_alias(alias: &str) -> bool {
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
fn modifier_alias_to_rdev_key(alias: &str) -> Option<Key> {
    match alias {
        "ctrl" | "control" => Some(Key::ControlLeft),
        "lctrl" => Some(Key::ControlLeft),
        "rctrl" => Some(Key::ControlRight),
        "alt" => Some(Key::Alt),
        "lalt" => Some(Key::Alt),
        "ralt" => Some(Key::AltGr),
        "shift" => Some(Key::ShiftLeft),
        "lshift" => Some(Key::ShiftLeft),
        "rshift" => Some(Key::ShiftRight),
        "win" | "mod" | "super" | "meta" => Some(Key::MetaLeft),
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
        "lctrl" => Some(evdev::KeyCode::KEY_LEFTCTRL),
        "rctrl" => Some(evdev::KeyCode::KEY_RIGHTCTRL),
        "alt" => Some(evdev::KeyCode::KEY_LEFTALT),
        "lalt" => Some(evdev::KeyCode::KEY_LEFTALT),
        "ralt" => Some(evdev::KeyCode::KEY_RIGHTALT),
        "shift" => Some(evdev::KeyCode::KEY_LEFTSHIFT),
        "lshift" => Some(evdev::KeyCode::KEY_LEFTSHIFT),
        "rshift" => Some(evdev::KeyCode::KEY_RIGHTSHIFT),
        "win" | "mod" | "super" | "meta" => Some(evdev::KeyCode::KEY_LEFTMETA),
        "cmd" | "command" => Some(evdev::KeyCode::KEY_LEFTMETA),
        "opt" | "option" => Some(evdev::KeyCode::KEY_LEFTALT),
        _ => None,
    }
}

/// Maps a key alias string to an rdev Key.
#[cfg(not(target_os = "linux"))]
fn alias_to_rdev_key(alias: &str) -> Option<Key> {
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
        "tilde" => Some(Key::BackQuote), // tilde is shift+backtick, but maps to same physical key
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

/// Injects a text segment into the active application.
pub fn inject_text_segment(text: &str, original_clipboard: &Option<String>) -> Option<String> {
    let post_paste_wait = if cfg!(target_os = "windows") {
        Duration::from_millis(220)
    } else if cfg!(target_os = "linux") {
        Duration::from_millis(300)
    } else {
        Duration::from_millis(160)
    };

    #[cfg(windows)]
    {
        let mut clip = crate::platform::windows::WindowsClipboard;
        if original_clipboard.is_none() {
            // First text segment: save the original clipboard.
            match prepare_clipboard_for_expansion(&mut clip, text) {
                Ok(orig) => {
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                    Some(orig)
                }
                Err(e) => {
                    if e.starts_with("clipboard verify failed:") {
                        warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                    } else {
                        error!("Could not prepare clipboard before paste: {}", e);
                    }
                    None
                }
            }
        } else {
            // Subsequent text segments: clipboard was already saved.
            if let Err(e) = clip.set_text(text) {
                error!("Failed to set clipboard for text segment: {}", e);
                return original_clipboard.clone();
            }
            thread::sleep(Duration::from_millis(25));
            simulate_paste();
            thread::sleep(post_paste_wait);
            original_clipboard.clone()
        }
    }

    #[cfg(target_os = "linux")]
    {
        use crate::platform::linux;

        let has_display =
            std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok();
        let use_typing = !has_display;

        if !use_typing {
            let mut clipboard = linux::LinuxClipboard;
            if original_clipboard.is_none() {
                match prepare_clipboard_for_expansion(&mut clipboard, text) {
                    Ok(orig) => {
                        simulate_paste();
                        thread::sleep(post_paste_wait);
                        return Some(orig);
                    }
                    Err(e) => {
                        error!(
                            "Clipboard expansion failed (verify mismatch or permission issue: {}). Falling back to direct typing.",
                            e
                        );
                    }
                }
            } else {
                if let Err(e) = clipboard.set_text(text) {
                    error!("Failed to set clipboard for text segment: {}", e);
                } else {
                    thread::sleep(Duration::from_millis(25));
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                }
                return original_clipboard.clone();
            }
        }

        // At this point, we must use direct typing (either display-less or fallback).
        if let Some(lookup) = linux::get_reverse_lookup() {
            linux::uinput::simulate_type_string(text, lookup);
        } else {
            error!("Direct typing failed: Linux XKB mapper not initialized");
        }
        original_clipboard.clone()
    }

    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        let mut clipboard = match Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to initialize clipboard: {}", e);
                return None;
            }
        };

        if original_clipboard.is_none() {
            match prepare_clipboard_for_expansion(&mut clipboard, text) {
                Ok(orig) => {
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                    Some(orig)
                }
                Err(e) => {
                    if e.starts_with("clipboard verify failed:") {
                        warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                    } else {
                        error!("Could not prepare clipboard before paste: {}", e);
                    }
                    None
                }
            }
        } else {
            if let Err(e) = clipboard.set_text(text) {
                error!("Failed to set clipboard for text segment: {}", e);
                return original_clipboard.clone();
            }
            thread::sleep(Duration::from_millis(25));
            simulate_paste();
            thread::sleep(post_paste_wait);
            original_clipboard.clone()
        }
    }
}

/// Restores the user's original clipboard content.
fn restore_clipboard(original: &str) {
    #[cfg(windows)]
    {
        let mut clip = crate::platform::windows::WindowsClipboard;
        if let Err(e) = clip.set_text(original) {
            error!("Failed to restore clipboard: {}", e);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut clipboard = crate::platform::linux::LinuxClipboard;
        if let Err(e) = clipboard.set_text(original) {
            error!("Failed to restore clipboard: {}", e);
        }
    }

    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        if let Ok(mut clipboard) = Clipboard::new()
            && let Err(e) = clipboard.set_text(original)
        {
            error!("Failed to restore clipboard: {}", e);
        }
    }
}

pub fn restore_clipboard_text(original: &str) {
    restore_clipboard(original);
}

pub struct StreamingTextSession {
    guard: Option<MutexGuard<'static, ()>>,
    original_clipboard: Option<String>,
}

impl StreamingTextSession {
    pub fn begin() -> Self {
        let guard = inject_mutex().lock().expect("inject mutex poisoned");
        INJECTION_ABORT.store(false, Ordering::SeqCst);
        pre_release_modifiers();
        IS_INJECTING.store(true, Ordering::SeqCst);

        Self {
            guard: Some(guard),
            original_clipboard: None,
        }
    }

    pub fn push_text(&mut self, text: &str) -> bool {
        if text.is_empty() || INJECTION_ABORT.load(Ordering::SeqCst) {
            return !INJECTION_ABORT.load(Ordering::SeqCst);
        }

        if let Some(original) = inject_text_segment(text, &self.original_clipboard)
            && self.original_clipboard.is_none()
        {
            self.original_clipboard = Some(original);
        }

        !INJECTION_ABORT.load(Ordering::SeqCst)
    }

    pub fn abort_requested(&self) -> bool {
        INJECTION_ABORT.load(Ordering::SeqCst)
    }

    pub fn finish(&mut self) {
        if let Some(ref original) = self.original_clipboard {
            restore_clipboard(original);
        }

        pre_release_modifiers();
        self.original_clipboard = None;
        INJECTION_ABORT.store(false, Ordering::SeqCst);
        IS_INJECTING.store(false, Ordering::SeqCst);
        self.guard.take();
    }
}

impl Drop for StreamingTextSession {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Executes an ordered sequence of expansion steps.
///
/// This is the new sequence-aware injector entry point. It replaces the old
/// `inject_payload` function. The sequence can contain text pastes, key presses,
/// and explicit delays.
///
/// **Safety protocols**:
/// - **Pre-Release**: All modifier keys are released before the sequence starts.
/// - **Clipboard Dance**: The user's original clipboard is saved once at the start
///   and restored once at the end.
/// - **Panic Release**: All modifier keys are released at the end (success or failure).
/// - **Implicit Delay**: A 10ms delay is inserted between every step.
///
/// `IS_INJECTING` must already be `true` when this is called (the hook sets it
/// before spawning this thread). We clear it when we are done.
pub fn inject_expansion(
    steps: Vec<ExpansionStep>,
    delete_count: usize,
    spinner_style: taurine_core::settings::SpinnerStyle,
) {
    let _inject_guard = inject_mutex().lock().expect("inject mutex poisoned");

    // Clear any stale abort from a previous injection.
    INJECTION_ABORT.store(false, Ordering::SeqCst);

    // Pre-Release: neutralize modifier state before any injection.
    pre_release_modifiers();

    // On Linux, give the OS a moment to register the released modifiers before typing starts.
    #[cfg(target_os = "linux")]
    thread::sleep(Duration::from_millis(10));

    // 1. Erase the trigger.
    erase_trigger(delete_count);

    #[cfg(target_os = "linux")]
    thread::sleep(Duration::from_millis(20));

    // 2. Execute each step in sequence.
    let mut original_clipboard: Option<String> = None;

    for (i, step) in steps.iter().enumerate() {
        // Check for user-initiated abort before each step.
        if INJECTION_ABORT.load(Ordering::SeqCst) {
            debug!("Injection aborted by physical keypress at step {}", i);
            break;
        }

        // Implicit inter-step delay (skip before the very first step).
        if i > 0 {
            thread::sleep(Duration::from_millis(INTER_STEP_DELAY_MS));
        }

        match step {
            ExpansionStep::Text(text) => {
                if let Some(orig) = inject_text_segment(text, &original_clipboard) {
                    if original_clipboard.is_none() {
                        original_clipboard = Some(orig);
                    }
                } else if original_clipboard.is_none() {
                    // First text segment failed entirely — abort.
                    break;
                }
            }
            ExpansionStep::KeyPress(alias) => {
                if !simulate_key_alias(alias) {
                    debug!("Unknown key alias '{}', skipping", alias);
                }
            }
            ExpansionStep::Delay(ms) => {
                // Split long delays into smaller chunks so abort is responsive.
                let mut remaining = *ms;
                while remaining > 0 {
                    if INJECTION_ABORT.load(Ordering::SeqCst) {
                        break;
                    }
                    let chunk = remaining.min(50);
                    thread::sleep(Duration::from_millis(chunk));
                    remaining -= chunk;
                }
            }
            ExpansionStep::Script(metadata) => {
                match metadata.behavior {
                    ScriptBehavior::Inline => {
                        // Start the modern Braille spinner in a dedicated module
                        let spinner_handle = crate::spinner::start(spinner_style);

                        // Execute script and block until completion (or abort/timeout)
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("Failed to initialize script runtime");

                        let script_result: taurine_core::Result<String> =
                            rt.block_on(crate::platform::executor::execute_script(metadata));

                        // Stop the spinner
                        spinner_handle.stop();

                        match script_result {
                            Ok(output) => {
                                let out: String = output;
                                if !out.is_empty() {
                                    inject_text_segment(&out, &original_clipboard);
                                }
                            }
                            Err(e) => {
                                // Silent abort: if the user killed it, don't paste an error.
                                let err_str = e.to_string();
                                if !err_str.contains("aborted by user") {
                                    let err_msg = format!(" [Error: {}] ", e);
                                    inject_text_segment(&err_msg, &original_clipboard);
                                }
                            }
                        }
                    }
                    ScriptBehavior::Silent => {
                        // Fire and forget in the background
                        let metadata_clone = metadata.clone();
                        thread::spawn(move || {
                            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                            {
                                let _ = rt.block_on(crate::platform::executor::execute_script(
                                    &metadata_clone,
                                ));
                            }
                        });
                    }
                }
            }
        }
    }

    // 3. Restore the user's original clipboard (if we touched it).
    if let Some(ref original) = original_clipboard {
        restore_clipboard(original);
    }

    // Panic Release: ensure all modifiers are logically released.
    pre_release_modifiers();

    INJECTION_ABORT.store(false, Ordering::SeqCst);
    IS_INJECTING.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ClipboardManager;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::{Arc, Barrier};

    /// Mock clipboard: records operations and supports simulating a race where post-write read
    /// does not match the payload (verify failure).
    struct MockClipboard {
        text: String,
        /// Number of `get_text` calls so far (used to sabotage the verify read).
        get_count: usize,
        /// If true, the second `get_text` returns stale content (another writer "won" the race).
        sabotage_second_read: bool,
        ops: Vec<&'static str>,
    }

    impl MockClipboard {
        fn new(initial: &str) -> Self {
            Self {
                text: initial.to_string(),
                get_count: 0,
                sabotage_second_read: false,
                ops: Vec::new(),
            }
        }

        fn with_sabotage(initial: &str) -> Self {
            Self {
                text: initial.to_string(),
                get_count: 0,
                sabotage_second_read: true,
                ops: Vec::new(),
            }
        }
    }

    impl crate::platform::ClipboardManager for MockClipboard {
        fn get_text(&mut self) -> Result<String, String> {
            self.get_count += 1;
            self.ops.push("get_text");
            if self.sabotage_second_read && self.get_count == 2 {
                // Verify read (after set_text): another writer won the race.
                return Ok("STALE_FROM_ANOTHER_PROCESS".to_string());
            }
            Ok(self.text.clone())
        }

        fn set_text(&mut self, text: &str) -> Result<(), String> {
            self.ops.push("set_text");
            self.text = text.to_string();
            Ok(())
        }
    }

    #[test]
    fn prepare_reads_previous_clipboard_sets_payload_verifies_then_restore_restores_previous() {
        let mut mock = MockClipboard::new("Something the user had copied earlier");
        let payload = "Expanded text only — not the old clipboard";

        let original = prepare_clipboard_for_expansion(&mut mock, payload).unwrap();
        assert_eq!(original, "Something the user had copied earlier");
        assert_eq!(
            mock.text, payload,
            "clipboard must hold payload until after paste+restore"
        );

        // Simulated app would read this value on Ctrl+V — not the pre-expansion clipboard.
        assert_eq!(mock.get_text().unwrap(), payload);

        mock.set_text(&original).unwrap();
        assert_eq!(
            mock.text, "Something the user had copied earlier",
            "after restore, user must see their original clip, not the expansion"
        );
    }

    #[test]
    fn prepare_fails_if_clipboard_raced_before_paste_so_stale_clip_is_never_intended_payload() {
        let mut mock = MockClipboard::with_sabotage("old");
        let err = prepare_clipboard_for_expansion(&mut mock, "new").unwrap_err();
        assert!(
            err.contains("clipboard verify failed"),
            "expected verify error, got {:?}",
            err
        );
    }

    #[test]
    fn inject_mutex_serializes_overlapping_injections_no_interleaved_critical_sections() {
        let depth = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(3));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let depth = depth.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    let _guard = inject_mutex().lock().expect("mutex");
                    assert_eq!(depth.fetch_add(1, AtomicOrdering::SeqCst), 0);
                    thread::sleep(Duration::from_millis(40));
                    depth.fetch_sub(1, AtomicOrdering::SeqCst);
                })
            })
            .collect();

        barrier.wait();
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn mock_clipboard_operation_order_matches_protocol() {
        let mut mock = MockClipboard::new("clip0");
        let _ = prepare_clipboard_for_expansion(&mut mock, "payload1").unwrap();
        assert_eq!(
            mock.ops,
            vec!["get_text", "set_text", "get_text"],
            "must read original, write payload, read back to verify before any paste"
        );
    }

    // ----- Phase 3: Key alias and modifier resolution tests -----

    #[cfg(target_os = "linux")]
    fn resolve_alias(alias: &str) -> bool {
        alias_to_evdev_key(alias).is_some()
    }
    #[cfg(not(target_os = "linux"))]
    fn resolve_alias(alias: &str) -> bool {
        alias_to_rdev_key(alias).is_some()
    }

    #[cfg(target_os = "linux")]
    fn resolve_modifier(alias: &str) -> bool {
        modifier_alias_to_evdev_key(alias).is_some()
    }
    #[cfg(not(target_os = "linux"))]
    fn resolve_modifier(alias: &str) -> bool {
        modifier_alias_to_rdev_key(alias).is_some()
    }

    #[test]
    fn alias_resolves_alphabet_keys() {
        for letter in 'a'..='z' {
            let alias = letter.to_string();
            assert!(resolve_alias(&alias), "missing alias for key '{}'", alias);
        }
    }

    #[test]
    fn alias_resolves_number_keys() {
        for digit in '0'..='9' {
            let alias = digit.to_string();
            assert!(resolve_alias(&alias), "missing alias for key '{}'", alias);
        }
    }

    #[test]
    fn alias_resolves_function_keys() {
        for n in 1..=12 {
            let alias = format!("f{}", n);
            assert!(resolve_alias(&alias), "missing alias for key '{}'", alias);
        }
    }

    #[test]
    fn alias_resolves_special_characters() {
        let specials = [
            "backtick",
            "grave",
            "tilde",
            "minus",
            "dash",
            "equal",
            "equals",
            "backslash",
            "semicolon",
            "quote",
            "apostrophe",
            "comma",
            "dot",
            "period",
            "slash",
            "lbracket",
            "rbracket",
            "capslock",
            "numlock",
            "scrolllock",
            "printscreen",
            "prtsc",
            "pause",
            "break",
            "insert",
            "ins",
        ];
        for alias in &specials {
            assert!(
                resolve_alias(alias),
                "missing alias for special key '{}'",
                alias
            );
        }
    }

    #[test]
    fn modifier_alias_resolves_all_variants() {
        let modifiers = [
            "ctrl", "control", "lctrl", "rctrl", "alt", "lalt", "ralt", "shift", "lshift",
            "rshift", "win", "mod", "super", "meta", "cmd", "command", "opt", "option",
        ];
        for alias in &modifiers {
            assert!(
                resolve_modifier(alias),
                "missing modifier alias '{}'",
                alias
            );
        }
    }

    #[test]
    fn modifier_alias_rejects_unknown() {
        assert!(!resolve_modifier("hyper"));
        assert!(!resolve_modifier("fn"));
    }

    #[test]
    fn alias_resolves_standalone_modifiers() {
        assert!(resolve_alias("ctrl"));
        assert!(resolve_alias("shift"));
        assert!(resolve_alias("alt"));
        assert!(resolve_alias("win"));
    }

    #[test]
    fn alias_rejects_unknown_keys() {
        assert!(!resolve_alias("unknown_key"));
        assert!(!resolve_alias("hyper"));
    }
}
