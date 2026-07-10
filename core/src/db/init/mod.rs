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
    {
        use std::fs::{self, OpenOptions};
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

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

    let conn = Connection::open(db_path)?;

    conn.busy_timeout(std::time::Duration::from_secs(5))?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;",
    )?;

    Ok(conn)
}
