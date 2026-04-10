use ahash::AHashMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Trait for a source of expanded snippets.
///
/// This abstraction allows the engine to be decoupled from the data backend,
/// enabling in-memory caching for performance or direct DB fetching for debugging.
pub trait SnippetSource: Send + Sync {
    fn get_snippet(&self, keyword: &str) -> Option<String>;

    /// Optional: Reloads the source with new snippets.
    /// Default implementation does nothing (for read-only sources).
    fn load_snippets(&self, _snippets: Vec<(String, String)>) {}
}

/// A source that stores snippets in an in-memory hash map.
pub struct MemorySource {
    map: RwLock<AHashMap<String, String>>,
}

impl MemorySource {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(AHashMap::new()),
        }
    }

    /// Reloads the in-memory cache with a new set of snippets.
    pub fn load_snippets(&self, snippets: impl IntoIterator<Item = (String, String)>) {
        let mut write_guard = self.map.write();
        write_guard.clear();
        for (k, v) in snippets {
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
    fn get_snippet(&self, keyword: &str) -> Option<String> {
        self.map.read().get(keyword).cloned()
    }

    fn load_snippets(&self, snippets: Vec<(String, String)>) {
        self.load_snippets(snippets);
    }
}

/// A source that queries the SQLite database directly for each expansion.
pub struct DatabaseSource;

impl SnippetSource for DatabaseSource {
    fn get_snippet(&self, keyword: &str) -> Option<String> {
        if let Ok(conn) = rusqlite::Connection::open(crate::paths::get_db_path())
            && let Ok(Some(action)) =
                crate::db::crud::automations::get_action_by_trigger(&conn, keyword)
        {
            return Some(action.output);
        }
        None
    }
}

/// A source that switches between a database source and a fallback source
/// based on the presence of the `TAURINE_DB_PATH` environment variable.
pub struct AdaptiveSource {
    fallback: Arc<dyn SnippetSource>,
}

impl AdaptiveSource {
    pub fn new(fallback: Arc<dyn SnippetSource>) -> Self {
        Self { fallback }
    }
}

impl SnippetSource for AdaptiveSource {
    fn get_snippet(&self, keyword: &str) -> Option<String> {
        if std::env::var("TAURINE_DB_PATH").is_ok() {
            return DatabaseSource.get_snippet(keyword);
        }
        self.fallback.get_snippet(keyword)
    }

    fn load_snippets(&self, snippets: Vec<(String, String)>) {
        self.fallback.load_snippets(snippets);
    }
}
