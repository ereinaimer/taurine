pub mod crud;
pub mod init;

pub use crud::{
    ActionType, AppFilterPrefix, StatRow, TargetOs, TriggerLimits, TriggerRow, TriggerType,
    delete_stat, delete_trigger, get_current_os_db_string, get_stat, get_stat_counters,
    get_trigger, increment_stat, normalize_os, upsert_trigger,
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

pub enum DbConnection {
    Pooled(r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>),
    Raw(rusqlite::Connection),
}

impl std::ops::Deref for DbConnection {
    type Target = rusqlite::Connection;
    fn deref(&self) -> &Self::Target {
        match self {
            DbConnection::Pooled(conn) => conn,
            DbConnection::Raw(conn) => conn,
        }
    }
}

impl std::ops::DerefMut for DbConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            DbConnection::Pooled(conn) => conn,
            DbConnection::Raw(conn) => conn,
        }
    }
}

fn is_test_env() -> bool {
    cfg!(test)
        || std::env::var("CARGO_MANIFEST_DIR").is_ok()
        || std::env::current_exe()
            .map(|p| {
                let s = p.to_string_lossy().to_lowercase();
                s.contains("deps") || s.contains("test")
            })
            .unwrap_or(false)
}

/// Returns a connection from the global shared SQLite connection pool (or a raw connection in tests).
///
/// Configures WAL mode, synchronous to NORMAL, and sets a busy timeout of 5 seconds
/// to resolve database locking errors under concurrent workloads.
pub fn get_conn() -> Result<DbConnection, crate::error::Error> {
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::OnceLock;

    crate::paths::ensure_data_dir();

    #[cfg(all(unix, not(target_os = "android")))]
    {
        use std::fs::{self, OpenOptions};
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let db_path = crate::paths::get_db_path();

        if !db_path.exists() {
            let _ = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&db_path);
        }

        if let Ok(metadata) = fs::metadata(&db_path) {
            let mut perms = metadata.permissions();
            if perms.mode() & 0o777 != 0o600 {
                perms.set_mode(0o600);
                let _ = fs::set_permissions(&db_path, perms);
            }
        }
    }

    if is_test_env() {
        let db_path = crate::paths::get_db_path();
        let conn = rusqlite::Connection::open(db_path).map_err(|e| {
            crate::error::Error::Service(format!("Failed to open test connection: {}", e))
        })?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| {
                crate::error::Error::Service(format!("Failed to set busy timeout: {}", e))
            })?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
        .map_err(|e| crate::error::Error::Service(format!("Failed to set pragmas: {}", e)))?;
        init::migrate::run_migrations(&conn).map_err(|e| {
            crate::error::Error::Service(format!("Failed to run migrations in test conn: {}", e))
        })?;
        return Ok(DbConnection::Raw(conn));
    }

    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::path::PathBuf;

    static POOLS: OnceLock<RwLock<HashMap<PathBuf, Pool<SqliteConnectionManager>>>> =
        OnceLock::new();

    let db_path = crate::paths::get_db_path();
    let pools = POOLS.get_or_init(|| RwLock::new(HashMap::new()));

    // Try reading with a read lock first
    {
        let read_guard = pools.read();
        if let Some(pool) = read_guard.get(&db_path) {
            return pool.get().map(DbConnection::Pooled).map_err(|e| {
                crate::error::Error::Service(format!("Failed to get connection from pool: {}", e))
            });
        }
    }

    // If not found, acquire write lock and initialize the pool for this path
    let mut write_guard = pools.write();
    if !write_guard.contains_key(&db_path) {
        let manager = SqliteConnectionManager::file(&db_path).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                     PRAGMA synchronous = NORMAL;
                     PRAGMA busy_timeout = 5000;",
            )?;
            Ok(())
        });
        let pool = Pool::builder().max_size(5).build(manager).map_err(|e| {
            crate::error::Error::Service(format!("Failed to initialize connection pool: {}", e))
        })?;
        write_guard.insert(db_path.clone(), pool);
    }
    let pool = write_guard.get(&db_path).cloned().ok_or_else(|| {
        crate::error::Error::Service(format!(
            "Failed to initialize connection pool for path: {}",
            db_path.display()
        ))
    })?;

    pool.get().map(DbConnection::Pooled).map_err(|e| {
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
    fn test_triggers_table() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let now = 1_700_000_000_i64;
        conn.execute(
            "INSERT INTO triggers (id, name, trigger, output, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            ("uuid-1", "Good Morning", "gm", "Good morning!", now, now),
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT name, trigger_type, action_type, is_deleted, is_synced, version
                 FROM triggers WHERE id = ?1",
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
    fn test_stats_table() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let now = 1_700_000_000_i64;
        conn.execute(
            "INSERT INTO stats (
                date, executions, ai_executions, keystrokes_saved, time_saved_ms, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            ("2026-03-30", 42_i64, 4_i64, 500_i64, 60_000_i64, now),
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT executions, ai_executions, keystrokes_saved, time_saved_ms
                 FROM stats
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
    fn test_triggers_table_includes_trigger_type_column() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let mut stmt = conn.prepare("PRAGMA table_info(triggers)").unwrap();
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

    #[test]
    fn test_get_conn_permissions() {
        let _guard = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        init_tracing_for_tests();

        let test_dir = std::env::temp_dir().join("taurine_db_perms_test");
        let _ = std::fs::remove_dir_all(&test_dir);

        unsafe { std::env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };

        // Calling get_conn() should initialize the DB and ensure permissions
        let conn = get_conn();
        assert!(conn.is_ok());

        let db_path = crate::paths::get_db_path();
        assert!(db_path.exists());

        #[cfg(all(unix, not(target_os = "android")))]
        {
            use std::fs;
            use std::os::unix::fs::PermissionsExt;

            // Check directory permissions
            let dir_metadata = fs::metadata(&test_dir).unwrap();
            assert_eq!(dir_metadata.permissions().mode() & 0o777, 0o700);

            // Check database file permissions
            let db_metadata = fs::metadata(&db_path).unwrap();
            assert_eq!(db_metadata.permissions().mode() & 0o777, 0o600);
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&test_dir);
        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
    }
}
