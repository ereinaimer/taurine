use rusqlite::{Connection, Result};

use crate::db::now_unix_secs;

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
    set_setting(conn, key, value_json)
}

/// Sets a setting and enqueues an outbox mutation for cloud synchronization.
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    let now = now_unix_secs();

    conn.execute(
        "INSERT INTO settings (key, value, version, updated_at)
         VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(key) DO UPDATE SET
             value      = excluded.value,
             version    = version + 1,
             updated_at = excluded.updated_at",
        (key, value, now),
    )?;

    if crate::db::crud::is_setting_syncable(key) {
        let (version, updated_at): (i64, i64) = conn.query_row(
            "SELECT version, updated_at FROM settings WHERE key = ?1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let parsed_value = serde_json::from_str::<serde_json::Value>(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));

        let rfc3339_time = chrono::DateTime::<chrono::Utc>::from_timestamp(updated_at, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        crate::db::crud::enqueue_outbox(
            conn,
            "setting",
            key,
            "upsert",
            serde_json::json!({
                "key": key,
                "value": parsed_value,
                "version": version,
                "updated_at": rfc3339_time,
            }),
        )
        .map_err(|e| match e {
            crate::Error::Database(inner) => inner,
            other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        })?;
    }

    Ok(())
}

/// Seeds an initial factory default setting on a fresh database.
///
/// Sets `version = 0`. This ensures:
/// 1. Factory defaults will immediately be overwritten by any cloud setting (`version >= 1`) during pull.
pub fn seed_default_setting(conn: &Connection, key: &str, value_json: &str) -> Result<()> {
    let now = now_unix_secs();

    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value, version, updated_at)
         VALUES (?1, ?2, 0, ?3)",
        (key, value_json, now),
    )?;

    Ok(())
}
