use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, OnceLock, RwLock};

const HISTORY_CAPACITY: usize = 3;
pub const MAX_PAYLOAD_BYTES: usize = 1_048_576; // 1MB

thread_local! {
    static MOCK_CLIP: RefCell<Option<VecDeque<String>>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
pub struct ClipEntry {
    pub text: String,
    pub timestamp: std::time::Instant,
}

#[derive(Clone, Debug)]
pub struct ClipManager {
    history: Arc<RwLock<VecDeque<ClipEntry>>>,
}

impl Default for ClipManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipManager {
    pub fn new() -> Self {
        Self {
            // The daemon listener takes short write locks while the parser clones a single slot
            // under a read lock, so clip history stays in-memory and contention stays low.
            history: Arc::new(RwLock::new(VecDeque::with_capacity(HISTORY_CAPACITY))),
        }
    }

    pub fn record_text(&self, text: String) -> bool {
        if !crate::settings::get_cached_clipboard_history_enabled() {
            return false;
        }
        if text.len() > MAX_PAYLOAD_BYTES || text.is_empty() {
            return false;
        }

        self.prune_expired();

        let Ok(mut history) = self.history.write() else {
            tracing::error!("clip history write lock poisoned");
            return false;
        };

        if history.front().is_some_and(|current| current.text == text) {
            return false;
        }

        history.push_front(ClipEntry {
            text,
            timestamp: std::time::Instant::now(),
        });
        while history.len() > HISTORY_CAPACITY {
            history.pop_back();
        }
        true
    }

    pub fn get(&self, index: usize) -> Option<String> {
        if !crate::settings::get_cached_clipboard_history_enabled() {
            return None;
        }
        self.prune_expired();
        let Ok(history) = self.history.read() else {
            tracing::error!("clip history read lock poisoned");
            return None;
        };

        history.get(index).map(|entry| entry.text.clone())
    }

    pub fn clear(&self) {
        if let Ok(mut history) = self.history.write() {
            history.clear();
        }
    }

    fn prune_expired(&self) {
        let retention_secs = crate::settings::get_cached_clipboard_history_retention_secs();
        let Some(limit) = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(retention_secs as u64))
        else {
            return;
        };

        let needs_prune = if let Ok(history) = self.history.read() {
            history.iter().any(|entry| entry.timestamp < limit)
        } else {
            false
        };

        if needs_prune && let Ok(mut history) = self.history.write() {
            history.retain(|entry| entry.timestamp >= limit);
        }
    }
}

static CLIP_MANAGER: OnceLock<ClipManager> = OnceLock::new();

pub fn clip_manager() -> &'static ClipManager {
    CLIP_MANAGER.get_or_init(ClipManager::new)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipKey {
    Valid(usize),
    OutOfBounds,
    Malformed,
}

fn parse_clip_key(key: &str) -> Option<ClipKey> {
    if matches!(key, "clip" | "clip(0)") {
        return Some(ClipKey::Valid(0));
    }

    let inner = key
        .strip_prefix("clip(")
        .and_then(|rest| rest.strip_suffix(')'))?;

    let inner = crate::engine::variables::system::strip_argument_quotes(inner);

    // Malformed arguments stay literal at the interpolation layer instead of panicking or
    // accidentally flowing into transformer fallback paths.
    match inner.parse::<usize>() {
        Ok(index) if index < HISTORY_CAPACITY => Some(ClipKey::Valid(index)),
        Ok(_) => Some(ClipKey::OutOfBounds),
        Err(_) => Some(ClipKey::Malformed),
    }
}

pub fn is_clip_key(key: &str) -> bool {
    parse_clip_key(key).is_some()
}

/// Resolves the `[clip]` system variable family from the in-memory history buffer.
pub fn resolve(key: &str) -> Option<String> {
    let index = match parse_clip_key(key)? {
        ClipKey::Valid(index) => index,
        ClipKey::OutOfBounds => return Some(String::new()),
        ClipKey::Malformed => return None,
    };

    if let Some(mocked) = MOCK_CLIP.with(|m| m.borrow().clone()) {
        return Some(mocked.get(index).cloned().unwrap_or_default());
    }

    Some(clip_manager().get(index).unwrap_or_default())
}

