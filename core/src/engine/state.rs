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

    fn get_raw_expansion(&self, keyword: &str) -> Option<String> {
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

    pub fn fetch_expansion(
        &self,
        keyword: &str,
    ) -> Option<crate::engine::variables::FinalExpansion> {
        // 1. Try exact match on `keyword` FIRST
        if let Some(template) = self.get_raw_expansion(keyword) {
            // Task 2.3: No-Argument Default Handling
            let args = crate::engine::variables::ArgMap::default();
            let interpolated =
                crate::engine::variables::interpolate(&template, &args, Some(keyword));
            return Some(crate::engine::variables::finalize(
                &interpolated,
                Some(keyword),
            ));
        }

        // 2. Task 2.1: Add hyphen-split fallback logic
        if let Some((base, raw_args)) = keyword.split_once('-')
            && let Some(template) = self.get_raw_expansion(base)
        {
            // Task 2.2: Hook up interpolation
            let args = crate::engine::variables::parse_args(raw_args);
            let interpolated = crate::engine::variables::interpolate(&template, &args, Some(base));
            return Some(crate::engine::variables::finalize(
                &interpolated,
                Some(base),
            ));
        }

        None
    }
}
