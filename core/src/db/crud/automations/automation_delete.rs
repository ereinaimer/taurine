use rusqlite::{Connection, Result};

use crate::db::now_unix_secs;

/// Soft-deletes the automation by tombstoning (`is_deleted = 1`).
///
/// Returns `true` if a row transitioned from `is_deleted = 0` to `1`,
/// `false` if the row did not exist or was already deleted.
pub fn delete_automation(conn: &Connection, id: &str) -> Result<bool> {
    let now = now_unix_secs();

    // Only tombstone "active" rows to avoid version churn on repeated calls.
    let rows_changed = conn.execute(
        "UPDATE automations
            SET is_deleted = 1,
                version    = version + 1,
                updated_at = ?1
         WHERE id = ?2 AND is_deleted = 0",
        (now, id),
    )?;

    Ok(rows_changed > 0)
}

/// Disables or tombstones all automations matching the specified trigger.
/// Returns the number of affected rows.
pub fn delete_automation_by_trigger(conn: &Connection, trigger: &str) -> Result<usize> {
    let now = now_unix_secs();

    let rows_changed = conn.execute(
        "UPDATE automations
            SET is_deleted = 1,
                version    = version + 1,
                updated_at = ?1
         WHERE trigger = ?2 AND is_deleted = 0",
        (now, trigger),
    )?;

    Ok(rows_changed)
}
/// Disables or tombstones all automations matching the specified triggers.
/// Returns the number of affected rows.
pub fn delete_automations_by_triggers(conn: &Connection, triggers: &[String]) -> Result<usize> {
    if triggers.is_empty() {
        return Ok(0);
    }

    let now = now_unix_secs();
    let placeholders = vec!["?"; triggers.len()].join(", ");

    let sql = format!(
        "UPDATE automations
            SET is_deleted = 1,
                version    = version + 1,
                updated_at = ?1
         WHERE trigger IN ({}) AND is_deleted = 0",
        placeholders
    );

    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(triggers.len() + 1);
    params.push(&now);
    for t in triggers {
        params.push(t);
    }

    let rows_changed = conn.execute(&sql, rusqlite::params_from_iter(params))?;

    Ok(rows_changed)
}
