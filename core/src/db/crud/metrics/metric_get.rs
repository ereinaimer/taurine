use rusqlite::{Connection, Result};

use super::MetricRow;

/// Returns the full row for `date`, or `None` if it does not exist.
pub fn get_metric(conn: &Connection, date: &str) -> Result<Option<MetricRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT date, executions, keystrokes_saved, version, updated_at
         FROM   metrics
         WHERE  date = ?1",
    )?;

    let result = stmt.query_row([date], |row| {
        Ok(MetricRow {
            date: row.get(0)?,
            executions: row.get(1)?,
            keystrokes_saved: row.get(2)?,
            version: row.get(3)?,
            updated_at: row.get(4)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Convenience wrapper: returns `(executions, keystrokes_saved)` for `date`,
/// or `None` if the row does not exist.
pub fn get_metric_counters(conn: &Connection, date: &str) -> Result<Option<(i64, i64)>> {
    Ok(get_metric(conn, date)?.map(|row| (row.executions, row.keystrokes_saved)))
}

