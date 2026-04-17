pub mod migrate;
pub mod seed;

use crate::error::Result;
use crate::paths::{ensure_data_dir, get_db_path};
use rusqlite::Connection;

pub fn setup() -> Result<Connection> {
    let conn = open_connection()?;
    migrate::run_migrations(&conn)?;
    seed::ensure_defaults(&conn)?;
    Ok(conn)
}

fn open_connection() -> Result<Connection> {
    ensure_data_dir();
    let db_path = get_db_path();

    #[cfg(all(unix, not(target_os = "android")))]
    if !db_path.exists() {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        let _ = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&db_path);
    }

    let conn = Connection::open(db_path)?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;",
    )?;

    Ok(conn)
}
