use super::types::DictionaryEntry;
use crate::system::paths::get_data_dir;
use rusqlite::Connection;
use tracing::debug;

pub fn lookup_offline(word: &str) -> Option<Vec<DictionaryEntry>> {
    let db_path = get_data_dir().join("dictionary.db");

    if !db_path.exists() {
        return None;
    }

    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            debug!("Failed to open dictionary db: {}", e);
            return None;
        }
    };

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
