mod init_db;
mod migrations;
pub mod crud;

pub use crud::{delete_setting, get_setting, get_setting_value, upsert_setting, SettingRow};
pub use init_db::init_db;
pub use migrations::run_migrations;

/// Returns the current time as Unix seconds (UTC).
///
/// Used by every CRUD write to stamp `updated_at` without pulling in an
/// extra dependency. Exposed as `pub(crate)` so all submodules share one copy.
pub(crate) fn now_unix_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock predates the Unix epoch")
        .as_secs() as i64
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared test helper
// ─────────────────────────────────────────────────────────────────────────────

/// Opens an isolated database for a single test.
///
/// Returns `(TempDir, Connection)`. The caller **must** bind the `TempDir` to
/// a name (e.g. `_dir`) so it is not dropped before the test ends — dropping
/// it deletes the file on disk while the connection is still open.
///
/// ```rust
/// let (_dir, conn) = open_test_db();
/// ```
///
/// Lives outside `mod tests` so any submodule can import it with:
/// `use crate::db::open_test_db;`
#[cfg(test)]
pub(crate) fn open_test_db() -> (tempfile::TempDir, rusqlite::Connection) {
    use rusqlite::Connection;
    use tempfile::TempDir;

    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test_taurine.db");

    // Open directly — no env var, no global state, fully parallel-safe.
    let conn = Connection::open(&db_path).expect("failed to open test DB");
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous  = NORMAL;",
    )
    .expect("PRAGMA setup failed");

    run_migrations(&conn).expect("run_migrations failed");

    (dir, conn)
}

// ─────────────────────────────────────────────────────────────────────────────
// Schema tests (verify migrations succeed and default values are correct)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_table() {
        let (_dir, conn) = open_test_db();

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
            .query_row(["fuzzy_finder_prefs"], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .unwrap();

        assert!(value.contains("show_icons"));
        assert_eq!(version, 1);
    }

    #[test]
    fn test_automations_table() {
        let (_dir, conn) = open_test_db();

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
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, i64>(5)?,
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
        let (_dir, conn) = open_test_db();

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
            .query_row(["2026-03-30"], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .unwrap();

        assert_eq!(executions, 42);
        assert_eq!(keystrokes, 500);
    }

    #[test]
    fn test_migrations_are_idempotent() {
        let (_dir, conn) = open_test_db(); // already applied

        // Running again must be a no-op, not an error
        run_migrations(&conn).expect("Second run_migrations call must not fail");

        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap();
        assert_eq!(version, 1);
    }
}
