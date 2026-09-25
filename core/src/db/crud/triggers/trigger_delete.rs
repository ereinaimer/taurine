use rusqlite::{Connection, Result};

use super::aliases::{InvocationType, delete_alias, tombstone_entry};
use super::trigger_set::{parents_by_invocation, with_transaction};
use crate::db::now_unix_secs;

fn crate_err(error: crate::Error) -> rusqlite::Error {
    match error {
        crate::Error::Database(inner) => inner,
        other => rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(other),
        ),
    }
}

/// Soft-deletes the entry by id via `tombstone_entry` (parent tombstone +
/// alias cleanup).
///
/// Returns `true` if a live row transitioned, `false` if the row did not
/// exist or was already deleted (no version churn on repeat calls).
pub fn delete_trigger(conn: &Connection, id: &str) -> Result<bool> {
    let active: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM triggers WHERE id = ?1 AND is_deleted = 0)",
        [id],
        |row| row.get(0),
    )?;
    if !active {
        return Ok(false);
    }
    tombstone_entry(conn, id, now_unix_secs()).map_err(crate_err)?;
    Ok(true)
}

/// Removes one invocation value from ALL holder parents across all four
/// invocation types (Word verbatim, Hotkey canonicalized, Voice normalized,
/// Regex verbatim — Hotkey/Voice normalize inside the multi-holder lookup).
/// Each emptied parent is tombstoned via `delete_alias` GC.
/// Returns the total number of alias rows removed (old rows-affected parity).
pub fn delete_trigger_by_value(conn: &Connection, trigger: &str) -> Result<usize> {
    delete_triggers_by_values(conn, &[trigger.to_string()])
}

/// Removes every listed invocation value (see `delete_trigger_by_value`).
/// Returns the total number of alias rows removed. One transaction
/// (all-or-nothing).
pub fn delete_triggers_by_values(conn: &Connection, triggers: &[String]) -> Result<usize> {
    if triggers.is_empty() {
        return Ok(0);
    }

    with_transaction(conn, || {
        let mut removed = 0;
        for value in triggers {
            for invocation_type in [
                InvocationType::Word,
                InvocationType::Hotkey,
                InvocationType::Regex,
                InvocationType::Voice,
            ] {
                for pid in parents_by_invocation(conn, invocation_type, value)? {
                    if delete_alias(conn, &pid, invocation_type, value)? {
                        removed += 1;
                    }
                }
            }
        }
        Ok(removed)
    })
    .map_err(crate_err)
}

/// Counts DISTINCT live parents with at least one matching invocation.
/// The `*` wildcard is converted to the SQL `%` LIKE wildcard.
pub fn count_triggers_by_pattern(conn: &Connection, pattern: &str) -> Result<usize> {
    let sql_pattern = pattern.replace('*', "%");
    let count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT al.trigger_id)
          FROM trigger_aliases al
          JOIN triggers t ON t.id = al.trigger_id
          WHERE al.invocation LIKE ?1 AND t.is_deleted = 0",
        [&sql_pattern],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

/// Collects DISTINCT live parent ids with at least one matching invocation.
fn parent_ids_by_invocation_like(
    conn: &Connection,
    sql_pattern: &str,
) -> crate::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT al.trigger_id
          FROM trigger_aliases al
          JOIN triggers t ON t.id = al.trigger_id
          WHERE al.invocation LIKE ?1 AND t.is_deleted = 0",
    )?;
    Ok(stmt
        .query_map([sql_pattern], |row| row.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?)
}

/// Soft-deletes whole parent entries with a matching invocation.
/// The `*` wildcard is converted to the SQL `%` LIKE wildcard.
/// Returns the number of parents tombstoned.
pub fn delete_triggers_by_pattern(conn: &Connection, pattern: &str) -> Result<usize> {
    let sql_pattern = pattern.replace('*', "%");
    with_transaction(conn, || {
        let ids = parent_ids_by_invocation_like(conn, &sql_pattern)?;
        let now = now_unix_secs();
        for id in &ids {
            tombstone_entry(conn, id, now)?;
        }
        Ok(ids.len())
    })
    .map_err(crate_err)
}

/// Soft-deletes whole parent entries containing the specified tag.
/// Returns the number of parents tombstoned.
pub fn delete_triggers_by_tag(conn: &Connection, tag: &str) -> Result<usize> {
    with_transaction(conn, || {
        let mut stmt = conn.prepare_cached(
            "SELECT id FROM triggers
              WHERE is_deleted = 0
                AND EXISTS (
                    SELECT 1
                    FROM json_each(triggers.tags)
                    WHERE json_each.value = ?1
                )",
        )?;
        let ids: Vec<String> = stmt
            .query_map([tag], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        let now = now_unix_secs();
        for id in &ids {
            tombstone_entry(conn, id, now)?;
        }
        Ok(ids.len())
    })
    .map_err(crate_err)
}
