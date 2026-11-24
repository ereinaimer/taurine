use ahash::AHashMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Trait for a source of expanded snippets.
///
/// This abstraction allows the engine to be decoupled from the data backend,
/// enabling in-memory caching for performance or direct DB fetching for debugging.
pub trait SnippetSource: Send + Sync {
    fn get_action(&self, keyword: &str) -> Option<crate::db::crud::TriggerAction>;

    /// Existence check without cloning the action. Hot paths use this to
    /// scan candidate keywords; the full clone happens only for the winner.
    fn has_action(&self, keyword: &str) -> bool {
        self.get_action(keyword).is_some()
    }

    /// Optional: Reloads the source with new snippets.
    /// Default implementation does nothing (for read-only sources).
    fn load_actions(&self, _actions: Vec<(String, crate::db::crud::TriggerAction)>) {}
}

/// A source that stores snippets in an in-memory hash map.
pub struct MemorySource {
    map: RwLock<AHashMap<String, Arc<crate::db::crud::TriggerAction>>>,
}

impl MemorySource {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(AHashMap::new()),
        }
    }

    /// Reloads the in-memory cache with a new set of snippets.
    pub fn load_actions(
        &self,
        actions: impl IntoIterator<Item = (String, crate::db::crud::TriggerAction)>,
    ) {
        let mut write_guard = self.map.write();
        write_guard.clear();
        // Aliases of one trigger share a single Arc'd action instead of k
        // owned copies of output + script_binary. Rows without a parent id
        // (in-memory test triggers) are never grouped: an empty key would
        // otherwise collapse unrelated actions into one.
        let mut by_parent: AHashMap<String, Arc<crate::db::crud::TriggerAction>> = AHashMap::new();
        for (keyword, action) in actions {
            let shared = if action.parent_id.is_empty() {
                Arc::new(action)
            } else {
                Arc::clone(
                    by_parent
                        .entry(action.parent_id.clone())
                        .or_insert_with(|| Arc::new(action)),
                )
            };
            write_guard.insert(keyword, shared);
        }
    }
}

impl Default for MemorySource {
    fn default() -> Self {
        Self::new()
    }
}

impl SnippetSource for MemorySource {
    fn get_action(&self, keyword: &str) -> Option<crate::db::crud::TriggerAction> {
        self.map
            .read()
            .get(keyword)
            .map(|action| (**action).clone())
    }

    fn has_action(&self, keyword: &str) -> bool {
        self.map.read().contains_key(keyword)
    }

    fn load_actions(&self, actions: Vec<(String, crate::db::crud::TriggerAction)>) {
        self.load_actions(actions);
    }
}

/// A source that queries the SQLite database directly for each expansion.
pub struct DatabaseSource;

impl SnippetSource for DatabaseSource {
    fn get_action(&self, keyword: &str) -> Option<crate::db::crud::TriggerAction> {
        if let Ok(conn) = crate::db::get_conn()
            && let Ok(Some(action)) =
                crate::db::crud::triggers::get_action_by_trigger(&conn, keyword)
        {
            return Some(action);
        }
        None
    }
}

/// A source that prefers in-memory actions and falls back to the database
/// when the `NITE_DATA_DIR` DEV override is present (tests).
pub struct AdaptiveSource {
    fallback: Arc<dyn SnippetSource>,
}

impl AdaptiveSource {
    pub fn new(fallback: Arc<dyn SnippetSource>) -> Self {
        Self { fallback }
    }
}

impl SnippetSource for AdaptiveSource {
    fn get_action(&self, keyword: &str) -> Option<crate::db::crud::TriggerAction> {
        if let Some(action) = self.fallback.get_action(keyword) {
            return Some(action);
        }
        if crate::paths::dev_env_var("NITE_DATA_DIR").is_some() {
            return DatabaseSource.get_action(keyword);
        }
        None
    }

    fn has_action(&self, keyword: &str) -> bool {
        if self.fallback.has_action(keyword) {
            return true;
        }
        crate::paths::dev_env_var("NITE_DATA_DIR").is_some() && DatabaseSource.has_action(keyword)
    }

    fn load_actions(&self, actions: Vec<(String, crate::db::crud::TriggerAction)>) {
        self.fallback.load_actions(actions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(parent: &str, output: &str) -> crate::db::crud::TriggerAction {
        let mut action = crate::db::crud::TriggerAction::text(output);
        action.parent_id = parent.to_string();
        action
    }

    fn output_of(source: &MemorySource, keyword: &str) -> Option<String> {
        source.get_action(keyword).map(|action| action.output)
    }

    #[test]
    fn aliases_of_one_trigger_share_one_action_copy() {
        let source = MemorySource::new();
        source.load_actions(vec![
            ("one".to_string(), action("p1", "shared")),
            ("two".to_string(), action("p1", "shared")),
            ("solo".to_string(), action("p2", "alone")),
        ]);

        assert!(source.has_action("one"));
        assert!(source.has_action("two"));
        assert!(source.has_action("solo"));
        assert!(!source.has_action("missing"));
        assert_eq!(output_of(&source, "one").as_deref(), Some("shared"));
        assert_eq!(output_of(&source, "two").as_deref(), Some("shared"));
        assert_eq!(output_of(&source, "solo").as_deref(), Some("alone"));
    }

    #[test]
    fn returned_actions_are_independent_copies() {
        let source = MemorySource::new();
        source.load_actions(vec![
            ("one".to_string(), action("p1", "shared")),
            ("two".to_string(), action("p1", "shared")),
        ]);

        let mut first = source.get_action("one").expect("action present");
        first.output = "mutated".to_string();

        assert_eq!(output_of(&source, "two").as_deref(), Some("shared"));
    }

    #[test]
    fn empty_parent_ids_never_alias_each_other() {
        let source = MemorySource::new();
        source.load_actions(vec![
            ("a".to_string(), action("", "first")),
            ("b".to_string(), action("", "second")),
        ]);

        assert_eq!(output_of(&source, "a").as_deref(), Some("first"));
        assert_eq!(output_of(&source, "b").as_deref(), Some("second"));
    }
}
