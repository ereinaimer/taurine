use std::sync::Mutex;

/// Global lock for tests that mutate environment variables or global state.
pub static TEST_LOCK: Mutex<()> = Mutex::new(());

pub use crate::logs::init_tracing_for_tests;

#[cfg(test)]
pub use test_db::*;

#[cfg(test)]
mod test_db {
    use std::sync::Once;
    use tempfile::TempDir;

    static MOCK_KEYRING: Once = Once::new();

    fn use_mock_keyring() {
        MOCK_KEYRING.call_once(|| {
            keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        });
    }

    /// Opens an isolated database for a single test.
    /// Uses `taurine.db` so `TAURINE_DATA_DIR=<dir>` resolves to the same file.
    /// Keyed via the OS keystore (mock in tests); WAL/NORMAL come from the helper.
    pub fn open_test_db() -> (TempDir, rusqlite::Connection) {
        use_mock_keyring();
        let dir = TempDir::new().expect("failed to create temp dir");
        let db_path = dir.path().join("taurine.db");

        let conn = crate::db::key::open_keyed_connection(&db_path).expect("failed to open test DB");

        crate::db::init::migrate::run_migrations(&conn).expect("run_migrations failed");
        crate::db::init::seed::ensure_defaults(&conn).expect("ensure_defaults failed");
        (dir, conn)
    }
}
