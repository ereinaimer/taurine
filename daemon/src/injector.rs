#[cfg(any(windows, target_os = "linux"))]
use crate::platform::ClipboardManager;
use arboard::Clipboard;
#[cfg(not(target_os = "linux"))]
use rdev::{EventType, Key, simulate};
#[cfg(not(target_os = "linux"))]
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;
#[cfg(not(target_os = "linux"))]
use std::time::Instant;
#[cfg(not(target_os = "linux"))]
use tracing::warn;
use tracing::{debug, error, trace};

use taurine_core::engine::shell::ScriptBehavior;
use taurine_core::engine::variables::ExpansionStep;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseButton {
    Left,
    Right,
    Middle,
}

#[cfg(not(target_os = "linux"))]
fn simulate_mouse_click(button: MouseButton) {
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
fn simulate_mouse_move(x: u16, y: u16) {
    let _ = simulate_monitored(&rdev::EventType::MouseMove {
        x: x as f64,
        y: y as f64,
    });
}

#[cfg(not(target_os = "linux"))]
fn simulate_mouse_scroll(delta: i32) {
    let _ = simulate_monitored(&rdev::EventType::Wheel {
        delta_x: 0,
        delta_y: delta as i64,
    });
}

#[cfg(not(target_os = "linux"))]
fn simulate_mouse_hold(button: MouseButton, hold: bool) {
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
fn simulate_mouse_click(button: MouseButton) {
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
fn simulate_mouse_move(x: u16, y: u16) {
    use x11rb::connection::Connection;
    if let Ok((conn, _)) = x11rb::connect(None)
        && let Some(screen) = conn.setup().roots.first()
    {
        let _ = conn.warp_pointer(x11rb::NONE, screen.root, 0, 0, 0, 0, x as i16, y as i16);
        let _ = conn.flush();
    }
}

#[cfg(target_os = "linux")]
fn simulate_mouse_scroll(delta: i32) {
    crate::platform::linux::uinput::simulate_mouse_scroll(delta);
}

#[cfg(target_os = "linux")]
fn simulate_mouse_hold(button: MouseButton, hold: bool) {
    let evdev_btn = match button {
        MouseButton::Left => evdev::KeyCode::BTN_LEFT,
        MouseButton::Right => evdev::KeyCode::BTN_RIGHT,
        MouseButton::Middle => evdev::KeyCode::BTN_MIDDLE,
    };
    crate::platform::linux::uinput::simulate_mouse_button(evdev_btn, hold);
}

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

static INJECTION_SCOPE_DEPTH: AtomicUsize = AtomicUsize::new(0);
static INJECTION_VISIBILITY_DEPTH: AtomicUsize = AtomicUsize::new(0);

/// Set to `true` momentarily while we are simulating a keystroke. The hook thread
/// checks this to distinguish physical from synthetic keyboard events.
#[allow(dead_code)]
pub static IS_SIMULATING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct InjectionGate<'a> {
    is_injecting: &'a AtomicBool,
    abort: &'a AtomicBool,
    scope_depth: &'a AtomicUsize,
    visibility_depth: &'a AtomicUsize,
}

impl<'a> InjectionGate<'a> {
    const fn new(
        is_injecting: &'a AtomicBool,
        abort: &'a AtomicBool,
        scope_depth: &'a AtomicUsize,
        visibility_depth: &'a AtomicUsize,
    ) -> Self {
        Self {
            is_injecting,
            abort,
            scope_depth,
            visibility_depth,
        }
    }

    fn begin_scope(self) {
        let was_outermost_scope = self.scope_depth.fetch_add(1, Ordering::SeqCst) == 0;
        self.visibility_depth.fetch_add(1, Ordering::SeqCst);
        self.is_injecting.store(true, Ordering::SeqCst);
        if was_outermost_scope {
            self.abort.store(false, Ordering::SeqCst);
        }
    }

    fn end_scope(self) {
        let previous_scope_depth = self.scope_depth.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous_scope_depth > 0, "scope depth underflow");
        let remaining_scope_depth = previous_scope_depth.saturating_sub(1);

        let previous_visibility_depth = self.visibility_depth.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous_visibility_depth > 0, "visibility depth underflow");
        let remaining_visibility_depth = previous_visibility_depth.saturating_sub(1);

        self.is_injecting
            .store(remaining_visibility_depth > 0, Ordering::SeqCst);
        if remaining_scope_depth == 0 {
            self.abort.store(false, Ordering::SeqCst);
        }
    }

    fn begin_visibility(self) {
        self.visibility_depth.fetch_add(1, Ordering::SeqCst);
        self.is_injecting.store(true, Ordering::SeqCst);
    }

    fn end_visibility(self) {
        let previous_visibility_depth = self.visibility_depth.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous_visibility_depth > 0, "visibility depth underflow");
        let remaining_visibility_depth = previous_visibility_depth.saturating_sub(1);
        self.is_injecting
            .store(remaining_visibility_depth > 0, Ordering::SeqCst);
    }
}

