use super::types::DictionaryEntry;
use crate::settings::InlineDictionaryMode;
use crate::system::paths::get_data_dir;
use rusqlite::Connection;
use std::sync::Mutex;
use tracing::debug;

static CACHED_CONN: Mutex<Option<(InlineDictionaryMode, Connection)>> = Mutex::new(None);

#[cfg(test)]
static MOCK_OFFLINE_WORDS: std::sync::RwLock<Option<std::collections::HashSet<String>>> =
    std::sync::RwLock::new(None);

#[cfg(test)]
pub fn set_mock_offline_words(words: Option<Vec<&str>>) {
    if let Ok(mut lock) = MOCK_OFFLINE_WORDS.write() {
        *lock = words.map(|w| w.into_iter().map(|s| s.to_lowercase()).collect());
    }
}

pub fn close_cached_connection() {
    if let Ok(mut cache) = CACHED_CONN.lock() {
        *cache = None;
    }
}

fn get_connection(
    cache: &mut Option<(InlineDictionaryMode, Connection)>,
    mode: InlineDictionaryMode,
    allow_fallback: bool,
) -> Option<&Connection> {
    if let Some((cached_mode, _)) = cache
        && *cached_mode == mode
    {
        return cache.as_ref().map(|(_, conn)| conn);
    }

    let file_name = match mode {
        InlineDictionaryMode::Lite => "dictionary_lite.db",
        InlineDictionaryMode::Full => "dictionary_full.db",
    };
    let mut db_path = get_data_dir().join("dict").join(file_name);

    if !db_path.exists() {
        if allow_fallback {
            let alt_name = match mode {
                InlineDictionaryMode::Lite => "dictionary_full.db",
                InlineDictionaryMode::Full => "dictionary_lite.db",
            };
            let alt_path = get_data_dir().join("dict").join(alt_name);
            if alt_path.exists() {
                db_path = alt_path;
            } else {
                return None;
            }
        } else {
            return None;
        }
    }

    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            debug!("Failed to open dictionary db at {:?}: {}", db_path, e);
            return None;
        }
    };
    *cache = Some((mode, conn));
    cache.as_ref().map(|(_, conn)| conn)
}

pub fn lookup_offline(word: &str) -> Option<Vec<DictionaryEntry>> {
    let mode = crate::settings::get_cached_inline_dictionary_mode();
    let mut cache = match CACHED_CONN.lock() {
        Ok(c) => c,
        Err(_) => return None,
    };
    let conn = get_connection(&mut cache, mode, false)?;

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

/// Checks whether a word exists as an authentic English vocabulary word in Taurine's offline dictionary.
///
/// English vocabulary (nouns, verbs, adjectives) is stored in lowercase in the dictionary,
/// whereas isolated proper names (e.g. "Darin", "Doreen") are stored only in Titlecase.
/// Querying by lowercase ensures legitimate English words are granted lexicon immunity
/// from phonetic hijacking, while misrecognized personal names or artifacts can be corrected.
pub fn is_offline_word(word: &str) -> bool {
    let trimmed = word.trim();
    if trimmed.is_empty() {
        return false;
    }

    #[cfg(test)]
    {
        if let Ok(lock) = MOCK_OFFLINE_WORDS.read()
            && let Some(mock) = &*lock
        {
            return mock.contains(&trimmed.to_lowercase());
        }
    }

    let mode = crate::settings::get_cached_inline_dictionary_mode();
    let mut cache = match CACHED_CONN.lock() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let conn = match get_connection(&mut cache, mode, true) {
        Some(c) => c,
        None => return false,
    };

    let mut stmt = match conn.prepare_cached("SELECT 1 FROM dictionary WHERE word = ? LIMIT 1") {
        Ok(s) => s,
        Err(_) => return false,
    };

    let lower = trimmed.to_lowercase();
    stmt.exists(rusqlite::params![lower]).unwrap_or(false)
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

    #[test]
    fn test_is_offline_word_checks_database_and_casing() {
        let _lock = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempdir().unwrap();
        // SAFETY: Setting environment variable for test directory isolation under TEST_LOCK.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", temp.path().to_str().unwrap()) };

        let dict_dir = temp.path().join("dict");
        std::fs::create_dir_all(&dict_dir).unwrap();

        let db_path = dict_dir.join("dictionary_lite.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE dictionary (word TEXT PRIMARY KEY, data TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO dictionary (word, data) VALUES ('train', '[]')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO dictionary (word, data) VALUES ('earn', '[]')",
            [],
        )
        .unwrap();
        // Insert a proper name that ONLY exists in Titlecase (like Darin)
        conn.execute(
            "INSERT INTO dictionary (word, data) VALUES ('Darin', '[]')",
            [],
        )
        .unwrap();
        drop(conn);

        set_cached_inline_dictionary_mode(InlineDictionaryMode::Lite);
        close_cached_connection();

        // Exact match on common English vocabulary
        assert!(is_offline_word("train"));
        assert!(is_offline_word("Train"));
        assert!(is_offline_word("TRAIN"));
        assert!(is_offline_word("earn"));
        assert!(is_offline_word("Earn"));

        // Isolated proper name ("Darin") is NOT considered a common English vocabulary word
        assert!(!is_offline_word("darin"));
        assert!(!is_offline_word("Darin"));

        // Words not in dictionary
        assert!(!is_offline_word("dorin"));
        assert!(!is_offline_word("erein"));
        assert!(!is_offline_word("toring"));
        assert!(!is_offline_word(""));
        assert!(!is_offline_word("   "));

        // Test mock override seam
        set_mock_offline_words(Some(vec!["custom_mock_word"]));
        assert!(is_offline_word("custom_mock_word"));
        assert!(is_offline_word("CUSTOM_MOCK_WORD"));
        assert!(!is_offline_word("train")); // mock replaces DB
        set_mock_offline_words(None); // restore
        assert!(is_offline_word("train")); // DB active again

        close_cached_connection();
        // SAFETY: Cleaning up environment variable under TEST_LOCK.
        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
    }
}
