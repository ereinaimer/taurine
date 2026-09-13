use std::path::Path;
use zeroize::{Zeroize, Zeroizing};

const KEYRING_SERVICE: &str = "taurine";
const KEYRING_USER: &str = "db-key";

/// Returns the per-install database encryption key, creating and storing one on first run.
///
/// Fail-closed: any keystore problem (or a corrupt stored value) is a `Service`
/// error. Never falls back to plaintext or an ephemeral key.
pub fn get_or_create_db_key() -> crate::Result<Zeroizing<[u8; 32]>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(keystore_unavailable)?;
    get_or_create_key_with(&entry)
}

fn get_or_create_key_with(entry: &keyring::Entry) -> crate::Result<Zeroizing<[u8; 32]>> {
    match entry.get_password() {
        Ok(stored) => decode_stored_key(stored),
        Err(keyring::Error::NoEntry) => create_and_store_key(entry),
        Err(err) => Err(keystore_unavailable(err)),
    }
}

fn decode_stored_key(mut stored: String) -> crate::Result<Zeroizing<[u8; 32]>> {
    let decoded = hex::decode(stored.as_bytes());
    stored.zeroize();
    let mut bytes = decoded.map_err(|_| invalid_stored_key())?;
    if bytes.len() != 32 {
        bytes.zeroize();
        return Err(invalid_stored_key());
    }
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&bytes);
    bytes.zeroize();
    Ok(key)
}

fn create_and_store_key(entry: &keyring::Entry) -> crate::Result<Zeroizing<[u8; 32]>> {
    let key = Zeroizing::new(rand::random::<[u8; 32]>());
    let hex_key = Zeroizing::new(hex::encode(&key[..]));
    entry
        .set_password(hex_key.as_str())
        .map_err(keystore_unavailable)?;
    // Re-read (not echo) so a broken store is caught before the key is used.
    let reread = entry.get_password().map_err(|err| match err {
        keyring::Error::NoEntry => crate::Error::Service(
            "Database key vanished from the OS keystore right after storing it; refusing to use an unverified key.".to_string(),
        ),
        err => keystore_unavailable(err),
    })?;
    let reread_key = decode_stored_key(reread)?;
    if *reread_key != *key {
        return Err(crate::Error::Service(
            "OS keystore returned a different database key than the one just stored; refusing to use it.".to_string(),
        ));
    }
    Ok(reread_key)
}

/// Opens `db_path` with the per-install key applied as the FIRST statement.
///
/// On a key error the key is re-fetched once and the open retried with the
/// fresh key. This heals the fresh-install race where two processes mint
/// different keys and the loser holds a stale one. A genuine mismatch (same
/// key re-fetched) returns the original error: still fail-closed.
pub fn open_keyed_connection(db_path: &Path) -> crate::Result<rusqlite::Connection> {
    open_keyed_connection_with(db_path, get_or_create_db_key)
}

fn open_keyed_connection_with<F>(
    db_path: &Path,
    mut fetch_key: F,
) -> crate::Result<rusqlite::Connection>
where
    F: FnMut() -> crate::Result<Zeroizing<[u8; 32]>>,
{
    let key = fetch_key()?;
    match open_with_key(db_path, &key) {
        Ok(conn) => Ok(conn),
        Err(err) => {
            let is_key = matches!(&err, crate::error::Error::Database(inner) if crate::db::init::is_key_error(inner));
            if !is_key {
                return Err(err);
            }
            let fresh = fetch_key()?;
            if *fresh == *key {
                return Err(err);
            }
            open_with_key(db_path, &fresh)
        }
    }
}

fn open_with_key(db_path: &Path, key: &[u8; 32]) -> crate::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(db_path)?;
    {
        let hex_key = Zeroizing::new(hex::encode(key));
        let pragma = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex_key.as_str()));
        conn.execute_batch(pragma.as_str())?;
    }
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;",
    )?;
    Ok(conn)
}

fn keystore_unavailable(err: keyring::Error) -> crate::Error {
    crate::Error::Service(format!(
        "Database key unavailable from the OS keystore ({err}). Start the secret service (Linux), unlock the login keychain (macOS), or sign in so Credential Manager is available (Windows); headless sessions without a secret daemon are unsupported. Taurine will not open the database without its key."
    ))
}