fn injection_gate() -> InjectionGate<'static> {
    InjectionGate::new(
        &IS_INJECTING,
        &INJECTION_ABORT,
        &INJECTION_SCOPE_DEPTH,
        &INJECTION_VISIBILITY_DEPTH,
    )
}

/// Marks a synthetic injection scope as active and releases it when dropped.
///
/// The guard uses ref-counted bookkeeping so nested scopes can safely outlive the scope that
/// spawned them, which is required for inline AI follow-up tasks that finish on another thread.
pub struct InjectionFlagGuard {
    gate: InjectionGate<'static>,
}

impl InjectionFlagGuard {
    pub fn begin() -> Self {
        let gate = injection_gate();
        gate.begin_scope();

        trace!(
            scope_depth = INJECTION_SCOPE_DEPTH.load(Ordering::SeqCst),
            visibility_depth = INJECTION_VISIBILITY_DEPTH.load(Ordering::SeqCst),
            "Injection guard armed"
        );

        Self { gate }
    }
}

impl Drop for InjectionFlagGuard {
    fn drop(&mut self) {
        self.gate.end_scope();

        trace!(
            remaining_scope_depth = INJECTION_SCOPE_DEPTH.load(Ordering::SeqCst),
            remaining_visibility_depth = INJECTION_VISIBILITY_DEPTH.load(Ordering::SeqCst),
            restored_injecting = IS_INJECTING.load(Ordering::SeqCst),
            restored_abort = INJECTION_ABORT.load(Ordering::SeqCst),
            "Injection guard reset"
        );
    }
}

/// Temporarily hides spinner-driven synthetic input from the hook while preserving any outer
/// injection state.
pub struct InjectionVisibilityGuard {
    gate: InjectionGate<'static>,
}

impl InjectionVisibilityGuard {
    pub fn begin() -> Self {
        let gate = injection_gate();
        gate.begin_visibility();
        Self { gate }
    }
}

impl Drop for InjectionVisibilityGuard {
    fn drop(&mut self) {
        self.gate.end_visibility();
        trace!(
            remaining_visibility_depth = INJECTION_VISIBILITY_DEPTH.load(Ordering::SeqCst),
            restored_injecting = IS_INJECTING.load(Ordering::SeqCst),
            "Injection visibility guard reset"
        );
    }
}

pub fn spawn_guarded_injection_thread<F>(thread_name: &str, task: F)
where
    F: FnOnce() + Send + 'static,
{
    let guard = InjectionFlagGuard::begin();
    let spawn_result = thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            let _guard = guard;
            task();
        });

    if let Err(error) = spawn_result {
        error!(
            thread_name,
            error = %error,
            "Failed to spawn guarded injection thread"
        );
    }
}

#[cfg(not(target_os = "linux"))]
#[derive(Clone)]
struct SimulatedEvent {
    event: EventType,
    queued_at: Instant,
}

#[cfg(not(target_os = "linux"))]
const SIMULATED_EVENT_TTL: Duration = Duration::from_millis(100);

#[cfg(not(target_os = "linux"))]
fn simulated_events() -> &'static Mutex<VecDeque<SimulatedEvent>> {
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
    trace!("Injecting {} backspaces", delete_count);
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

