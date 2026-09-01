use ahash::AHashMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Trait for a source of expanded snippets.
///
/// This abstraction allows the engine to be decoupled from the data backend,
/// enabling in-memory caching for performance or direct DB fetching for debugging.
pub trait SnippetSource: Send + Sync {
    fn get_action(&self, keyword: &str) -> Option<crate::db::crud::TriggerAction>;

    /// Optional: Reloads the source with new snippets.
    /// Default implementation does nothing (for read-only sources).
    fn load_actions(&self, _actions: Vec<(String, crate::db::crud::TriggerAction)>) {}
}

/// A source that stores snippets in an in-memory hash map.
pub struct MemorySource {
    map: RwLock<AHashMap<String, crate::db::crud::TriggerAction>>,
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
        for (k, v) in actions {
            write_guard.insert(k, v);
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
        self.map.read().get(keyword).cloned()
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

/// A source that switches between a database source and a fallback source
/// based on the presence of the `TAURINE_DB_PATH` environment variable.
pub struct AdaptiveSource {
    fallback: Arc<dyn SnippetSource>,
    is_db_override: bool,
}

impl AdaptiveSource {
    pub fn new(fallback: Arc<dyn SnippetSource>) -> Self {
        let is_db_override = std::env::var("TAURINE_DB_PATH").is_ok();
        Self {
            fallback,
            is_db_override,
        }
    }
}

impl SnippetSource for AdaptiveSource {
    fn get_action(&self, keyword: &str) -> Option<crate::db::crud::TriggerAction> {
        if self.is_db_override {
            return DatabaseSource.get_action(keyword);
        }
        self.fallback.get_action(keyword)
    }

    fn load_actions(&self, actions: Vec<(String, crate::db::crud::TriggerAction)>) {
        self.fallback.load_actions(actions);
    }
}
