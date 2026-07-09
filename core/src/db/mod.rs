pub mod crud;
pub mod init;

pub use crud::{
    AutomationRow, MetricRow, TriggerType, delete_automation, delete_metric, get_automation,
    get_current_os_db_string, get_metric, get_metric_counters, increment_metric, normalize_os,
    upsert_automation,
};
pub use crud::{SettingRow, delete_setting, get_setting, get_setting_value, upsert_setting};

/// Returns the current time as Unix seconds (UTC).
///
/// Used by every CRUD write to stamp `updated_at` without pulling in an
/// extra dependency. Exposed as `pub(crate)` so all submodules share one copy.
pub(crate) fn now_unix_secs() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static LAST_TIME: AtomicI64 = AtomicI64::new(0);

    let current = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let prev = LAST_TIME.fetch_max(current, Ordering::Relaxed);
    prev.max(current)
}

/// Returns a connection from the global shared SQLite connection pool.
///
/// Configures WAL mode, synchronous to NORMAL, and sets a busy timeout of 5 seconds
/// to resolve database locking errors under concurrent workloads.
pub fn get_conn()
-> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, crate::error::Error> {
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::OnceLock;

    if cfg!(test) {
        thread_local! {
            static THREAD_POOL: std::cell::RefCell<Option<Pool<SqliteConnectionManager>>> = const { std::cell::RefCell::new(None) };
        }

        return THREAD_POOL.with(|tp| {
            let mut guard = tp.borrow_mut();
            if guard.is_none() {
                let db_path = crate::paths::get_db_path();
                let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
                    conn.execute_batch(
                        "PRAGMA journal_mode = WAL;
                             PRAGMA synchronous = NORMAL;
                             PRAGMA busy_timeout = 5000;",
                    )?;
                    Ok(())
                });
                let pool = Pool::builder()
                    .max_size(1)
                    .build(manager)
                    .expect("Failed to initialize test thread connection pool");
                *guard = Some(pool);
            }
            guard.as_ref().unwrap().get().map_err(|e| {
                crate::error::Error::Service(format!(
                    "Failed to get connection from test pool: {}",
                    e
                ))
            })
        });
    }

    static POOL: OnceLock<Pool<SqliteConnectionManager>> = OnceLock::new();

    let pool = POOL.get_or_init(|| {
        let db_path = crate::paths::get_db_path();
        let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                     PRAGMA synchronous = NORMAL;
                     PRAGMA busy_timeout = 5000;",
            )?;
            Ok(())
        });
        Pool::builder()
            .max_size(5)
            .build(manager)
            .expect("Failed to initialize connection pool")
    });

    pool.get().map_err(|e| {
        crate::error::Error::Service(format!("Failed to get connection from pool: {}", e))
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Schema tests (verify migrations succeed and default values are correct)

// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::{init_tracing_for_tests, open_test_db};

    #[test]
    fn test_settings_table() {
        init_tracing_for_tests();
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
            .query_row(["fuzzy_finder_prefs"], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap();

        assert!(value.contains("show_icons"));
        assert_eq!(version, 1);
    }

    #[test]
    fn test_automations_table() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let now = 1_700_000_000_i64;
        conn.execute(
            "INSERT INTO automations (id, name, trigger, output, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            ("uuid-1", "Good Morning", "gm", "Good morning!", now, now),
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT name, trigger_type, action_type, is_deleted, is_synced, version
                 FROM automations WHERE id = ?1",
            )
            .unwrap();

        let (name, trigger_type, action_type, is_deleted, is_synced, version): (
            String,
            String,
            String,
            bool,
            bool,
            i64,
        ) = stmt
            .query_row(["uuid-1"], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .unwrap();

        assert_eq!(name, "Good Morning");
        assert_eq!(trigger_type, "word");
        assert_eq!(action_type, "text");
        assert!(!is_deleted);
        assert!(is_synced);
        assert_eq!(version, 1);
    }

    #[test]
    fn test_metrics_table() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let now = 1_700_000_000_i64;
        conn.execute(
            "INSERT INTO metrics (
                date, executions, ai_executions, keystrokes_saved, time_saved_ms, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            ("2026-03-30", 42_i64, 4_i64, 500_i64, 60_000_i64, now),
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT executions, ai_executions, keystrokes_saved, time_saved_ms
                 FROM metrics
                 WHERE date = ?1",
            )
            .unwrap();
        let (executions, ai_executions, keystrokes, time_saved_ms): (i64, i64, i64, i64) = stmt
            .query_row(["2026-03-30"], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .unwrap();

        assert_eq!(executions, 42);
        assert_eq!(ai_executions, 4);
        assert_eq!(keystrokes, 500);
        assert_eq!(time_saved_ms, 60_000);
    }

    #[test]
    fn test_migrations_are_idempotent() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db(); // already applied

        // Running again must be a no-op, not an error
        init::migrate::run_migrations(&conn).expect("Second run_migrations call must not fail");

        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn test_automations_table_includes_trigger_type_column() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let mut stmt = conn.prepare("PRAGMA table_info(automations)").unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .unwrap();

        let columns: Vec<(String, String, bool, Option<String>)> =
            rows.map(|row| row.unwrap()).collect();
        let trigger_type = columns
            .iter()
            .find(|(name, _, _, _)| name == "trigger_type")
            .expect("trigger_type column should exist");

        assert_eq!(trigger_type.1, "TEXT");
        assert!(trigger_type.2);
        assert_eq!(trigger_type.3.as_deref(), Some("'word'"));
    }

    #[test]
    fn test_now_unix_secs_monotonicity_and_safety() {
        let t1 = now_unix_secs();
        assert!(t1 > 0, "Time should be positive since Unix Epoch");

        let t2 = now_unix_secs();
        assert!(t2 >= t1, "Time should not run backward");
    }
}
