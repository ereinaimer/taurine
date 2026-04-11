use arboard::Clipboard;
use std::cell::RefCell;

thread_local! {
    static MOCK_CLIPBOARD: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Resolves the `{clipboard}` system variable.
pub fn resolve(key: &str) -> Option<String> {
    if key != "clipboard" {
        return None;
    }

    // Check mock first for tests
    if let Some(mocked) = MOCK_CLIPBOARD.with(|m| m.borrow().clone()) {
        return Some(mocked);
    }

    // Read from system clipboard
    match Clipboard::new() {
        Ok(mut clip) => clip.get_text().ok(),
        Err(e) => {
            tracing::error!("Failed to initialize clipboard: {}", e);
            None
        }
    }
}

/// Sets the mock clipboard content for the current thread.
/// Used only for testing.
#[cfg(test)]
pub fn set_mock_clipboard(text: Option<String>) {
    MOCK_CLIPBOARD.with(|m| *m.borrow_mut() = text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_clipboard_mocked() {
        set_mock_clipboard(Some("mocked content".to_string()));
        assert_eq!(resolve("clipboard"), Some("mocked content".to_string()));
        set_mock_clipboard(None);
    }

    #[test]
    fn test_resolve_not_clipboard_key() {
        assert_eq!(resolve("not_clipboard"), None);
    }
}