/// Sets the mock clip content for the current thread.
/// Used only for testing.
#[cfg(test)]
pub fn set_mock_clip(text: Option<String>) {
    set_mock_clip_history(text.into_iter().collect::<Vec<_>>());
}

/// Sets a mock clip history for the current thread.
/// Used only for testing.
#[cfg(test)]
pub fn set_mock_clip_history(history: Vec<String>) {
    let queue = if history.is_empty() {
        None
    } else {
        Some(history.into_iter().collect())
    };
    MOCK_CLIP.with(|m| *m.borrow_mut() = queue);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_clip_mocked() {
        set_mock_clip(Some("mocked content".to_string()));
        assert_eq!(resolve("clip"), Some("mocked content".to_string()));
        set_mock_clip(None);
    }

    #[test]
    fn test_resolve_not_clip_key() {
        assert_eq!(resolve("not_clip"), None);
    }

    #[test]
    fn test_resolve_clip_history_indices_from_mock() {
        set_mock_clip_history(vec![
            "current".to_string(),
            "previous".to_string(),
            "oldest".to_string(),
        ]);

        assert_eq!(resolve("clip"), Some("current".to_string()));
        assert_eq!(resolve("clip(0)"), Some("current".to_string()));
        assert_eq!(resolve("clip(1)"), Some("previous".to_string()));
        assert_eq!(resolve("clip(2)"), Some("oldest".to_string()));
        assert_eq!(resolve("clip(9)"), Some(String::new()));
        assert_eq!(resolve("clip(abc)"), None);
        assert_eq!(resolve("clip(-1)"), None);

        set_mock_clip(None);
    }

    #[test]
    fn test_resolve_clip_history_missing_slot_is_empty_string() {
        set_mock_clip_history(vec!["current".to_string()]);
        assert_eq!(resolve("clip(2)"), Some(String::new()));
        set_mock_clip(None);
    }

    #[test]
    fn test_clip_manager_ignores_empty_large_and_duplicate_payloads() {
        let manager = ClipManager::new();

        assert!(manager.record_text("alpha".to_string()));
        assert!(!manager.record_text(String::new()));
        assert!(!manager.record_text("alpha".to_string()));
        assert!(!manager.record_text("x".repeat(MAX_PAYLOAD_BYTES + 1)));

        assert_eq!(manager.get(0), Some("alpha".to_string()));
        assert_eq!(manager.get(1), None);
    }

    #[test]
    fn test_clip_manager_keeps_three_items_in_ring_order() {
        let manager = ClipManager::new();

        assert!(manager.record_text("one".to_string()));
        assert!(manager.record_text("two".to_string()));
        assert!(manager.record_text("three".to_string()));
        assert!(manager.record_text("four".to_string()));

        assert_eq!(manager.get(0), Some("four".to_string()));
        assert_eq!(manager.get(1), Some("three".to_string()));
        assert_eq!(manager.get(2), Some("two".to_string()));
        assert_eq!(manager.get(3), None);
    }

    #[test]
    fn test_clip_manager_respects_history_toggle() {
        let manager = ClipManager::new();

        crate::settings::set_cached_clipboard_history_enabled(false);
        assert!(!manager.record_text("hello".to_string()));
        assert_eq!(manager.get(0), None);

        crate::settings::set_cached_clipboard_history_enabled(true);
        assert!(manager.record_text("hello".to_string()));
        assert_eq!(manager.get(0), Some("hello".to_string()));
    }

    #[test]
    fn test_clip_manager_clears_history() {
        let manager = ClipManager::new();
        crate::settings::set_cached_clipboard_history_enabled(true);
        manager.record_text("one".to_string());
        manager.record_text("two".to_string());

        manager.clear();
        assert_eq!(manager.get(0), None);
    }

    #[test]
    fn test_clip_manager_prunes_expired_entries() {
        let manager = ClipManager::new();
        crate::settings::set_cached_clipboard_history_enabled(true);

        // 0 seconds retention means everything is expired instantly
        crate::settings::set_cached_clipboard_history_retention_secs(0);
        manager.record_text("expired".to_string());

        assert_eq!(manager.get(0), None);

        // Set to 5 seconds and test that items are kept then expired
        crate::settings::set_cached_clipboard_history_retention_secs(5);
        manager.record_text("fresh".to_string());
        assert_eq!(manager.get(0), Some("fresh".to_string()));
    }
}
