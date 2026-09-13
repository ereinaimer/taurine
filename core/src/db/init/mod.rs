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
    // Legacy plaintext DBs are quarantined as incompatible before any keyed
    // open, then a fresh encrypted DB is created below.
    if is_plaintext_db(db_path) {
        quarantine_incompatible_db(db_path)?;
    }
    match init_database(db_path) {
        Ok(conn) => Ok(conn),
        // Wrong key (or damaged header): actionable error, never quarantine.
        Err(err) if matches!(&err, crate::error::Error::Database(inner) if is_key_error(inner)) => {
            Err(key_mismatch_error())
        }
        Err(err) if is_corrupt_error(&err) => {
            error!(
                error = %err,
                path = %db_path.display(),
                "SQLite database corruption detected during setup; auto-quarantining and recreating database"
            );
            let quarantined = quarantine_corrupted_db(db_path)?;
            init_database(db_path).map_err(|retry_err| {
                error!(
                    error = %retry_err,
                    path = %db_path.display(),
                    quarantined = ?quarantined,
                    "Recreated database failed to initialize after quarantine; original preserved at quarantine path"
                );
                retry_err
            })
        }
        // Keystore-unavailable (`Error::Service`) and anything else: fail
        // closed immediately, no quarantine, no retry.
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
            // No truncate(): SQLite creates the file itself. Truncating here
            // would wipe a database created concurrently between the check
            // and the open.
            let _ = OpenOptions::new()
                .write(true)
                .create(true)
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

    let conn = crate::db::key::open_keyed_connection(db_path)?;

    Ok(conn)
}

/// Automatically renames a corrupted database file and any associated WAL/SHM files
/// to `.corrupted.<timestamp>` to allow creating a healthy fresh database.
pub fn quarantine_corrupted_db(db_path: &Path) -> std::io::Result<Option<PathBuf>> {
    quarantine_with_suffix(
        db_path,
        "corrupted",
        "Quarantined corrupted SQLite database file",
    )
}

/// Renames a legacy plaintext database file and any associated WAL/SHM files
/// to `.incompatible.<timestamp>` so a fresh encrypted database can be created.
/// The quarantined file is left intact for manual inspection.
pub fn quarantine_incompatible_db(db_path: &Path) -> std::io::Result<Option<PathBuf>> {
    quarantine_with_suffix(
        db_path,
        "incompatible",
        "Quarantined plaintext SQLite database (incompatible with encryption)",
    )
}

fn quarantine_with_suffix(
    db_path: &Path,
    suffix: &str,
    warn_msg: &str,
) -> std::io::Result<Option<PathBuf>> {
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

    let mut quarantined_path =
        db_path.with_file_name(quarantine_file_name(file_name, suffix, timestamp, pid(), 0));
    let mut n = 0u32;
    while quarantined_path.exists() {
        n += 1;
        if n > 9999 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "could not find a free quarantine name for {}",
                    db_path.display()
                ),
            ));
        }
        quarantined_path =
            db_path.with_file_name(quarantine_file_name(file_name, suffix, timestamp, pid(), n));
    }

    std::fs::rename(db_path, &quarantined_path)?;
    warn!(
        source = %db_path.display(),
        target = %quarantined_path.display(),
        "{warn_msg}"
    );

    // WAL/SHM targets derive from the FINAL quarantined name, so they can
    // neither collide independently nor orphan from the main file.
    let final_name = quarantined_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file_name);
    move_sidecar(db_path, file_name, "-wal", final_name)?;
    move_sidecar(db_path, file_name, "-shm", final_name)?;

    Ok(Some(quarantined_path))
}

/// Deterministic quarantine file name; `n == 0` keeps the legacy
/// `{file}.{suffix}.{millis}` shape, later attempts append pid + counter.
fn quarantine_file_name(
    file_name: &str,
    suffix: &str,
    timestamp: u128,
    pid: u32,
    n: u32,
) -> String {
    if n == 0 {
        format!("{file_name}.{suffix}.{timestamp}")
    } else {
        format!("{file_name}.{suffix}.{timestamp}.p{pid}.{n}")
    }
}

fn pid() -> u32 {
    std::process::id()
}

/// Moves a `-wal`/`-shm` sidecar next to its quarantined main file.
///
/// The main file is already safe at this point. If the rename fails (locked
/// file, permissions), the orphan is removed instead so a stale WAL can never
/// attach to the fresh database; removal failure fails boot loudly.
fn move_sidecar(
    db_path: &Path,
    file_name: &str,
    sidecar: &str,
    final_name: &str,
) -> std::io::Result<()> {
    let src = db_path.with_file_name(format!("{file_name}{sidecar}"));
    if !src.exists() {
        return Ok(());
    }
    let dst = db_path.with_file_name(format!("{final_name}{sidecar}"));
    match std::fs::rename(&src, &dst) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            warn!(
                source = %src.display(),
                error = %rename_err,
                "Could not move database sidecar next to quarantine; removing orphan so the fresh database starts clean"
            );
            std::fs::remove_file(&src)
        }
    }
}