fn invalid_stored_key() -> crate::Error {
    crate::Error::Service(
        "Stored database key in the OS keystore is invalid (expected 64 hex characters decoding to 32 bytes). Taurine will not overwrite it: restore the taurine db-key entry from backup or re-import from an encrypted export.".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn use_mock_keyring() {
        crate::testing::use_shared_test_keyring();
    }

    fn test_entry() -> keyring::Entry {
        use_mock_keyring();
        keyring::Entry::new_with_credential(Box::new(keyring::mock::MockCredential::default()))
    }

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn get_or_create_returns_same_key_twice() {
        let _guard = lock();
        let entry = test_entry();
        let first = get_or_create_key_with(&entry).expect("first get-or-create must succeed");
        let second = get_or_create_key_with(&entry).expect("second get-or-create must succeed");
        assert_eq!(*first, *second);
        assert_eq!(first.len(), 32);
    }

    #[test]
    fn stored_value_is_64_char_hex() {
        let _guard = lock();
        let entry = test_entry();
        let key = get_or_create_key_with(&entry).expect("get-or-create must succeed");
        let stored = entry.get_password().expect("key must be stored");
        assert_eq!(stored.len(), 64);
        let decoded = hex::decode(&stored).expect("stored value must be hex");
        assert_eq!(decoded.len(), 32);
        assert_eq!(decoded.as_slice(), &key[..]);
    }

    #[test]
    fn corrupt_and_short_stored_values_are_service_errors() {
        let _guard = lock();
        let short_hex = "00".repeat(31);
        for bad in ["abc123", "not-hex-at-all!!!", short_hex.as_str()] {
            let entry = test_entry();
            entry
                .set_password(bad)
                .expect("seeding corrupt value must succeed");
            let err = get_or_create_key_with(&entry).expect_err("corrupt key must fail");
            assert!(
                matches!(err, crate::Error::Service(_)),
                "corrupt key must be Service error, got: {err}"
            );
        }
    }

    #[test]
    fn keystore_error_is_service_error_not_panic() {
        let _guard = lock();
        use_mock_keyring();
        let entry = test_entry();
        let mock: &keyring::mock::MockCredential = entry
            .get_credential()
            .downcast_ref()
            .expect("mock downcast must succeed");
        mock.set_error(keyring::Error::PlatformFailure(Box::new(
            std::io::Error::new(std::io::ErrorKind::NotFound, "no secret daemon"),
        )));
        let err = get_or_create_key_with(&entry).expect_err("keystore-down must fail");
        assert!(
            matches!(err, crate::Error::Service(_)),
            "keystore-down must be Service error, got: {err}"
        );
    }

    #[test]
    fn public_get_or_create_returns_32_bytes() {
        let _guard = lock();
        use_mock_keyring();
        let key = get_or_create_db_key().expect("public get-or-create must succeed");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn open_retries_once_with_fresh_key_after_stale_key_error() {
        let _guard = lock();
        use_mock_keyring();
        let dir = tempfile::TempDir::new().expect("temp dir must be created");
        let db_path = dir.path().join("taurine.db");
        let stale = Zeroizing::new(rand::random::<[u8; 32]>());
        let current = get_or_create_db_key().expect("key must exist");
        assert_ne!(*stale, *current, "test needs distinct keys");

        // File keyed with the current key; first fetch serves the stale one
        // (fresh-install race), second fetch serves the current one.
        open_with_key(&db_path, &current).expect("seed with current key must succeed");
        let mut fetches = [Some(stale), Some(current)].into_iter();
        let conn = open_keyed_connection_with(&db_path, || {
            fetches
                .next()
                .flatten()
                .ok_or_else(|| crate::Error::Service("out of test keys".to_string()))
        })
        .expect("retry with fresh key must succeed");
        let check: String = conn
            .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
            .expect("quick_check must run");
        assert_eq!(check, "ok");
    }

    #[test]
    fn open_with_unchanged_wrong_key_stays_fail_closed() {
        let _guard = lock();
        use_mock_keyring();
        let dir = tempfile::TempDir::new().expect("temp dir must be created");
        let db_path = dir.path().join("taurine.db");
        let wrong = Zeroizing::new(rand::random::<[u8; 32]>());
        open_with_key(&db_path, &wrong).expect("seed with wrong key must succeed");

        // Store holds a different, unchanged key: refetch agrees, original error stands.
        let stored = get_or_create_db_key().expect("key must exist");
        assert_ne!(*stored, *wrong, "test needs distinct keys");
        let err = open_keyed_connection(&db_path).expect_err("foreign key must fail");
        let quarantined = std::fs::read_dir(dir.path())
            .expect("dir must list")
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with("taurine.db.corrupted.")
                    || name.starts_with("taurine.db.incompatible.")
            });
        assert!(!quarantined, "fail-closed must not quarantine, got: {err}");
    }

    #[test]
    fn open_keyed_connection_creates_encrypted_db() {
        let _guard = lock();
        use_mock_keyring();
        let dir = tempfile::TempDir::new().expect("temp dir must be created");
        let db_path = dir.path().join("taurine.db");
        let conn = open_keyed_connection(&db_path).expect("keyed open must succeed");
        conn.execute_batch("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1);")
            .expect("write must succeed");
        drop(conn);

        let header = std::fs::read(&db_path).expect("db file must exist");
        assert!(header.len() > 16);
        const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";
        assert_ne!(&header[..16], SQLITE_MAGIC);
    }

    #[test]
    fn keyed_db_round_trips_and_rejects_plain_open() {
        let _guard = lock();
        let entry = test_entry();
        let key = get_or_create_key_with(&entry).expect("get-or-create must succeed");
        let dir = tempfile::TempDir::new().expect("temp dir must be created");
        let db_path = dir.path().join("taurine.db");

        {
            let conn = open_with_key(&db_path, &key).expect("keyed open must succeed");
            conn.execute_batch("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (42);")
                .expect("write must succeed");
        }
        {
            let conn = open_with_key(&db_path, &key).expect("reopen must succeed");
            let value: i64 = conn
                .query_row("SELECT id FROM t", [], |row| row.get(0))
                .expect("read must succeed");
            assert_eq!(value, 42);
        }

        let plain = rusqlite::Connection::open(&db_path).expect("plain open must succeed");
        let err = plain
            .query_row("SELECT 1", [], |_row: &rusqlite::Row| Ok(()))
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
}
