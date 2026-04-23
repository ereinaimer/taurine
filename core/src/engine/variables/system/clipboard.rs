use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, OnceLock, RwLock};

const HISTORY_CAPACITY: usize = 3;
pub const MAX_PAYLOAD_BYTES: usize = 65_536;

thread_local! {
    static MOCK_CLIPBOARD: RefCell<Option<VecDeque<String>>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
pub struct ClipboardManager {
    history: Arc<RwLock<VecDeque<String>>>,
}

impl Default for ClipboardManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardManager {
    pub fn new() -> Self {
        Self {
            // The daemon listener takes short write locks while the parser clones a single slot
            // under a read lock, so clipboard history stays in-memory and contention stays low.
            history: Arc::new(RwLock::new(VecDeque::with_capacity(HISTORY_CAPACITY))),
        }
    }

    pub fn record_text(&self, text: String) -> bool {
        if text.len() > MAX_PAYLOAD_BYTES || text.is_empty() {
            return false;
        }

        let Ok(mut history) = self.history.write() else {
            tracing::error!("Clipboard history write lock poisoned");
            return false;
        };

        if history.front().is_some_and(|current| current == &text) {
            return false;
        }

        history.push_front(text);
        while history.len() > HISTORY_CAPACITY {
            history.pop_back();
        }
        true
    }

    pub fn get(&self, index: usize) -> Option<String> {
        let Ok(history) = self.history.read() else {
            tracing::error!("Clipboard history read lock poisoned");
            return None;
        };

        history.get(index).cloned()
    }
}

static CLIPBOARD_MANAGER: OnceLock<ClipboardManager> = OnceLock::new();

pub fn clipboard_manager() -> &'static ClipboardManager {
    CLIPBOARD_MANAGER.get_or_init(ClipboardManager::new)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardKey {
    Valid(usize),
    OutOfBounds,
    Malformed,
}

fn parse_clipboard_key(key: &str) -> Option<ClipboardKey> {
    if matches!(key, "clipboard" | "clipboard(0)") {
        return Some(ClipboardKey::Valid(0));
    }

    let inner = key
        .strip_prefix("clipboard(")
        .and_then(|rest| rest.strip_suffix(')'))?;

    // Malformed arguments stay literal at the interpolation layer instead of panicking or
    // accidentally flowing into transformer fallback paths.
    match inner.parse::<usize>() {
        Ok(index) if index < HISTORY_CAPACITY => Some(ClipboardKey::Valid(index)),
        Ok(_) => Some(ClipboardKey::OutOfBounds),
        Err(_) => Some(ClipboardKey::Malformed),
    }
}

pub fn is_clipboard_key(key: &str) -> bool {
    parse_clipboard_key(key).is_some()
}

/// Resolves the `[clipboard]` system variable family from the in-memory history buffer.
pub fn resolve(key: &str) -> Option<String> {
    let index = match parse_clipboard_key(key)? {
        ClipboardKey::Valid(index) => index,
        ClipboardKey::OutOfBounds => return Some(String::new()),
        ClipboardKey::Malformed => return None,
    };

    if let Some(mocked) = MOCK_CLIPBOARD.with(|m| m.borrow().clone()) {
        return Some(mocked.get(index).cloned().unwrap_or_default());
    }

    Some(clipboard_manager().get(index).unwrap_or_default())
}

/// Sets the mock clipboard content for the current thread.
/// Used only for testing.
#[cfg(test)]
pub fn set_mock_clipboard(text: Option<String>) {
    set_mock_clipboard_history(text.into_iter().collect::<Vec<_>>());
}

/// Sets a mock clipboard history for the current thread.
/// Used only for testing.
#[cfg(test)]
pub fn set_mock_clipboard_history(history: Vec<String>) {
    let queue = if history.is_empty() {
        None
    } else {
        Some(history.into_iter().collect())
    };
    MOCK_CLIPBOARD.with(|m| *m.borrow_mut() = queue);
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

    #[test]
    fn test_resolve_clipboard_history_indices_from_mock() {
        set_mock_clipboard_history(vec![
            "current".to_string(),
            "previous".to_string(),
            "oldest".to_string(),
        ]);

        assert_eq!(resolve("clipboard"), Some("current".to_string()));
        assert_eq!(resolve("clipboard(0)"), Some("current".to_string()));
        assert_eq!(resolve("clipboard(1)"), Some("previous".to_string()));
        assert_eq!(resolve("clipboard(2)"), Some("oldest".to_string()));
        assert_eq!(resolve("clipboard(9)"), Some(String::new()));
        assert_eq!(resolve("clipboard(abc)"), None);
        assert_eq!(resolve("clipboard(-1)"), None);

        set_mock_clipboard(None);
    }

    #[test]
    fn test_resolve_clipboard_history_missing_slot_is_empty_string() {
        set_mock_clipboard_history(vec!["current".to_string()]);
        assert_eq!(resolve("clipboard(2)"), Some(String::new()));
        set_mock_clipboard(None);
    }

    #[test]
    fn test_clipboard_manager_ignores_empty_large_and_duplicate_payloads() {
        let manager = ClipboardManager::new();

        assert!(manager.record_text("alpha".to_string()));
        assert!(!manager.record_text(String::new()));
        assert!(!manager.record_text("alpha".to_string()));
        assert!(!manager.record_text("x".repeat(MAX_PAYLOAD_BYTES + 1)));

        assert_eq!(manager.get(0), Some("alpha".to_string()));
        assert_eq!(manager.get(1), None);
    }

    #[test]
    fn test_clipboard_manager_keeps_three_items_in_ring_order() {
        let manager = ClipboardManager::new();

        assert!(manager.record_text("one".to_string()));
        assert!(manager.record_text("two".to_string()));
        assert!(manager.record_text("three".to_string()));
        assert!(manager.record_text("four".to_string()));

        assert_eq!(manager.get(0), Some("four".to_string()));
        assert_eq!(manager.get(1), Some("three".to_string()));
        assert_eq!(manager.get(2), Some("two".to_string()));
        assert_eq!(manager.get(3), None);
    }
}
