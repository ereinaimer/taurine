use rusqlite::{Connection, Result};

use crate::db::now_unix_secs;

/// Inserts a new metrics row or updates an existing one.
///
/// - On **insert**: `version` starts at `1`.
/// - On **update**: `version` is incremented by `1` atomically.
/// - `updated_at` is always set to the current Unix timestamp.
pub fn upsert_metric(
    conn: &Connection,
    date: &str,
    executions: i64,
    keystrokes_saved: i64,
) -> Result<()> {
    let now = now_unix_secs();

    conn.execute(
        "INSERT INTO metrics (date, executions, keystrokes_saved, version, updated_at)
         VALUES (?1, ?2, ?3, 1, ?4)
         ON CONFLICT(date) DO UPDATE SET
             executions        = excluded.executions,
             keystrokes_saved = excluded.keystrokes_saved,
             version          = version + 1,
             updated_at       = excluded.updated_at",
        (date, executions, keystrokes_saved, now),
    )?;

    Ok(())
}

