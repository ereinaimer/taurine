use super::types::DictionaryEntry;
use crate::settings::InlineDictionaryMode;
use crate::system::paths::get_data_dir;
use rusqlite::Connection;
use std::sync::Mutex;
use tracing::debug;

static CACHED_CONN: Mutex<Option<(InlineDictionaryMode, Connection)>> = Mutex::new(None);

pub fn close_cached_connection() {
    if let Ok(mut cache) = CACHED_CONN.lock() {
        *cache = None;
    }
}

pub fn lookup_offline(word: &str) -> Option<Vec<DictionaryEntry>> {
    let mode = crate::settings::get_cached_inline_dictionary_mode();
    let file_name = match mode {
        InlineDictionaryMode::Lite => "dictionary_lite.db",
        InlineDictionaryMode::Full => "dictionary_full.db",
    };
    let db_path = get_data_dir().join("dict").join(file_name);

    if !db_path.exists() {
        return None;
    }

    let mut cache = match CACHED_CONN.lock() {
        Ok(c) => c,
        Err(_) => return None,
    };

    let should_reopen = match &*cache {
        Some((cached_mode, _)) => *cached_mode != mode,
        None => true,
    };

    if should_reopen {
        let conn = match Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to open dictionary db at {:?}: {}", db_path, e);
                return None;
            }
        };
        *cache = Some((mode, conn));
    }

    let (_, conn) = cache.as_ref().unwrap();

    let mut stmt = match conn.prepare("SELECT data FROM dictionary WHERE word = ? LIMIT 1") {
        Ok(s) => s,
        Err(_) => return None,
    };

    let result: rusqlite::Result<String> = stmt.query_row([word], |row| row.get(0));

    match result {
        Ok(json_data) => serde_json::from_str(&json_data).ok(),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{InlineDictionaryMode, set_cached_inline_dictionary_mode};
    use rusqlite::Connection;
    use tempfile::tempdir;

    #[test]
    fn test_lookup_offline_uses_cached_connections_and_reopen_on_mode_change() {
        let _lock = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempdir().unwrap();
        // SAFETY: Setting environment variable for test directory isolation.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", temp.path().to_str().unwrap()) };

        let dict_dir = temp.path().join("dict");
        std::fs::create_dir_all(&dict_dir).unwrap();

        // Create Lite DB with test word "happy"
        let lite_conn = Connection::open(dict_dir.join("dictionary_lite.db")).unwrap();
        lite_conn
            .execute(
                "CREATE TABLE dictionary (word TEXT PRIMARY KEY, data TEXT)",
                [],
            )
            .unwrap();
        lite_conn.execute("INSERT INTO dictionary (word, data) VALUES ('happy', '[{\"word\":\"happy\",\"meanings\":[]}]')", []).unwrap();
        drop(lite_conn);

        // Create Full DB with test word "happy" (different meaning) and "qualtagh"
        let full_conn = Connection::open(dict_dir.join("dictionary_full.db")).unwrap();
        full_conn
            .execute(
                "CREATE TABLE dictionary (word TEXT PRIMARY KEY, data TEXT)",
                [],
            )
            .unwrap();
        full_conn.execute("INSERT INTO dictionary (word, data) VALUES ('happy', '[{\"word\":\"happy-full\",\"meanings\":[]}]')", []).unwrap();
        full_conn.execute("INSERT INTO dictionary (word, data) VALUES ('qualtagh', '[{\"word\":\"qualtagh\",\"meanings\":[]}]')", []).unwrap();
        drop(full_conn);

        // 1. Test Lite mode lookup
        set_cached_inline_dictionary_mode(InlineDictionaryMode::Lite);
        let res = lookup_offline("happy").expect("Lite lookup failed");
        assert_eq!(res[0].word, "happy");

        // 2. Test Full mode lookup (should close Lite connection, open Full and get different result)
        set_cached_inline_dictionary_mode(InlineDictionaryMode::Full);
        let res = lookup_offline("happy").expect("Full lookup failed");
        assert_eq!(res[0].word, "happy-full");

        let res = lookup_offline("qualtagh").expect("Full rare lookup failed");
        assert_eq!(res[0].word, "qualtagh");

        // 3. Test close_cached_connection allows modifying/deleting the file (verifies cache was cleared)
        close_cached_connection();
        std::fs::remove_file(dict_dir.join("dictionary_full.db")).unwrap();
        let res = lookup_offline("qualtagh");
        assert!(res.is_none());

        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
    }
}
