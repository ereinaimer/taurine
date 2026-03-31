use rusqlite::{Connection, Result};

/// Permanently removes the setting with `key`.
///
/// Returns `true` if a row was deleted, `false` if the key did not exist.
///
/// Settings have no sync tombstone requirement, so this is a hard delete.
pub fn delete_setting(conn: &Connection, key: &str) -> Result<bool> {
    let rows_changed = conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(rows_changed > 0)
}

