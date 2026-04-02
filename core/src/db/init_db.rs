use crate::paths::{ensure_data_dir, get_db_path};
use rusqlite::{Connection, Result};

/// Opens (or creates) the SQLite database at the platform-appropriate path
/// and applies connection-level PRAGMAs.
///
/// # Responsibilities
/// This function has a single job: open the file and tune the connection.
/// It does NOT create tables or indices — schema is owned entirely by
/// `run_migrations`, which must be called after this on every startup.
///
/// # Note
/// On Android, `paths::init_android_path()` must be called before this.
pub fn init_db() -> Result<Connection> {
    // Guarantee the data directory exists before SQLite tries to create the file
    ensure_data_dir();

    let db_path = get_db_path();

    #[cfg(all(unix, not(target_os = "android")))]
    if !db_path.exists() {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        // Pre-create the DB file with secure permissions to prevent unauthorized access
        let _ = OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o600)
            .open(&db_path);
    }

    let conn = Connection::open(db_path)?;

    // Connection-level PRAGMAs only — not schema, not migrations.
    // WAL:         multiple readers don't block a single writer.
    // synchronous: NORMAL is safe with WAL and measurably faster than FULL.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous  = NORMAL;",
    )?;

    Ok(conn)
}
