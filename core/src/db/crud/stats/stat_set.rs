use rusqlite::{Connection, Result};

use super::StatDeltas;
use crate::db::now_unix_secs;

/// Inserts a new stats row or updates an existing one.
///
/// - On **insert**: `version` starts at `1`.
/// - On **update**: `version` is incremented by `1` atomically.
/// - `updated_at` is always set to the current Unix timestamp.
pub fn increment_stat(conn: &Connection, date: &str, deltas: &StatDeltas) -> Result<()> {
    let now = now_unix_secs();

    conn.execute(
        "INSERT INTO stats (
             date,
             executions,
             ai_executions,
             voice_executions,
             words_dictated,
             keystrokes_saved,
             time_saved_ms,
             version,
             updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)
         ON CONFLICT(date) DO UPDATE SET
             executions       = executions + excluded.executions,
             ai_executions    = ai_executions + excluded.ai_executions,
             voice_executions = voice_executions + excluded.voice_executions,
             words_dictated   = words_dictated + excluded.words_dictated,
             keystrokes_saved = keystrokes_saved + excluded.keystrokes_saved,
             time_saved_ms    = time_saved_ms + excluded.time_saved_ms,
             version          = version + 1,
             updated_at       = excluded.updated_at",
        (
            date,
            deltas.executions,
            deltas.ai_executions,
            deltas.voice_executions,
            deltas.words_dictated,
            deltas.keystrokes_saved,
            deltas.time_saved_ms,
            now,
        ),
    )?;

    Ok(())
}
