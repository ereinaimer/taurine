use rusqlite::{Connection, Result};

use crate::db::now_unix_secs;

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/// A single row from the `settings` table.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingRow {
    /// The unique key that identifies this setting (e.g. `"theme"`, `"fuzzy_finder_prefs"`).
    pub key: String,
    /// The setting's value, stored as a JSON string.
    pub value: String,
    /// Incremented on every write. Used as a Last-Write-Wins arbiter during sync.
    pub version: i64,
    /// Unix timestamp (seconds) of the last write.
    pub updated_at: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Read
// ─────────────────────────────────────────────────────────────────────────────

/// Returns the full row for `key`, or `None` if it does not exist.
///
/// Hits the primary-key index — O(log n).
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<SettingRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT key, value, version, updated_at
         FROM   settings
         WHERE  key = ?1",
    )?;

    let result = stmt.query_row([key], |row| {
        Ok(SettingRow {
            key:        row.get(0)?,
            value:      row.get(1)?,
            version:    row.get(2)?,
            updated_at: row.get(3)?,
        })
    });

    match result {
        Ok(row)                                  => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e)                                   => Err(e),
    }
}

/// Convenience wrapper: returns just the JSON value string for `key`,
/// or `None` if the key does not exist.
pub fn get_setting_value(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(get_setting(conn, key)?.map(|row| row.value))
}

// ─────────────────────────────────────────────────────────────────────────────
// Write
// ─────────────────────────────────────────────────────────────────────────────

/// Inserts a new setting or updates an existing one.
///
/// - On **insert**: `version` starts at `1`.
/// - On **update**: `version` is incremented by `1` atomically — no separate
///   read required.
/// - `updated_at` is always set to the current Unix timestamp.
///
/// # Why not `INSERT OR REPLACE`?
///
/// `INSERT OR REPLACE` deletes the existing row and re-inserts it, which
/// silently resets `version` back to `1`. The `ON CONFLICT … DO UPDATE`
/// form updates the row in-place, preserving and incrementing `version`.
pub fn upsert_setting(conn: &Connection, key: &str, value_json: &str) -> Result<()> {
    let now = now_unix_secs();

    conn.execute(
        "INSERT INTO settings (key, value, version, updated_at)
         VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(key) DO UPDATE SET
             value      = excluded.value,
             version    = version + 1,
             updated_at = excluded.updated_at",
        (key, value_json, now),
    )?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Delete
// ─────────────────────────────────────────────────────────────────────────────

/// Permanently removes the setting with `key`.
///
/// Returns `true` if a row was deleted, `false` if the key did not exist.
///
/// Settings have no sync tombstone requirement, so this is a hard delete.
pub fn delete_setting(conn: &Connection, key: &str) -> Result<bool> {
    let rows_changed = conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(rows_changed > 0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_test_db;

    // ── read ──────────────────────────────────────────────────────────────────

    #[test]
    fn get_setting_returns_none_for_missing_key() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        let result = get_setting(&conn, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_setting_value_returns_none_for_missing_key() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        let result = get_setting_value(&conn, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    // ── insert (first upsert) ─────────────────────────────────────────────────

    #[test]
    fn upsert_setting_inserts_new_key_with_version_1() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        upsert_setting(&conn, "theme", r#""dark""#).unwrap();

        let row = get_setting(&conn, "theme").unwrap().unwrap();
        assert_eq!(row.key, "theme");
        assert_eq!(row.value, r#""dark""#);
        assert_eq!(row.version, 1);
        assert!(row.updated_at > 0);
    }

    #[test]
    fn get_setting_value_returns_value_after_insert() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        upsert_setting(&conn, "theme", r#""dark""#).unwrap();

        let value = get_setting_value(&conn, "theme").unwrap().unwrap();
        assert_eq!(value, r#""dark""#);
    }

    // ── update (subsequent upserts) ───────────────────────────────────────────

    #[test]
    fn upsert_setting_increments_version_on_update() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_setting(&conn, "theme", r#""dark""#).unwrap();
        upsert_setting(&conn, "theme", r#""light""#).unwrap();
        upsert_setting(&conn, "theme", r#""system""#).unwrap();

        let row = get_setting(&conn, "theme").unwrap().unwrap();
        assert_eq!(row.version, 3, "version must be 3 after three writes");
        assert_eq!(row.value, r#""system""#);
    }

    #[test]
    fn upsert_setting_updates_value() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_setting(&conn, "lang", r#""en""#).unwrap();
        upsert_setting(&conn, "lang", r#""fr""#).unwrap();

        let value = get_setting_value(&conn, "lang").unwrap().unwrap();
        assert_eq!(value, r#""fr""#);
    }

    #[test]
    fn upsert_setting_does_not_affect_other_keys() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_setting(&conn, "theme", r#""dark""#).unwrap();
        upsert_setting(&conn, "lang", r#""en""#).unwrap();
        upsert_setting(&conn, "theme", r#""light""#).unwrap(); // only theme changes

        let lang_row = get_setting(&conn, "lang").unwrap().unwrap();
        assert_eq!(lang_row.version, 1, "lang must be untouched");
        assert_eq!(lang_row.value, r#""en""#);
    }

    // ── delete ────────────────────────────────────────────────────────────────

    #[test]
    fn delete_setting_returns_true_when_key_exists() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        upsert_setting(&conn, "to_delete", r#"true"#).unwrap();

        let deleted = delete_setting(&conn, "to_delete").unwrap();
        assert!(deleted);
    }

    #[test]
    fn delete_setting_returns_false_when_key_missing() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        let deleted = delete_setting(&conn, "ghost").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn delete_setting_actually_removes_the_row() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        upsert_setting(&conn, "gone", r#"42"#).unwrap();
        delete_setting(&conn, "gone").unwrap();

        let result = get_setting(&conn, "gone").unwrap();
        assert!(result.is_none());
    }
}
