mod init_db;
mod migrations;

pub use init_db::init_db;
pub use migrations::run_migrations;

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    /// Opens an isolated DB in a temp dir, runs init + migrations, returns the
    /// connection. `TempDir` is intentionally dropped here — the file stays open
    /// via the connection, so the drop is safe on all platforms.
    fn open_test_db() -> rusqlite::Connection {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_taurine.db");

        let original = env::var_os("TAURINE_DB_PATH");
        unsafe { env::set_var("TAURINE_DB_PATH", &db_path) };

        let conn = init_db().expect("init_db failed");
        run_migrations(&conn).expect("run_migrations failed");

        match &original {
            Some(val) => unsafe { env::set_var("TAURINE_DB_PATH", val) },
            None => unsafe { env::remove_var("TAURINE_DB_PATH") },
        }

        conn
    }

    #[test]
    fn test_settings_table() {
        let conn = open_test_db();

        conn.execute(
            "INSERT INTO settings (key, value, updated_at, version) VALUES (?1, ?2, ?3, ?4)",
            (
                "fuzzy_finder_prefs",
                r#"{"show_icons": true, "max_results": 20}"#,
                1_700_000_000_i64,
                1_i64,
            ),
        )
        .unwrap();

        let mut stmt = conn
            .prepare("SELECT value, version FROM settings WHERE key = ?1")
            .unwrap();
        let (value, version): (String, i64) = stmt
            .query_row(["fuzzy_finder_prefs"], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap();

        assert!(value.contains("show_icons"));
        assert_eq!(version, 1);
    }

    #[test]
    fn test_automations_table() {
        let conn = open_test_db();

        let now = 1_700_000_000_i64;
        conn.execute(
            "INSERT INTO automations (id, name, trigger, payload, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            ("uuid-1", "Good Morning", "gm", "Good morning!", now, now),
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT name, action_type, is_regex, is_deleted, is_synced, version
                 FROM automations WHERE id = ?1",
            )
            .unwrap();

        let (name, action_type, is_regex, is_deleted, is_synced, version): (
            String, String, bool, bool, bool, i64,
        ) = stmt
            .query_row(["uuid-1"], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .unwrap();

        assert_eq!(name, "Good Morning");
        assert_eq!(action_type, "text");
        assert!(!is_regex);
        assert!(!is_deleted);
        assert!(is_synced);
        assert_eq!(version, 1);
    }

    #[test]
    fn test_metrics_table() {
        let conn = open_test_db();

        let now = 1_700_000_000_i64;
        conn.execute(
            "INSERT INTO metrics (date, executions, keystrokes_saved, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            ("2026-03-30", 42_i64, 500_i64, now),
        )
        .unwrap();

        let mut stmt = conn
            .prepare("SELECT executions, keystrokes_saved FROM metrics WHERE date = ?1")
            .unwrap();
        let (executions, keystrokes): (i64, i64) = stmt
            .query_row(["2026-03-30"], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap();

        assert_eq!(executions, 42);
        assert_eq!(keystrokes, 500);
    }

    #[test]
    fn test_migrations_are_idempotent() {
        let conn = open_test_db(); // already applied v1

        // Running again must be a no-op, not an error
        run_migrations(&conn).expect("Second run_migrations call must not fail");

        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }
}
