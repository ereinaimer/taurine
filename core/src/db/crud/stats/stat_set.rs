use rusqlite::{Connection, Result};

use crate::db::now_unix_secs;

/// Inserts a new stats row or updates an existing one.
///
/// - On **insert**: `version` starts at `1`.
/// - On **update**: `version` is incremented by `1` atomically.
/// - `updated_at` is always set to the current Unix timestamp.
pub fn increment_stat(
    conn: &Connection,
    date: &str,
    delta_executions: i64,
    delta_ai_executions: i64,
    delta_keystrokes_saved: i64,
    delta_time_saved_ms: i64,
) -> Result<()> {
    let now = now_unix_secs();

    conn.execute(
        "INSERT INTO stats (
             date,
             executions,
             ai_executions,
             keystrokes_saved,
             time_saved_ms,
             version,
             updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
         ON CONFLICT(date) DO UPDATE SET
             executions       = executions + excluded.executions,
             ai_executions    = ai_executions + excluded.ai_executions,
             keystrokes_saved = keystrokes_saved + excluded.keystrokes_saved,
             time_saved_ms    = time_saved_ms + excluded.time_saved_ms,
             version          = version + 1,
             updated_at       = excluded.updated_at",
        (
            date,
            delta_executions,
            delta_ai_executions,
            delta_keystrokes_saved,
            delta_time_saved_ms,
            now,
        ),
    )?;

    Ok(())
}
