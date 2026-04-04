use arboard::Clipboard;
use rdev::{EventType, Key, simulate};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tracing::{debug, error};

/// Shared flag: set to `true` while we are simulating keystrokes so the
/// hook callback can ignore those synthetic events and avoid feeding them
/// back into the evaluator.
pub static IS_INJECTING: AtomicBool = AtomicBool::new(false);

/// Sends n Backspace keystrokes with inter-key sleeps so the OS registers
/// each one individually.
pub fn erase_trigger(delete_count: usize) {
    debug!("Injecting {} backspaces", delete_count);
    for _ in 0..delete_count {
        let _ = simulate(&EventType::KeyPress(Key::Backspace));
        let _ = simulate(&EventType::KeyRelease(Key::Backspace));
        thread::sleep(Duration::from_millis(3));
    }
}

/// Erases the typed trigger sequence and pastes the expansion payload via the
/// OS clipboard, restoring the previous clipboard contents afterwards.
pub fn inject_payload(payload: String, delete_count: usize) {
    // Gate: all OS-level simulation runs under this flag so the hook ignores them.
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

    if let Err(e) = clipboard.set_text(payload) {
        error!("Failed to set payload onto clipboard: {}", e);
        IS_INJECTING.store(false, Ordering::SeqCst);
        return;
    }

    // 3. Paste: Ctrl+V (Win/Linux) or Cmd+V (macOS)
    let modifier = if cfg!(target_os = "macos") {
        Key::MetaLeft
    } else {
        Key::ControlLeft
    };

    let _ = simulate(&EventType::KeyPress(modifier));
    let _ = simulate(&EventType::KeyPress(Key::KeyV));
    let _ = simulate(&EventType::KeyRelease(Key::KeyV));
    let _ = simulate(&EventType::KeyRelease(modifier));

    // 4. Wait for the OS clipboard to dispatch the paste asynchronously
    thread::sleep(Duration::from_millis(50));

    // 5. Restore original clipboard and release the injection gate
    let _ = clipboard.set_text(original_clipboard);
    IS_INJECTING.store(false, Ordering::SeqCst);
}
