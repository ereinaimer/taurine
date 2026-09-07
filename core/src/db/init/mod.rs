pub mod migrate;
pub mod seed;

use crate::error::Result;
use crate::paths::{ensure_data_dir, get_db_path};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use tracing::{error, warn};

/// Initializes the primary SQLite database at the configured default path,
/// applying migrations and seeding default settings.
///
/// If the database file is corrupted (e.g. disk image is malformed or unreadable),
/// the file and its associated WAL/SHM artifacts are automatically quarantined
/// to `taurine.db.corrupted.<timestamp>`, and a fresh database is initialized
/// to ensure the daemon never gets stuck in a boot loop.
pub fn setup() -> Result<Connection> {
    ensure_data_dir();
    let db_path = get_db_path();
    setup_at_path(&db_path)
}

/// Initializes a SQLite database at an explicit file path with corruption self-healing.
pub fn setup_at_path(db_path: &Path) -> Result<Connection> {
    match init_database(db_path) {
        Ok(conn) => Ok(conn),
        Err(err) if is_corrupt_error(&err) => {
            error!(
                error = %err,
                path = %db_path.display(),
                "SQLite database corruption detected during setup; auto-quarantining and recreating database"
            );
            quarantine_corrupted_db(db_path)?;
            init_database(db_path)
        }
        Err(err) => Err(err),
    }
}

fn init_database(db_path: &Path) -> Result<Connection> {
    let conn = open_connection_at(db_path)?;
    migrate::run_migrations(&conn)?;
    seed::ensure_defaults(&conn)?;

    // Proactively verify integrity with PRAGMA quick_check(1)
    let check: String = conn
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(crate::error::Error::Database)?;
    if check != "ok" {
        return Err(crate::error::Error::Database(
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
                Some(format!("Database integrity check failed: {}", check)),
            ),
        ));
    }

    Ok(conn)
}

