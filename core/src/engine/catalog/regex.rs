use std::sync::{OnceLock, RwLock};

use crate::db::crud::TriggerAction;
use crate::engine::catalog::is_app_allowed;
pub struct RegexCatalog {
    snapshot: RwLock<RegexCatalogSnapshot>,
}

#[derive(Default)]
struct RegexCatalogSnapshot {
    entries: Vec<ParsedRegexAction>,
}

#[derive(Clone)]
struct ParsedRegexAction {
    pattern: String,
    regex: OnceLock<Result<regex::Regex, ()>>,
    action: TriggerAction,
}

pub const MAX_REGEX_PATTERN_BYTES: usize = 64 * 1024; // 64 KiB
pub const MAX_REGEX_PATTERNS_COUNT: usize = 512;

impl RegexCatalog {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(RegexCatalogSnapshot::default()),
        }
    }

    pub fn is_empty(&self) -> bool {
        if let Ok(guard) = self.snapshot.read() {
            return guard.entries.is_empty();
        }
        true
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, TriggerAction)>) {
        let mut entries = Vec::new();
        for (pattern, action) in actions {
            if pattern.len() > MAX_REGEX_PATTERN_BYTES {
                continue;
            }
            entries.push(ParsedRegexAction {
                pattern,
                regex: OnceLock::new(),
                action,
            });
            if entries.len() >= MAX_REGEX_PATTERNS_COUNT {
                break;
            }
        }
        if let Ok(mut guard) = self.snapshot.write() {
            guard.entries = entries;
        }
    }

    pub fn match_action(
        &self,
        buffer_string: &str,
        active_window: Option<&str>,
    ) -> Option<(String, TriggerAction, Vec<String>)> {
        let guard = self.snapshot.read().ok()?;
        for entry in &guard.entries {
            let re = match entry
                .regex
                .get_or_init(|| regex::Regex::new(&entry.pattern).map_err(|_| ()))
            {
                Ok(re) => re,
                Err(_) => continue,
            };
            if is_app_allowed(&entry.action, active_window)
                && let Some(m) = re.find_iter(buffer_string).last()
                && m.end() == buffer_string.len()
                && !m.as_str().is_empty()
            {
                let matched_str = m.as_str();
                let mut captures_list = Vec::new();
                if let Some(caps) = re.captures(matched_str) {
                    for i in 1..caps.len() {
                        let val = caps
                            .get(i)
                            .map(|c| c.as_str().to_string())
                            .unwrap_or_default();
                        captures_list.push(val);
                    }
                }
                return Some((matched_str.to_string(), entry.action.clone(), captures_list));
            }
        }
        None
    }
}

impl Default for RegexCatalog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regex_catalog_ignores_overlong_patterns() {
        let catalog = RegexCatalog::new();
        let overlong_pattern = "a".repeat(MAX_REGEX_PATTERN_BYTES + 1);
        let action = TriggerAction::text("test");
        catalog.load_actions(vec![(overlong_pattern, action)]);
        assert!(catalog.is_empty());
    }

    #[test]
    fn test_regex_catalog_caps_total_patterns() {
        let catalog = RegexCatalog::new();
        let action = TriggerAction::text("test");
        let items =
            (0..MAX_REGEX_PATTERNS_COUNT + 10).map(|i| (format!("pat_{i}"), action.clone()));
        catalog.load_actions(items);

        let guard = catalog.snapshot.read().unwrap();
        assert_eq!(guard.entries.len(), MAX_REGEX_PATTERNS_COUNT);
    }
}
