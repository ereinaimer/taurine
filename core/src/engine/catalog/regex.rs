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
            entries.push(ParsedRegexAction {
                pattern,
                regex: OnceLock::new(),
                action,
            });
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
