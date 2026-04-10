use rusqlite::Connection;
use tempfile::TempDir;

/// Opens an isolated database for a single test.
///
/// Returns `(TempDir, Connection)`. The caller **must** bind the `TempDir` to
/// a name (e.g. `_dir`) so it is not dropped before the test ends — dropping
/// it deletes the file on disk while the connection is still open.
///
/// ```rust
/// let (_dir, conn) = open_test_db();
/// ```
pub fn open_test_db() -> (TempDir, Connection) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test_taurine.db");

    // Open directly — no env var, no global state, fully parallel-safe.
    let conn = Connection::open(&db_path).expect("failed to open test DB");
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous  = NORMAL;",
    )
    .expect("PRAGMA setup failed");

    crate::db::init::migrate::run_migrations(&conn).expect("run_migrations failed");
    crate::db::init::seed::ensure_defaults(&conn).expect("ensure_defaults failed");
    (dir, conn)
}

pub use crate::logs::init_tracing_for_tests;
