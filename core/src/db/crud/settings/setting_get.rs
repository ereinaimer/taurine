use rusqlite::{Connection, Result};

use super::SettingRow;

/// Returns the full row for `key`, or `None` if it does not exist.
///
/// Hits the primary-key index — O(log n).
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<SettingRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT key, CAST(value AS TEXT), version, updated_at
         FROM   settings
         WHERE  key = ?1",
    )?;

    let result = stmt.query_row([key], |row| {
        Ok(SettingRow {
            key: row.get(0)?,
            value: row.get(1)?,
            version: row.get(2)?,
            updated_at: row.get(3)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Convenience wrapper: returns just the JSON value string for `key`,
/// or `None` if the key does not exist.
pub fn get_setting_value(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(get_setting(conn, key)?.map(|row| row.value))
}

/// Returns all settings as a HashMap mapping key to JSON value string.
pub fn get_all_settings(conn: &Connection) -> Result<std::collections::HashMap<String, String>> {
    let mut stmt = conn.prepare_cached("SELECT key, CAST(value AS TEXT) FROM settings")?;

    let mut map = std::collections::HashMap::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        map.insert(key, value);
    }

    Ok(map)
}