/// Reverts a just-inserted expansion back into its original trigger text.
///
/// The triggering Backspace is now swallowed by the hook state machine on every
/// platform, so Taurine must erase the full expanded output before retyping the
/// original trigger.
pub fn inject_undo(trigger_string: String, output_length: usize) {
    let _state_guard = InjectionFlagGuard::begin();
    let _inject_guard = inject_mutex().lock().expect("inject mutex poisoned");
    pre_release_modifiers();

    #[cfg(target_os = "linux")]
    thread::sleep(Duration::from_millis(10));

    erase_trigger(output_length);

    #[cfg(target_os = "linux")]
    thread::sleep(Duration::from_millis(20));

    let original_clipboard = inject_text_segment(&trigger_string, &None);

    if let Some(ref original) = original_clipboard.original_clipboard {
        restore_clipboard(original);
    }

    pre_release_modifiers();
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InjectionReport {
    pub successful_chars: usize,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextSegmentInjection {
    pub original_clipboard: Option<String>,
    pub injected_chars: usize,
    pub success: bool,
}

pub fn inject_text_segment(
    text: &str,
    original_clipboard: &Option<String>,
) -> TextSegmentInjection {
    let injected_chars = text.chars().count();
    let delay_ms = if let Ok(conn) = taurine_core::db::init::setup() {
        taurine_core::settings::SettingsManager::new(&conn)
            .load_all()
            .clipboard_restore_delay_ms
    } else {
        taurine_core::settings::Settings::default().clipboard_restore_delay_ms
    };
    let post_paste_wait = Duration::from_millis(delay_ms as u64);

    #[cfg(windows)]
    {
        let mut clip = crate::platform::windows::WindowsClipboard;
        if original_clipboard.is_none() {
            // First text segment: save the original clipboard.
            match prepare_clipboard_for_expansion(&mut clip, text) {
                Ok(orig) => {
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                    TextSegmentInjection {
                        original_clipboard: Some(orig),
                        injected_chars,
                        success: true,
                    }
                }
                Err(e) => {
                    if e.starts_with("clipboard verify failed:") {
                        warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                    } else {
                        error!("Could not prepare clipboard before paste: {}", e);
                    }
                    TextSegmentInjection::default()
                }
            }
        } else {
            // Subsequent text segments: clipboard was already saved.
            if let Err(e) = clip.set_text(text) {
                error!("Failed to set clipboard for text segment: {}", e);
                return TextSegmentInjection {
                    original_clipboard: original_clipboard.clone(),
                    injected_chars: 0,
                    success: false,
                };
            }
            thread::sleep(Duration::from_millis(25));
            simulate_paste();
            thread::sleep(post_paste_wait);
            TextSegmentInjection {
                original_clipboard: original_clipboard.clone(),
                injected_chars,
                success: true,
            }
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
                        return TextSegmentInjection {
                            original_clipboard: Some(orig),
                            injected_chars,
                            success: true,
                        };
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
                    return TextSegmentInjection {
                        original_clipboard: original_clipboard.clone(),
                        injected_chars: 0,
                        success: false,
                    };
                } else {
                    thread::sleep(Duration::from_millis(25));
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                }
                return TextSegmentInjection {
                    original_clipboard: original_clipboard.clone(),
                    injected_chars,
                    success: true,
                };
            }
        }

        // At this point, we must use direct typing (either display-less or fallback).
        if let Some(lookup) = linux::get_reverse_lookup() {
            linux::uinput::simulate_type_string(text, lookup);
            TextSegmentInjection {
                original_clipboard: original_clipboard.clone(),
                injected_chars,
                success: true,
            }
        } else {
            error!("Direct typing failed: Linux XKB mapper not initialized");
            TextSegmentInjection {
                original_clipboard: original_clipboard.clone(),
                injected_chars: 0,
                success: false,
            }
        }
    }

    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        let mut clipboard = match Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to initialize clipboard: {}", e);
                return TextSegmentInjection::default();
            }
        };

        if original_clipboard.is_none() {
            match prepare_clipboard_for_expansion(&mut clipboard, text) {
                Ok(orig) => {
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                    TextSegmentInjection {
                        original_clipboard: Some(orig),
                        injected_chars,
                        success: true,
                    }
                }
                Err(e) => {
                    if e.starts_with("clipboard verify failed:") {
                        warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                    } else {
                        error!("Could not prepare clipboard before paste: {}", e);
                    }
                    TextSegmentInjection::default()
                }
            }
        } else {
            if let Err(e) = clipboard.set_text(text) {
                error!("Failed to set clipboard for text segment: {}", e);
                return TextSegmentInjection {
                    original_clipboard: original_clipboard.clone(),
                    injected_chars: 0,
                    success: false,
                };
            }
            thread::sleep(Duration::from_millis(25));
            simulate_paste();
            thread::sleep(post_paste_wait);
            TextSegmentInjection {
                original_clipboard: original_clipboard.clone(),
                injected_chars,
                success: true,
            }
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
    state_guard: Option<InjectionFlagGuard>,
    original_clipboard: Option<String>,
    tracked_chars: usize,
}

