use arboard::Clipboard;
use rdev::{EventType, Key, simulate};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tracing::{debug, error};

/// Shared flag: set to `true` while we are simulating keystrokes so the
/// hook callback can ignore those synthetic events and avoid feeding them
/// back into the evaluator.
pub static IS_INJECTING: AtomicBool = AtomicBool::new(false);

/// Serializes concurrent injection requests. Without this, back-to-back
/// expansions spawn overlapping threads whose backspaces and clipboard
/// writes clobber each other.
static INJECTION_LOCK: Mutex<()> = Mutex::new(());

/// Sends n Backspace keystrokes with inter-key sleeps so the OS registers
/// each one individually.
fn erase_trigger(delete_count: usize) {
    debug!("Injecting {} backspaces", delete_count);
    for _ in 0..delete_count {
        let _ = simulate(&EventType::KeyPress(Key::Backspace));
        let _ = simulate(&EventType::KeyRelease(Key::Backspace));
        thread::sleep(Duration::from_millis(3));
    }
}

/// Erases the typed trigger sequence and pastes the expansion payload via the
/// OS clipboard, restoring the previous clipboard contents afterwards.
///
/// This function is serialized: if a second expansion fires while the first
/// is still injecting, it queues behind the lock instead of racing.
pub fn inject_payload(payload: String, delete_count: usize) {
    // Serialize: only one injection can run at a time.
    let _guard = INJECTION_LOCK.lock().unwrap();

    // Gate: tell the hook to ignore all synthetic events we're about to send.
    IS_INJECTING.store(true, Ordering::SeqCst);

    // 1. Delete the trigger sequence
    erase_trigger(delete_count);

    // 2. Clipboard swap
    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to initialize clipboard: {}", e);
            IS_INJECTING.store(false, Ordering::SeqCst);
            return;
        }
    };

    let original_clipboard = clipboard.get_text().unwrap_or_default();

    if let Err(e) = clipboard.set_text(&payload) {
        error!("Failed to set payload onto clipboard: {}", e);
        IS_INJECTING.store(false, Ordering::SeqCst);
        return;
    }

    // 3. Give the OS time to flush the clipboard write before we simulate paste.
    //    Without this, Ctrl+V can read stale clipboard contents on Windows.
    thread::sleep(Duration::from_millis(10));

    // 4. Paste: Ctrl+V (Win/Linux) or Cmd+V (macOS)
    let modifier = if cfg!(target_os = "macos") {
        Key::MetaLeft
    } else {
        Key::ControlLeft
    };

    let _ = simulate(&EventType::KeyPress(modifier));
    let _ = simulate(&EventType::KeyPress(Key::KeyV));
    let _ = simulate(&EventType::KeyRelease(Key::KeyV));
    let _ = simulate(&EventType::KeyRelease(modifier));

    // 5. Wait for the OS to fully dispatch the paste before restoring the clipboard.
    thread::sleep(Duration::from_millis(60));

    // 6. Restore original clipboard and release the injection gate.
    let _ = clipboard.set_text(original_clipboard);
    IS_INJECTING.store(false, Ordering::SeqCst);
}
