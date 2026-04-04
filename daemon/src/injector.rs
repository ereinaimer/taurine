use arboard::Clipboard;
use rdev::{EventType, Key, simulate};
use std::thread;
use std::time::Duration;
use tracing::{debug, error};

/// Sends n Backspace Keystrokes safely formatted with sleeps.
pub fn erase_trigger(delete_count: usize) {
    debug!("Injecting {} backspaces", delete_count);
    for _ in 0..delete_count {
        let _ = simulate(&EventType::KeyPress(Key::Backspace));
        let _ = simulate(&EventType::KeyRelease(Key::Backspace));
        // Give OS time to process event sequentially
        thread::sleep(Duration::from_millis(3));
    }
}

/// Executes the payload injection utilizing OS clipboards seamlessly.
pub fn inject_payload(payload: String, delete_count: usize) {
    // 1. Delete sequence
    erase_trigger(delete_count);

    // 2. Clipboard setup
    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to initialize clipboard: {}", e);
            return;
        }
    };

    let original_clipboard = clipboard.get_text().unwrap_or_default();

    if let Err(e) = clipboard.set_text(payload) {
        error!("Failed to set payload onto clipboard: {}", e);
        return;
    }

    // 3. Paste simulation (CMD+V mac / CTRL+V win linux)
    let modifier = if cfg!(target_os = "macos") {
        Key::MetaLeft
    } else {
        Key::ControlLeft
    };

    let _ = simulate(&EventType::KeyPress(modifier));
    let _ = simulate(&EventType::KeyPress(Key::KeyV));
    let _ = simulate(&EventType::KeyRelease(Key::KeyV));
    let _ = simulate(&EventType::KeyRelease(modifier));

    // 4. Wait for OS desktop environment strictly to dispatch paste operations async correctly
    thread::sleep(Duration::from_millis(50));

    // 5. Cleanup
    let _ = clipboard.set_text(original_clipboard);
}
