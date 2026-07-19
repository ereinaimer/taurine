use rusqlite::{Connection, Result};

/// Permanently removes the stats row for `date`.
///
/// Returns `true` if a row was deleted, `false` if the date did not exist.
pub fn delete_stat(conn: &Connection, date: &str) -> Result<bool> {
    let rows_changed = conn.execute("DELETE FROM stats WHERE date = ?1", [date])?;
    Ok(rows_changed > 0)
}