impl StreamingTextSession {
    pub fn begin() -> Self {
        let guard = inject_mutex().lock().expect("inject mutex poisoned");
        let state_guard = InjectionFlagGuard::begin();
        pre_release_modifiers();

        Self {
            guard: Some(guard),
            state_guard: Some(state_guard),
            original_clipboard: None,
            tracked_chars: 0,
        }
    }

    pub fn push_text(&mut self, text: &str, track_metrics: bool) -> bool {
        if text.is_empty() || INJECTION_ABORT.load(Ordering::SeqCst) {
            return !INJECTION_ABORT.load(Ordering::SeqCst);
        }

        let injection = inject_text_segment(text, &self.original_clipboard);
        if self.original_clipboard.is_none() {
            self.original_clipboard = injection.original_clipboard;
        }
        if track_metrics {
            self.tracked_chars = self.tracked_chars.saturating_add(injection.injected_chars);
        }

        !INJECTION_ABORT.load(Ordering::SeqCst)
    }

    pub fn abort_requested(&self) -> bool {
        INJECTION_ABORT.load(Ordering::SeqCst)
    }

    pub fn finish(&mut self) -> usize {
        let tracked_chars = self.tracked_chars;
        if let Some(ref original) = self.original_clipboard {
            restore_clipboard(original);
        }

        pre_release_modifiers();
        self.original_clipboard = None;
        self.tracked_chars = 0;
        self.guard.take();
        self.state_guard.take();
        tracked_chars
    }
}

