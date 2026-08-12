use std::sync::Mutex;

/// Global lock for tests that mutate environment variables or global state.
pub static TEST_LOCK: Mutex<()> = Mutex::new(());

pub use crate::logs::init_tracing_for_tests;

#[cfg(test)]
pub use test_db::*;

#[cfg(test)]
mod test_db {
    use rusqlite::Connection;
    use tempfile::TempDir;

    /// Opens an isolated database for a single test.
    pub fn open_test_db() -> (TempDir, Connection) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let db_path = dir.path().join("test_taurine.db");

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
}
