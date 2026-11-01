use rusqlite::{Connection, Result};

use super::StatRow;

/// Returns the full row for `date`, or `None` if it does not exist.
pub fn get_stat(conn: &Connection, date: &str) -> Result<Option<StatRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT date,
                SUM(executions) as executions,
                SUM(ai_executions) as ai_executions,
                SUM(voice_executions) as voice_executions,
                SUM(words_spoken) as words_spoken,
                SUM(words_expanded) as words_expanded,
                SUM(time_saved_ms) as time_saved_ms,
                MAX(version) as version,
                MAX(updated_at) as updated_at
         FROM   stats
         WHERE  date = ?1
         GROUP BY date",
    )?;

    let result = stmt.query_row([date], |row| {
        Ok(StatRow {
            date: row.get(0)?,
            executions: row.get(1)?,
            ai_executions: row.get(2)?,
            voice_executions: row.get(3)?,
            words_spoken: row.get(4)?,
            words_expanded: row.get(5)?,
            time_saved_ms: row.get(6)?,
            version: row.get(7)?,
            updated_at: row.get(8)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Convenience wrapper: returns `(executions, ai_executions, words_expanded, time_saved_ms)` for `date`,
/// or `None` if the row does not exist.
pub fn get_stat_counters(conn: &Connection, date: &str) -> Result<Option<(i64, i64, i64, i64)>> {
    Ok(get_stat(conn, date)?.map(|row| {
        (
            row.executions,
            row.ai_executions,
            row.words_expanded,
            row.time_saved_ms,
        )
    }))
}

/// Returns the raw stats row for a specific `(device_id, date)`, or `None` if absent.
pub fn get_stat_for_device(
    conn: &Connection,
    device_id: &str,
    date: &str,
) -> Result<Option<StatRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT date, executions, ai_executions, voice_executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
         FROM   stats
         WHERE  device_id = ?1 AND date = ?2",
    )?;

    let result = stmt.query_row(rusqlite::params![device_id, date], |row| {
        Ok(StatRow {
            date: row.get(0)?,
            executions: row.get(1)?,
            ai_executions: row.get(2)?,
            voice_executions: row.get(3)?,
            words_spoken: row.get(4)?,
            words_expanded: row.get(5)?,
            time_saved_ms: row.get(6)?,
            version: row.get(7)?,
            updated_at: row.get(8)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}