impl Drop for StreamingTextSession {
    fn drop(&mut self) {
        let _ = self.finish();
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
/// Injection state is guarded so synthetic event suppression is reset even if the work returns
/// early or panics inside the caller-owned thread.
pub fn inject_expansion(
    steps: Vec<ExpansionStep>,
    delete_count: usize,
    spinner_style: taurine_core::settings::SpinnerStyle,
) -> InjectionReport {
    let _state_guard = InjectionFlagGuard::begin();
    let _inject_guard = inject_mutex().lock().expect("inject mutex poisoned");

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
    let mut report = InjectionReport {
        successful_chars: 0,
        completed: true,
    };

    for (i, step) in steps.iter().enumerate() {
        // Check for user-initiated abort before each step.
        if INJECTION_ABORT.load(Ordering::SeqCst) {
            debug!("Injection aborted by physical keypress at step {}", i);
            report.completed = false;
            break;
        }

        // Implicit inter-step delay (skip before the very first step).
        if i > 0 {
            thread::sleep(Duration::from_millis(INTER_STEP_DELAY_MS));
        }

        match step {
            ExpansionStep::Text(text) => {
                let injection = inject_text_segment(text, &original_clipboard);
                report.successful_chars = report
                    .successful_chars
                    .saturating_add(injection.injected_chars);
                if original_clipboard.is_none() {
                    original_clipboard = injection.original_clipboard;
                }
                if !injection.success && original_clipboard.is_none() {
                    // First text segment failed entirely — abort.
                    report.completed = false;
                    break;
                }
                if !injection.success {
                    report.completed = false;
                }
            }
            ExpansionStep::KeyPress(alias) => {
                if !simulate_key_alias(alias) {
                    debug!("Unknown key alias '{}', skipping", alias);
                }
            }
            ExpansionStep::MouseClick => {
                simulate_mouse_click(MouseButton::Left);
            }
            ExpansionStep::MouseRClick => {
                simulate_mouse_click(MouseButton::Right);
            }
            ExpansionStep::MouseMClick => {
                simulate_mouse_click(MouseButton::Middle);
            }
            ExpansionStep::MouseMove(x, y) => {
                simulate_mouse_move(*x, *y);
            }
            ExpansionStep::MouseScroll(delta) => {
                simulate_mouse_scroll(*delta);
            }
            ExpansionStep::MouseHold => {
                simulate_mouse_hold(MouseButton::Left, true);
            }
            ExpansionStep::MouseRelease => {
                simulate_mouse_hold(MouseButton::Left, false);
            }
            ExpansionStep::Delay(ms) => {
                // Split long delays into smaller chunks so abort is responsive.
                let mut remaining = *ms;
                while remaining > 0 {
                    if INJECTION_ABORT.load(Ordering::SeqCst) {
                        report.completed = false;
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
                        // Start the modern Braille spinner from core
                        let spinner_handle = taurine_core::utils::spinner::spawn_threaded(
                            spinner_style,
                            crate::platform::spinner_renderer::OsSpinnerRenderer::default(),
                        );

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
                                    let injection = inject_text_segment(&out, &original_clipboard);
                                    report.successful_chars = report
                                        .successful_chars
                                        .saturating_add(injection.injected_chars);
                                    if original_clipboard.is_none() {
                                        original_clipboard = injection.original_clipboard;
                                    }
                                    if !injection.success {
                                        report.completed = false;
                                    }
                                }
                            }
                            Err(e) => {
                                report.completed = false;
                                // Silent abort: if the user killed it, don't paste an error.
                                let err_str = e.to_string();
                                if !err_str.contains("aborted by user") {
                                    let err_msg = format!(" [Error: {}] ", e);
                                    let injection =
                                        inject_text_segment(&err_msg, &original_clipboard);
                                    if original_clipboard.is_none() {
                                        original_clipboard = injection.original_clipboard;
                                    }
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
            ExpansionStep::InlineRun(metadata) => match metadata.behavior {
                ScriptBehavior::Inline => {
                    let spinner_handle = taurine_core::utils::spinner::spawn_threaded(
                        spinner_style,
                        crate::platform::spinner_renderer::OsSpinnerRenderer::default(),
                    );

                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to initialize inline run runtime");

                    let script_result: taurine_core::Result<String> =
                        rt.block_on(crate::platform::executor::execute_script(metadata));

                    spinner_handle.stop();

                    match script_result {
                        Ok(output) => {
                            if !output.is_empty() {
                                let injection = inject_text_segment(&output, &original_clipboard);
                                report.successful_chars = report
                                    .successful_chars
                                    .saturating_add(injection.injected_chars);
                                if original_clipboard.is_none() {
                                    original_clipboard = injection.original_clipboard;
                                }
                                if !injection.success {
                                    report.completed = false;
                                }
                            }
                        }
                        Err(e) => {
                            report.completed = false;
                            let err_str = e.to_string();
                            if !err_str.contains("aborted by user") {
                                let err_msg = format!("[Error: {}]", e);
                                let injection = inject_text_segment(&err_msg, &original_clipboard);
                                if original_clipboard.is_none() {
                                    original_clipboard = injection.original_clipboard;
                                }
                            }
                        }
                    }
                }
                ScriptBehavior::Silent => {
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
            },
        }
    }

    // 3. Restore the user's original clipboard (if we touched it).
    if let Some(ref original) = original_clipboard {
        restore_clipboard(original);
    }

    // Panic Release: ensure all modifiers are logically released.
    pre_release_modifiers();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ClipboardManager;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::{Arc, Barrier};
    use taurine_core::db::crud::AutomationAction;
    use taurine_core::engine::{EngineEvent, EngineState, Evaluator};

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

    fn assert_normal_expansion_still_works() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gm".to_string(),
            AutomationAction::text("Good morning!"),
        )]);
        let mut evaluator = Evaluator::new(state);

        for ch in ">gm".chars() {
            assert_eq!(
                evaluator.process_event(if ch == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(ch)
                }),
                None
            );
        }

        let expansion = evaluator
            .process_event(EngineEvent::ActionDelimiter)
            .expect("word trigger should still expand after AI cleanup");
        assert_eq!(
            expansion.steps,
            vec![ExpansionStep::Text("Good morning!".to_string())]
        );
    }

    #[test]
    fn injection_gate_successful_follow_up_cleanup_reenables_normal_expansion() {
        let is_injecting = AtomicBool::new(false);
        let abort = AtomicBool::new(false);
        let scope_depth = AtomicUsize::new(0);
        let visibility_depth = AtomicUsize::new(0);
        let gate = InjectionGate::new(&is_injecting, &abort, &scope_depth, &visibility_depth);

        gate.begin_scope();
        gate.begin_scope();
        gate.end_scope();

        assert!(
            is_injecting.load(Ordering::SeqCst),
            "the follow-up scope must stay active after the dispatch scope exits"
        );

        gate.end_scope();

        assert!(
            !is_injecting.load(Ordering::SeqCst),
            "successful follow-up cleanup must release hook suppression"
        );
        assert!(
            !abort.load(Ordering::SeqCst),
            "successful cleanup must leave no stale abort signal behind"
        );
        assert_normal_expansion_still_works();
    }

    #[test]
    fn injection_gate_cancelled_follow_up_cleanup_clears_abort_and_reenables_normal_expansion() {
        let is_injecting = AtomicBool::new(false);
        let abort = AtomicBool::new(false);
        let scope_depth = AtomicUsize::new(0);
        let visibility_depth = AtomicUsize::new(0);
        let gate = InjectionGate::new(&is_injecting, &abort, &scope_depth, &visibility_depth);

        gate.begin_scope();
        gate.begin_scope();
        abort.store(true, Ordering::SeqCst);

        gate.end_scope();
        assert!(
            abort.load(Ordering::SeqCst),
            "the active follow-up should still observe the cancel request until it exits"
        );

        gate.end_scope();

        assert!(
            !is_injecting.load(Ordering::SeqCst),
            "cancelled follow-up cleanup must release hook suppression"
        );
        assert!(
            !abort.load(Ordering::SeqCst),
            "cancelled cleanup must clear the abort signal for later triggers"
        );
        assert_normal_expansion_still_works();
    }

    #[test]
    fn injection_gate_error_cleanup_waits_for_overlapping_visibility_scope_before_releasing() {
        let is_injecting = AtomicBool::new(false);
        let abort = AtomicBool::new(false);
        let scope_depth = AtomicUsize::new(0);
        let visibility_depth = AtomicUsize::new(0);
        let gate = InjectionGate::new(&is_injecting, &abort, &scope_depth, &visibility_depth);

        gate.begin_scope();
        gate.begin_scope();
        gate.begin_visibility();

        gate.end_scope();
        gate.end_scope();

        assert!(
            is_injecting.load(Ordering::SeqCst),
            "an overlapping spinner or UI-injection frame must keep suppression active until it finishes"
        );

        gate.end_visibility();

        assert!(
            !is_injecting.load(Ordering::SeqCst),
            "error cleanup must fully release suppression after the last overlapping scope ends"
        );
        assert!(
            !abort.load(Ordering::SeqCst),
            "error cleanup must not leak abort state into future trigger handling"
        );
        assert_normal_expansion_still_works();
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn simulated_event_filter_consumes_only_the_expected_event() {
        let key_a_press = EventType::KeyPress(Key::KeyA);
        let key_b_press = EventType::KeyPress(Key::KeyB);

        {
            let mut queue = simulated_events()
                .lock()
                .expect("queue lock should succeed");
            queue.clear();
            queue.push_back(SimulatedEvent {
                event: key_a_press,
                queued_at: Instant::now(),
            });
        }

        assert!(
            !consume_simulated_event(&key_b_press),
            "different physical events must not be swallowed as synthetic"
        );
        assert!(
            consume_simulated_event(&key_a_press),
            "the exact synthetic event should be swallowed"
        );
        assert!(
            !consume_simulated_event(&key_a_press),
            "a consumed synthetic event should not match twice"
        );
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
            "ctrl",
            "control",
            "lctrl",
            "leftctrl",
            "leftcontrol",
            "rctrl",
            "rightctrl",
            "rightcontrol",
            "alt",
            "lalt",
            "leftalt",
            "leftoption",
            "ralt",
            "rightalt",
            "rightoption",
            "altgr",
            "shift",
            "lshift",
            "leftshift",
            "rshift",
            "rightshift",
            "win",
            "mod",
            "super",
            "meta",
            "lmeta",
            "leftmeta",
            "leftwin",
            "leftsuper",
            "leftcmd",
            "leftcommand",
            "rmeta",
            "rightmeta",
            "rightwin",
            "rightsuper",
            "rightcmd",
            "rightcommand",
            "cmd",
            "command",
            "opt",
            "option",
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
