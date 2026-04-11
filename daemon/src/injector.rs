#[cfg(not(target_os = "linux"))]
use crate::platform::ClipboardManager;
use arboard::Clipboard;
#[cfg(not(target_os = "linux"))]
use rdev::{EventType, Key, simulate};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use tracing::{debug, error, warn};

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
            let _ = simulate(&EventType::KeyPress(Key::Backspace));
            let _ = simulate(&EventType::KeyRelease(Key::Backspace));
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
        let _ = simulate(&EventType::KeyPress(modifier));
        let _ = simulate(&EventType::KeyPress(Key::KeyV));
        let _ = simulate(&EventType::KeyRelease(Key::KeyV));
        let _ = simulate(&EventType::KeyRelease(modifier));
    }
}

/// Erases the typed trigger sequence and pastes the expansion payload via the
/// OS clipboard, restoring the previous clipboard contents afterwards.
///
/// `IS_INJECTING` must already be `true` when this is called (the hook sets it
/// before spawning this thread). We clear it when we are done.
pub fn inject_payload(payload: String, delete_count: usize, left_arrow_count: usize) {
    let _inject_guard = inject_mutex().lock().expect("inject mutex poisoned");

    // 1. Erase the trigger
    erase_trigger(delete_count);

    #[cfg(target_os = "linux")]
    thread::sleep(Duration::from_millis(20));

    let post_paste_wait = if cfg!(target_os = "windows") {
        Duration::from_millis(220)
    } else if cfg!(target_os = "linux") {
        Duration::from_millis(300)
    } else {
        Duration::from_millis(160)
    };

    #[cfg(windows)]
    {
        // Win32 UTF-16 + cloud-clipboard exclusion flags so expansion text does not land in
        // Win+V history while keeping reliable paste for emoji and non-Latin scripts.
        let mut clip = crate::platform::windows::WindowsClipboard;
        let original_clipboard = match prepare_clipboard_for_expansion(&mut clip, &payload) {
            Ok(s) => s,
            Err(e) => {
                if e.starts_with("clipboard verify failed:") {
                    warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                } else {
                    error!("Could not prepare clipboard before paste: {}", e);
                }
                IS_INJECTING.store(false, Ordering::SeqCst);
                return;
            }
        };

        simulate_paste();
        thread::sleep(post_paste_wait);

        if let Err(e) = clip.set_text(&original_clipboard) {
            error!("Failed to restore clipboard: {}", e);
        }
    }

    #[cfg(not(windows))]
    {
        #[cfg(target_os = "linux")]
        let mut clipboard = crate::platform::linux::LinuxClipboard;

        #[cfg(not(target_os = "linux"))]
        let mut clipboard = match Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to initialize clipboard: {}", e);
                IS_INJECTING.store(false, Ordering::SeqCst);
                return;
            }
        };

        let original_clipboard = match prepare_clipboard_for_expansion(&mut clipboard, &payload) {
            Ok(s) => s,
            Err(e) => {
                if e.starts_with("clipboard verify failed:") {
                    warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                } else {
                    error!("Could not prepare clipboard before paste: {}", e);
                }
                IS_INJECTING.store(false, Ordering::SeqCst);
                return;
            }
        };

        simulate_paste();
        thread::sleep(post_paste_wait);

        if let Err(e) = clipboard.set_text(&original_clipboard) {
            error!("Failed to restore clipboard: {}", e);
        }
    }

    if left_arrow_count > 0 {
        debug!("Moving cursor left {} times", left_arrow_count);
        for _ in 0..left_arrow_count {
            #[cfg(target_os = "linux")]
            {
                crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_LEFT);
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = simulate(&EventType::KeyPress(Key::LeftArrow));
                let _ = simulate(&EventType::KeyRelease(Key::LeftArrow));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

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
}
