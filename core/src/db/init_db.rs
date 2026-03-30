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

    let conn = Connection::open(get_db_path())?;

    // Connection-level PRAGMAs only — not schema, not migrations.
    // WAL:         multiple readers don't block a single writer.
    // synchronous: NORMAL is safe with WAL and measurably faster than FULL.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous  = NORMAL;",
    )?;

    Ok(conn)
}
