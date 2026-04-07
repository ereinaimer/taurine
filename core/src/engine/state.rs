use ahash::AHashMap;
use parking_lot::RwLock;

pub struct EngineState {
    pub trigger_char: char,
    pub map: RwLock<AHashMap<String, String>>,
}

impl EngineState {
    pub fn new(trigger_char: char) -> Self {
        Self {
            trigger_char,
            map: RwLock::new(AHashMap::new()),
        }
    }

    pub fn load_snippets(&self, snippets: impl IntoIterator<Item = (String, String)>) {
        let mut write_guard = self.map.write();
        write_guard.clear();
        for (k, v) in snippets {
            write_guard.insert(k, v);
        }
    }

    pub fn fetch_expansion(&self, keyword: &str) -> Option<String> {
        // If TAURINE_DB_PATH is set, query the DB directly to respect the override.
        if std::env::var("TAURINE_DB_PATH").is_ok() {
            if let Ok(conn) = rusqlite::Connection::open(crate::paths::get_db_path())
                && let Ok(Some(action)) =
                    crate::db::crud::automations::get_action_by_trigger(&conn, keyword)
            {
                return Some(action.output);
            }
            return None;
        }

        let read_guard = self.map.read();
        read_guard.get(keyword).cloned()
    }
}
