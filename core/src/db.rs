use crate::paths::get_db_path;
use rusqlite::{Connection, Result};
use std::fs;

/// Initializes the SQLite database at the platform-appropriate path.
/// NOTE: On Android, `paths::init_android_path()` must be called first.
pub fn init_db() -> Result<Connection> {
    let db_path = get_db_path();

    // Ensure parent directories exist
    if let Some(parent) = db_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).expect("Failed to create database directories");
        }
    }

    let conn = Connection::open(db_path)?;

    // Initialize schema
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        (),
    )?;

    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    #[test]
    fn test_init_db() {
        // Create an isolated temp directory that cleans itself up
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_taurine.db");

        // Force init_db to use our temporary path via the env override in paths::get_db_path
        let original = env::var_os("TAURINE_DB_PATH");
        unsafe { env::set_var("TAURINE_DB_PATH", &db_path) };

        let result = init_db();

        // Restore env before any assertion so cleanup always runs
        match &original {
            Some(val) => unsafe { env::set_var("TAURINE_DB_PATH", val) },
            None => unsafe { env::remove_var("TAURINE_DB_PATH") },
        }

        let conn = result.expect("Failed to initialize database");

        // Verify the schema was created by inserting a row
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            ("test_key", "test_value"),
        )
        .unwrap();

        // Read it back
        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .unwrap();
        let value: String = stmt
            .query_row(["test_key"], |row| row.get(0))
            .unwrap();

        assert_eq!(value, "test_value");

        // temp_dir is dropped here -> the SQLite db file is deleted from disk
    }
}