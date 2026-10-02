pub mod crud;
pub mod init;
pub mod key;

pub use key::{get_or_create_db_key, open_keyed_connection};

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

pub(crate) fn is_test_env() -> bool {
    cfg!(test)
        || std::env::var("CARGO_MANIFEST_DIR").is_ok()
        || std::env::current_exe()
            .map(|p| {
                let s = p.to_string_lossy().to_lowercase();
                s.contains("deps") || s.contains("test")
            })
            .unwrap_or(false)
}

/// Builds the shared keyed SQLite pool for `db_path`.
///
/// The database key is fetched ONCE per pool creation and moved into the
/// `with_init` closure, so every pooled connection applies `PRAGMA key` first.
/// Keystore failure returns `Error::Service` before any pool exists: no unkeyed pool.
fn build_keyed_pool(
    db_path: &std::path::Path,
) -> Result<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, crate::error::Error> {
    let db_key = key::get_or_create_db_key()?;
    let manager = r2d2_sqlite::SqliteConnectionManager::file(db_path).with_init(move |conn| {
        let hex_key = zeroize::Zeroizing::new(hex::encode(&db_key[..]));
        let pragma = zeroize::Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex_key.as_str()));
        conn.execute_batch(pragma.as_str())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
                     PRAGMA synchronous = NORMAL;
                     PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    });
    r2d2::Pool::builder()
        .max_size(5)
        // Fail fast on exhaustion instead of blocking the hot path: expansions
        // are millisecond-scale, so a 5s wait means the pool is wedged.
        .connection_timeout(std::time::Duration::from_secs(5))
        // Recycle connections so WAL checkpoints and fresh keys apply over time.
        .max_lifetime(Some(std::time::Duration::from_secs(30 * 60)))
        .idle_timeout(Some(std::time::Duration::from_secs(5 * 60)))
        .build(manager)
        .map_err(|e| {
            crate::error::Error::Service(format!("Failed to initialize connection pool: {}", e))
        })
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
            // truncate(false): SQLite creates the file itself. Truncating here
            // would wipe a database created concurrently between the check
            // and the open.
            let _ = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
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
        let conn = key::open_keyed_connection(&db_path).map_err(|e| {
            crate::error::Error::Service(format!("Failed to open test connection: {}", e))
        })?;
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

    // Try reading with a read lock first. The pool is cloned so the checkout
    // below runs without holding the global lock.
    if let Some(pool) = pools.read().get(&db_path).cloned() {
        return pool.get().map(DbConnection::Pooled).map_err(|e| {
            crate::error::Error::Service(format!("Failed to get connection from pool: {}", e))
        });
    }

    // If not found, acquire write lock and initialize the pool for this path.
    // The checkout happens AFTER the guard is dropped: holding the global
    // write lock across pool.get() would stall every other path on exhaustion.
    let pool = {
        let mut write_guard = pools.write();
        if !write_guard.contains_key(&db_path) {
            let pool = build_keyed_pool(&db_path)?;
            write_guard.insert(db_path.clone(), pool);
        }
        write_guard.get(&db_path).cloned().ok_or_else(|| {
            crate::error::Error::Service(format!(
                "Failed to initialize connection pool for path: {}",
                db_path.display()
            ))
        })?
    };

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

    fn use_mock_keyring() {
        crate::testing::use_shared_test_keyring();
    }

    struct EnvGuard;
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: Serialized via TEST_LOCK; paired with the set_var at entry.
            unsafe {
                std::env::remove_var("TAURINE_DATA_DIR");
            }
        }
    }

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
            "INSERT INTO triggers (id, name, output, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            ("uuid-1", "Good Morning", "Good morning!", now, now),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO trigger_aliases (id, trigger_id, invocation, invocation_type, require_confirmation)
             VALUES ('uuid-1-alias', 'uuid-1', 'gm', 'word', 0)",
            [],
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT name, action_type, is_deleted, is_synced, version
                 FROM triggers WHERE id = ?1",
            )
            .unwrap();

        let (name, action_type, is_deleted, is_synced, version): (String, String, bool, bool, i64) =
            stmt.query_row(["uuid-1"], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .unwrap();

        assert_eq!(name, "Good Morning");
        assert_eq!(action_type, "text");
        assert!(!is_deleted);
        assert!(is_synced);
        assert_eq!(version, 1);

        let (invocation, invocation_type): (String, String) = conn
            .query_row(
                "SELECT invocation, invocation_type FROM trigger_aliases WHERE trigger_id = ?1",
                ["uuid-1"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(invocation, "gm");
        assert_eq!(invocation_type, "word");
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
    fn test_trigger_aliases_table_shape() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        // Per-invocation rows live in trigger_aliases, keyed by type+invocation.
        let mut stmt = conn.prepare("PRAGMA table_info(trigger_aliases)").unwrap();
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
        let invocation_type = columns
            .iter()
            .find(|(name, _, _, _)| name == "invocation_type")
            .expect("invocation_type column should exist");

        assert_eq!(invocation_type.1, "TEXT");
        assert!(invocation_type.2);
        assert_eq!(invocation_type.3.as_deref(), Some("'word'"));

        // The old per-row trigger columns are gone from the parent table.
        let mut parent = conn.prepare("PRAGMA table_info(triggers)").unwrap();
        let parent_names: Vec<String> = parent
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert!(!parent_names.iter().any(|n| n == "trigger"));
        assert!(!parent_names.iter().any(|n| n == "trigger_type"));
    }

    #[test]
    fn test_now_unix_secs_monotonicity_and_safety() {
        let t1 = now_unix_secs();
        assert!(t1 > 0, "Time should be positive since Unix Epoch");

        let t2 = now_unix_secs();
        assert!(t2 >= t1, "Time should not run backward");
    }

    #[test]
    fn task4_get_conn_test_path_is_encrypted() {
        let _guard = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use_mock_keyring();
        init_tracing_for_tests();

        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        // SAFETY: Serialized via TEST_LOCK for test isolation.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", tmp.path().to_str().unwrap()) };
        let _env_guard = EnvGuard;

        let conn = get_conn().expect("get_conn must succeed");
        conn.execute_batch("CREATE TABLE t4 (id INTEGER); INSERT INTO t4 VALUES (7);")
            .expect("write must succeed");
        let value: i64 = conn
            .query_row("SELECT id FROM t4", [], |row| row.get(0))
            .expect("read must succeed");
        assert_eq!(value, 7);

        let db_path = crate::paths::get_db_path();
        drop(conn);
        let bytes = std::fs::read(&db_path).expect("db file must exist");
        assert!(bytes.len() > 16);
        assert_ne!(
            &bytes[..16],
            b"SQLite format 3\0",
            "get_conn file header must not be plaintext"
        );

        let plain = rusqlite::Connection::open(&db_path).expect("plain open must succeed");
        let err = plain
            .query_row("SELECT 1", [], |_: &rusqlite::Row| Ok(()))
            .expect_err("plain read of encrypted db must fail");
        assert!(
            matches!(
                err,
                rusqlite::Error::SqliteFailure(e, _)
                    if e.code == rusqlite::ErrorCode::NotADatabase || e.extended_code == 26
            ),
            "plain read must fail with NotADatabase, got: {err}"
        );
    }

    #[test]
    fn task4_pool_connections_are_encrypted() {
        let _guard = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use_mock_keyring();
        init_tracing_for_tests();

        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        let db_path = tmp.path().join("taurine.db");

        let pool = build_keyed_pool(&db_path).expect("keyed pool build must succeed");
        {
            let conn = pool.get().expect("checkout must succeed");
            conn.execute_batch("CREATE TABLE t4 (id INTEGER); INSERT INTO t4 VALUES (7);")
                .expect("write must succeed");
        }
        {
            let conn = pool.get().expect("second checkout must succeed");
            let value: i64 = conn
                .query_row("SELECT id FROM t4", [], |row| row.get(0))
                .expect("read must succeed");
            assert_eq!(value, 7);
        }
        drop(pool);

        let bytes = std::fs::read(&db_path).expect("db file must exist");
        assert!(bytes.len() > 16);
        assert_ne!(
            &bytes[..16],
            b"SQLite format 3\0",
            "pooled file header must not be plaintext"
        );
    }

    #[test]
    fn task4_open_test_db_is_encrypted_and_seeded() {
        init_tracing_for_tests();
        let (dir, conn) = open_test_db();

        let seeded = crate::db::crud::get_setting_value(&conn, "start_on_boot")
            .expect("seeded read must succeed");
        assert_eq!(seeded.as_deref(), Some("true"));
        drop(conn);

        let bytes = std::fs::read(dir.path().join("taurine.db")).expect("db file must exist");
        assert!(bytes.len() > 16);
        assert_ne!(
            &bytes[..16],
            b"SQLite format 3\0",
            "open_test_db file header must not be plaintext"
        );
    }

    #[test]
    fn test_get_conn_permissions() {
        let _guard = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use_mock_keyring();
        init_tracing_for_tests();

        let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
        let test_dir = tmp.path().to_path_buf();

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

        // Cleanup (tmp dir auto-removed on drop)
        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
    }
}
