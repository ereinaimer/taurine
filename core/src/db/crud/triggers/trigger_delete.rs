use rusqlite::{Connection, Result};

use crate::db::now_unix_secs;

/// Soft-deletes the trigger by tombstoning (`is_deleted = 1`).
///
/// Returns `true` if a row transitioned from `is_deleted = 0` to `1`,
/// `false` if the row did not exist or was already deleted.
pub fn delete_trigger(conn: &Connection, id: &str) -> Result<bool> {
    let now = now_unix_secs();

    // Only tombstone "active" rows to avoid version churn on repeated calls.
    let rows_changed = conn.execute(
        "UPDATE triggers
            SET is_deleted = 1,
                version    = version + 1,
                updated_at = ?1
         WHERE id = ?2 AND is_deleted = 0",
        (now, id),
    )?;

    Ok(rows_changed > 0)
}

/// Disables or tombstones all triggers matching the specified trigger.
/// Returns the number of affected rows.
pub fn delete_trigger_by_value(conn: &Connection, trigger: &str) -> Result<usize> {
    let now = now_unix_secs();

    let rows_changed = conn.execute(
        "UPDATE triggers
            SET is_deleted = 1,
                version    = version + 1,
                updated_at = ?1
         WHERE trigger = ?2 AND is_deleted = 0",
        (now, trigger),
    )?;

    Ok(rows_changed)
}
/// Disables or tombstones all triggers matching the specified triggers.
/// Returns the number of affected rows.
pub fn delete_triggers_by_values(conn: &Connection, triggers: &[String]) -> Result<usize> {
    if triggers.is_empty() {
        return Ok(0);
    }

    let now = now_unix_secs();
    let placeholders = vec!["?"; triggers.len()].join(", ");

    let sql = format!(
        "UPDATE triggers
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

/// Counts active triggers whose trigger matches the given glob pattern.
/// The `*` wildcard is converted to the SQL `%` LIKE wildcard.
pub fn count_triggers_by_pattern(conn: &Connection, pattern: &str) -> Result<usize> {
    let sql_pattern = pattern.replace('*', "%");
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM triggers WHERE trigger LIKE ?1 AND is_deleted = 0",
        [&sql_pattern],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

/// Soft-deletes active triggers whose trigger matches the given glob pattern.
/// The `*` wildcard is converted to the SQL `%` LIKE wildcard.
/// Returns the number of rows tombstoned.
pub fn delete_triggers_by_pattern(conn: &Connection, pattern: &str) -> Result<usize> {
    let now = now_unix_secs();
    let sql_pattern = pattern.replace('*', "%");

    let rows_changed = conn.execute(
        "UPDATE triggers
            SET is_deleted = 1,
                version    = version + 1,
                updated_at = ?1
         WHERE trigger LIKE ?2 AND is_deleted = 0",
        (now, &sql_pattern),
    )?;

    Ok(rows_changed)
}

/// Disables or tombstones all triggers containing the specified tag.
/// Returns the number of affected rows.
pub fn delete_triggers_by_tag(conn: &Connection, tag: &str) -> Result<usize> {
    let now = now_unix_secs();

    let sql = "UPDATE triggers
               SET is_deleted = 1,
                   version    = version + 1,
                   updated_at = ?1
               WHERE is_deleted = 0
                 AND EXISTS (
                     SELECT 1
                     FROM json_each(triggers.tags)
                     WHERE json_each.value = ?2
                 )";

    let rows_changed = conn.execute(sql, rusqlite::params![now, tag])?;

    Ok(rows_changed)
}
