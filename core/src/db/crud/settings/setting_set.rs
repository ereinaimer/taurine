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
    let now = now_unix_secs();

    conn.execute(
        "INSERT INTO settings (key, value, version, updated_at, is_synced)
         VALUES (?1, ?2, 1, ?3, 0)
         ON CONFLICT(key) DO UPDATE SET
             value      = excluded.value,
             version    = version + 1,
             updated_at = excluded.updated_at,
             is_synced  = 0",
        (key, value_json, now),
    )?;

    Ok(())
}

/// Seeds an initial factory default setting on a fresh database.
///
/// Sets `version = 0` and `is_synced = 1`. This ensures:
/// 1. Factory defaults will immediately be overwritten by any cloud setting (`version >= 1`) during pull.
/// 2. Freshly seeded defaults will not be queued as unsynced changes to overwrite the cloud.
pub fn seed_default_setting(conn: &Connection, key: &str, value_json: &str) -> Result<()> {
    let now = now_unix_secs();

    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value, version, updated_at, is_synced)
         VALUES (?1, ?2, 0, ?3, 1)",
        (key, value_json, now),
    )?;

    Ok(())
}
