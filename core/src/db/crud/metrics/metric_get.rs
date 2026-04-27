use rusqlite::{Connection, Result};

use super::MetricRow;

/// Returns the full row for `date`, or `None` if it does not exist.
pub fn get_metric(conn: &Connection, date: &str) -> Result<Option<MetricRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT date, executions, ai_executions, keystrokes_saved, time_saved_ms, version, updated_at
         FROM   metrics
         WHERE  date = ?1",
    )?;

    let result = stmt.query_row([date], |row| {
        Ok(MetricRow {
            date: row.get(0)?,
            executions: row.get(1)?,
            ai_executions: row.get(2)?,
            keystrokes_saved: row.get(3)?,
            time_saved_ms: row.get(4)?,
            version: row.get(5)?,
            updated_at: row.get(6)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Convenience wrapper: returns `(executions, ai_executions, keystrokes_saved, time_saved_ms)` for `date`,
/// or `None` if the row does not exist.
pub fn get_metric_counters(conn: &Connection, date: &str) -> Result<Option<(i64, i64, i64, i64)>> {
    Ok(get_metric(conn, date)?.map(|row| {
        (
            row.executions,
            row.ai_executions,
            row.keystrokes_saved,
            row.time_saved_ms,
        )
    }))
}