fn open_connection_at(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

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
                .open(db_path);
        }

        if let Ok(metadata) = fs::metadata(db_path) {
            let mut perms = metadata.permissions();
            if perms.mode() & 0o777 != 0o600 {
                perms.set_mode(0o600);
                let _ = fs::set_permissions(db_path, perms);
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

/// Automatically renames a corrupted database file and any associated WAL/SHM files
/// to `.corrupted.<timestamp>` to allow creating a healthy fresh database.
pub fn quarantine_corrupted_db(db_path: &Path) -> std::io::Result<Option<PathBuf>> {
    if !db_path.exists() {
        return Ok(None);
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let file_name = db_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("taurine.db");

    let mut corrupted_path =
        db_path.with_file_name(format!("{}.corrupted.{}", file_name, timestamp));
    if corrupted_path.exists() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        corrupted_path = db_path.with_file_name(format!("{}.corrupted.{}", file_name, nanos));
    }

    std::fs::rename(db_path, &corrupted_path)?;
    warn!(
        source = %db_path.display(),
        target = %corrupted_path.display(),
        "Quarantined corrupted SQLite database file"
    );

    // Also quarantine WAL and SHM journal artifacts if present
    let wal_path = db_path.with_file_name(format!("{}-wal", file_name));
    if wal_path.exists() {
        let corrupted_wal =
            db_path.with_file_name(format!("{}.corrupted.{}-wal", file_name, timestamp));
        let _ = std::fs::rename(&wal_path, &corrupted_wal);
    }

    let shm_path = db_path.with_file_name(format!("{}-shm", file_name));
    if shm_path.exists() {
        let corrupted_shm =
            db_path.with_file_name(format!("{}.corrupted.{}-shm", file_name, timestamp));
        let _ = std::fs::rename(&shm_path, &corrupted_shm);
    }

    Ok(Some(corrupted_path))
}

/// Inspects an error to determine whether it represents SQLite corruption.
pub fn is_corrupt_error(err: &crate::error::Error) -> bool {
    match err {
        crate::error::Error::Database(rusqlite_err) => is_rusqlite_corrupt(rusqlite_err),
        crate::error::Error::Service(msg) => is_corrupt_str(msg),
        _ => is_corrupt_str(&err.to_string()),
    }
}

/// Checks if a rusqlite error indicates database file corruption.
pub fn is_rusqlite_corrupt(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(ffi_err, msg) => {
            if ffi_err.code == rusqlite::ErrorCode::DatabaseCorrupt
                || ffi_err.code == rusqlite::ErrorCode::NotADatabase
                || ffi_err.extended_code == 11
                || ffi_err.extended_code == 26
            {
                return true;
            }
            if msg.as_deref().is_some_and(is_corrupt_str) {
                return true;
            }
            is_corrupt_str(&ffi_err.to_string())
        }
        _ => is_corrupt_str(&err.to_string()),
    }
}

fn is_corrupt_str(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.contains("disk image is malformed")
        || lower.contains("database disk image is malformed")
        || lower.contains("file is not a database")
        || lower.contains("file is encrypted or is not a database")
        || lower.contains("unsupported file format")
        || lower.contains("malformed database schema")
        || lower.contains("database integrity check failed")
        || lower.contains("sqlite_corrupt")
        || lower.contains("sqlite_notadb")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_quarantine_corrupted_garbage_database() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("taurine.db");

        // Write non-SQLite garbage data to the database path
        std::fs::write(&db_path, b"THIS IS COMPLETELY CORRUPTED GARBAGE DATA").unwrap();

        // Calling setup_at_path should detect corruption, quarantine, and initialize a fresh DB
        let conn =
            setup_at_path(&db_path).expect("setup_at_path should recover from corrupted file");

        // Verify the connection is functional and integrity passes
        let check: String = conn
            .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
            .expect("quick_check must succeed");
        assert_eq!(check, "ok");

        // Verify standard tables exist
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))
            .expect("settings table must exist");
        assert!(count > 0);

        // Verify that a quarantined file was created in the directory
        let entries = std::fs::read_dir(temp_dir.path()).unwrap();
        let quarantined = entries
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("taurine.db.corrupted.")
            })
            .expect("Quarantined file must exist");

        let content = std::fs::read(quarantined.path()).unwrap();
        assert_eq!(content, b"THIS IS COMPLETELY CORRUPTED GARBAGE DATA");
    }

    #[test]
    fn test_quarantine_malformed_sqlite_page() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("taurine.db");

        // First initialize a valid database
        {
            let conn = setup_at_path(&db_path).expect("initial setup should succeed");
            conn.execute(
                "CREATE TABLE test_data (id INTEGER PRIMARY KEY, content TEXT);",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO test_data (id, content) VALUES (1, 'important');",
                [],
            )
            .unwrap();
        }

        // Corrupt the database file by overwriting bytes past header with 0xFF
        let mut data = std::fs::read(&db_path).unwrap();
        assert!(data.len() > 100);
        let end = data.len().min(500);
        for byte in &mut data[40..end] {
            *byte = 0xFF;
        }
        std::fs::write(&db_path, data).unwrap();

        // setup_at_path must detect corruption, quarantine, and initialize a fresh DB
        let conn =
            setup_at_path(&db_path).expect("setup_at_path should recover from malformed page");

        let check: String = conn
            .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
            .expect("quick_check must succeed on regenerated database");
        assert_eq!(check, "ok");

        // Verify quarantined file exists
        let entries = std::fs::read_dir(temp_dir.path()).unwrap();
        let has_quarantined = entries.filter_map(|e| e.ok()).any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("taurine.db.corrupted.")
        });
        assert!(has_quarantined, "Quarantined file must exist");
    }

    #[test]
    fn test_quarantine_cleans_up_wal_and_shm() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("taurine.db");
        let wal_path = temp_dir.path().join("taurine.db-wal");
        let shm_path = temp_dir.path().join("taurine.db-shm");

        std::fs::write(&db_path, b"CORRUPTED DB").unwrap();
        std::fs::write(&wal_path, b"DUMMY WAL DATA").unwrap();
        std::fs::write(&shm_path, b"DUMMY SHM DATA").unwrap();

        let quarantined = quarantine_corrupted_db(&db_path).unwrap();
        assert!(quarantined.is_some());

        // Both WAL and SHM files should be quarantined alongside the main DB
        let entries = std::fs::read_dir(temp_dir.path()).unwrap();
        let file_names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert!(
            file_names
                .iter()
                .any(|n| n.starts_with("taurine.db.corrupted.") && n.ends_with("-wal")),
            "Quarantined WAL file must exist, found: {:?}",
            file_names
        );
        assert!(
            file_names
                .iter()
                .any(|n| n.starts_with("taurine.db.corrupted.") && n.ends_with("-shm")),
            "Quarantined SHM file must exist, found: {:?}",
            file_names
        );
        assert!(!wal_path.exists(), "Original WAL file must no longer exist");
        assert!(!shm_path.exists(), "Original SHM file must no longer exist");
    }

    #[test]
    fn test_is_corrupt_error_classifier() {
        let err_malformed = crate::error::Error::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(11),
            Some("database disk image is malformed".to_string()),
        ));
        assert!(is_corrupt_error(&err_malformed));

        let err_notadb = crate::error::Error::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(26),
            Some("file is not a database".to_string()),
        ));
        assert!(is_corrupt_error(&err_notadb));

        let err_syntax = crate::error::Error::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(1),
            Some("near 'SYNTAX': syntax error".to_string()),
        ));
        assert!(!is_corrupt_error(&err_syntax));
    }
}