/// Plaintext SQLite magic header (`SQLite format 3\0`).
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// True when the file exists and starts with the plaintext SQLite magic.
/// Reads only the first 16 bytes; missing or short files return false.
fn is_plaintext_db(db_path: &Path) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(db_path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut header = [0u8; 16];
    match file.read_exact(&mut header) {
        Ok(()) => header == *SQLITE_MAGIC,
        Err(_) => false,
    }
}

/// Actionable error for wrong-key opens: never quarantines the file.
fn key_mismatch_error() -> crate::error::Error {
    crate::error::Error::Service(
        "Database key mismatch: the OS keystore key cannot decrypt the database file. Restore from an encrypted export; Taurine will not quarantine or overwrite it.".to_string(),
    )
}

/// True for wrong-key/damaged-header failures: `NotADatabase` or extended code 26.
pub(crate) fn is_key_error(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(ffi_err, _) => {
            ffi_err.code == rusqlite::ErrorCode::NotADatabase || ffi_err.extended_code == 26
        }
        _ => false,
    }
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
/// Key errors (wrong key / damaged header) are never corruption.
pub fn is_rusqlite_corrupt(err: &rusqlite::Error) -> bool {
    if is_key_error(err) {
        return false;
    }
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

    fn use_mock_keyring() {
        crate::testing::use_shared_test_keyring();
    }

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn test_setup_at_path_creates_encrypted_db() {
        let _guard = lock();
        use_mock_keyring();
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("taurine.db");

        let conn = setup_at_path(&db_path).expect("fresh setup must succeed");

        let check: String = conn
            .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
            .expect("quick_check must succeed");
        assert_eq!(check, "ok");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))
            .expect("settings table must exist");
        assert!(count > 0);
        drop(conn);

        let bytes = std::fs::read(&db_path).expect("db file must exist");
        assert!(bytes.len() > 16);
        assert_ne!(
            &bytes[..16],
            b"SQLite format 3\0",
            "fresh db header must not be plaintext"
        );
    }

    #[test]
    fn test_setup_at_path_quarantines_plaintext_db() {
        let _guard = lock();
        use_mock_keyring();
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("taurine.db");

        // Build a REAL plaintext DB: plain open without key.
        {
            let plain = rusqlite::Connection::open(&db_path).expect("plain open must succeed");
            plain
                .execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (1);")
                .expect("plaintext write must succeed");
        }
        let header = std::fs::read(&db_path).expect("plaintext db must exist");
        assert_eq!(&header[..16], b"SQLite format 3\0");

        let conn = setup_at_path(&db_path).expect("plaintext must heal to fresh encrypted db");

        let check: String = conn
            .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
            .expect("quick_check must succeed");
        assert_eq!(check, "ok");
        drop(conn);

        let fresh = std::fs::read(&db_path).expect("fresh db must exist");
        assert_ne!(&fresh[..16], b"SQLite format 3\0");

        let entries: Vec<String> = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            entries
                .iter()
                .any(|n| n.starts_with("taurine.db.incompatible.")),
            "incompatible quarantine must exist, found: {:?}",
            entries
        );
    }

    #[test]
    fn test_quarantine_incompatible_db_moves_wal_and_shm() {
        let _guard = lock();
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("taurine.db");
        let wal_path = temp_dir.path().join("taurine.db-wal");
        let shm_path = temp_dir.path().join("taurine.db-shm");

        std::fs::write(&db_path, b"SQLite format 3\0plaintext").unwrap();
        std::fs::write(&wal_path, b"DUMMY WAL DATA").unwrap();
        std::fs::write(&shm_path, b"DUMMY SHM DATA").unwrap();

        let quarantined = quarantine_incompatible_db(&db_path).unwrap();
        assert!(quarantined.is_some());

        let file_names: Vec<String> = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            file_names
                .iter()
                .any(|n| n.starts_with("taurine.db.incompatible.") && n.ends_with("-wal")),
            "quarantined WAL must exist, found: {:?}",
            file_names
        );
        assert!(
            file_names
                .iter()
                .any(|n| n.starts_with("taurine.db.incompatible.") && n.ends_with("-shm")),
            "quarantined SHM must exist, found: {:?}",
            file_names
        );
        assert!(!wal_path.exists());
        assert!(!shm_path.exists());
    }

    #[test]
    fn test_key_error_classifier() {
        let notadb = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(26),
            Some("file is encrypted or is not a database".to_string()),
        );
        assert!(is_key_error(&notadb));
        assert!(
            !is_corrupt_error(&crate::error::Error::Database(notadb)),
            "key error must not classify as corruption"
        );

        let corrupt = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(11),
            Some("database disk image is malformed".to_string()),
        );
        assert!(!is_key_error(&corrupt));
        assert!(is_corrupt_error(&crate::error::Error::Database(corrupt)));

        let syntax = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(1),
            Some("near 'SYNTAX': syntax error".to_string()),
        );
        assert!(!is_key_error(&syntax));
        assert!(!is_corrupt_error(&crate::error::Error::Database(syntax)));

        // Keystore-unavailable Service errors must never look like corruption.
        let down = crate::error::Error::Service(
            "Database key unavailable from the OS keystore (test).".to_string(),
        );
        assert!(!is_corrupt_error(&down));
    }

    #[test]
    fn test_damaged_header_returns_key_error_without_quarantine() {
        let _guard = lock();
        use_mock_keyring();
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("taurine.db");

        // Non-SQLite garbage: damaged header, surfaces as a key error by design.
        std::fs::write(&db_path, b"THIS IS COMPLETELY CORRUPTED GARBAGE DATA").unwrap();

        let err = setup_at_path(&db_path).expect_err("damaged header must fail closed");
        assert!(
            matches!(err, crate::error::Error::Service(ref msg) if msg.contains("key mismatch")),
            "damaged header must be a key-mismatch Service error, got: {err}"
        );

        // Fail-closed: original file intact, nothing quarantined, no fresh DB.
        assert_eq!(
            std::fs::read(&db_path).unwrap(),
            b"THIS IS COMPLETELY CORRUPTED GARBAGE DATA"
        );
        let names: Vec<String> = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            names
                .iter()
                .all(|n| !n.contains(".corrupted.") && !n.contains(".incompatible.")),
            "no quarantine must be created, found: {:?}",
            names
        );
    }

    #[test]
    fn test_damaged_ciphertext_returns_key_error_without_quarantine() {
        let _guard = lock();
        use_mock_keyring();
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

        // Damaged ciphertext is indistinguishable from a wrong key: fail closed,
        // never quarantine or overwrite.
        let err = setup_at_path(&db_path).expect_err("damaged ciphertext must fail closed");
        assert!(
            matches!(err, crate::error::Error::Service(ref msg) if msg.contains("key mismatch")),
            "damaged ciphertext must be a key-mismatch Service error, got: {err}"
        );

        let names: Vec<String> = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            names
                .iter()
                .all(|n| !n.contains(".corrupted.") && !n.contains(".incompatible.")),
            "no quarantine must be created, found: {:?}",
            names
        );
    }

    #[test]
    fn test_quarantine_cleans_up_wal_and_shm() {
        let _guard = lock();
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
    fn test_quarantine_file_name_shapes() {
        assert_eq!(
            quarantine_file_name("taurine.db", "corrupted", 123, 456, 0),
            "taurine.db.corrupted.123"
        );
        assert_eq!(
            quarantine_file_name("taurine.db", "corrupted", 123, 456, 2),
            "taurine.db.corrupted.123.p456.2"
        );
    }

    #[test]
    fn test_double_quarantine_keeps_both_with_distinct_names() {
        let _guard = lock();
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("taurine.db");

        std::fs::write(&db_path, b"FIRST GARBAGE").unwrap();
        let first = quarantine_corrupted_db(&db_path)
            .expect("first quarantine must succeed")
            .expect("first quarantine must return a path");
        std::fs::write(&db_path, b"SECOND GARBAGE").unwrap();
        let second = quarantine_corrupted_db(&db_path)
            .expect("second quarantine must succeed")
            .expect("second quarantine must return a path");

        assert_ne!(first, second, "colliding quarantines must not share a name");
        assert_eq!(std::fs::read(&first).unwrap(), b"FIRST GARBAGE");
        assert_eq!(std::fs::read(&second).unwrap(), b"SECOND GARBAGE");
    }

    #[test]
    fn test_quarantine_sidecars_derive_from_final_main_name() {
        let _guard = lock();
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("taurine.db");
        std::fs::write(&db_path, b"CORRUPTED DB").unwrap();
        std::fs::write(temp_dir.path().join("taurine.db-wal"), b"DUMMY WAL").unwrap();
        std::fs::write(temp_dir.path().join("taurine.db-shm"), b"DUMMY SHM").unwrap();

        let quarantined = quarantine_corrupted_db(&db_path)
            .expect("quarantine must succeed")
            .expect("quarantine must return a path");
        let stem = quarantined
            .file_name()
            .and_then(|f| f.to_str())
            .expect("quarantine must have a file name");
        assert!(
            temp_dir.path().join(format!("{stem}-wal")).exists(),
            "WAL must sit next to the quarantined main file"
        );
        assert!(
            temp_dir.path().join(format!("{stem}-shm")).exists(),
            "SHM must sit next to the quarantined main file"
        );
        assert_eq!(
            std::fs::read(temp_dir.path().join(format!("{stem}-wal"))).unwrap(),
            b"DUMMY WAL"
        );
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
        // NotADatabase is a key error now, never corruption.
        assert!(!is_corrupt_error(&err_notadb));
        assert!(matches!(
            &err_notadb,
            crate::error::Error::Database(inner) if is_key_error(inner)
        ));

        let err_syntax = crate::error::Error::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(1),
            Some("near 'SYNTAX': syntax error".to_string()),
        ));
        assert!(!is_corrupt_error(&err_syntax));
    }
}
