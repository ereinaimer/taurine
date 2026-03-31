use rusqlite::{Connection, Result};

use super::{AutomationAction, AutomationRow, AutomationSummary};

/// Returns the full row for `id`, or `None` if it does not exist.
pub fn get_automation(conn: &Connection, id: &str) -> Result<Option<AutomationRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
            id,
            name,
            description,
            trigger,
            payload,
            action_type,
            is_regex,
            target_os,
            tags,
            usage_count,
            last_used_at,
            created_at,
            updated_at,
            version,
            is_deleted,
            is_synced
         FROM automations
         WHERE id = ?1",
    )?;

    let result = stmt.query_row([id], |row| {
        Ok(AutomationRow {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            trigger: row.get(3)?,
            payload: row.get(4)?,
            action_type: row.get(5)?,
            is_regex: row.get(6)?,
            target_os: row.get(7)?,
            tags: row.get(8)?,
            usage_count: row.get(9)?,
            last_used_at: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            version: row.get(13)?,
            is_deleted: row.get(14)?,
            is_synced: row.get(15)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Hot-path lookup: returns just `(payload, action_type)` for an active trigger.
///
/// Uses the `idx_active_triggers` partial index by matching its predicate:
/// `WHERE is_deleted = 0 AND trigger = ?`.
pub fn get_action_by_trigger(
    conn: &Connection,
    trigger: &str,
) -> Result<Option<AutomationAction>> {
    let mut stmt = conn.prepare_cached(
        "SELECT payload, action_type
         FROM   automations
         WHERE  trigger = ?1
           AND  is_deleted = 0
         ORDER BY usage_count DESC
         LIMIT 1",
    )?;

    let result = stmt.query_row([trigger], |row| {
        Ok(AutomationAction {
            payload: row.get(0)?,
            action_type: row.get(1)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Fetches just the trigger strings for all active automations.
///
/// Use this at app startup to build a fast in-memory lookup cache.
pub fn get_all_active_triggers(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT trigger FROM automations WHERE is_deleted = 0",
    )?;

    let rows = stmt.query_map([], |row| row.get(0))?;

    let mut triggers = Vec::new();
    for trigger in rows {
        triggers.push(trigger?);
    }

    Ok(triggers)
}

/// Fuzzy-finder search over active automations by name and trigger.
///
/// Returns a small list of summaries ordered by `usage_count` (most-used first),
/// then by most recently updated as a tie-breaker.
pub fn search_automations(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> Result<Vec<AutomationSummary>> {
    let pattern = format!("%{}%", query);

    let mut stmt = conn.prepare_cached(
        "SELECT id, name, description, trigger, usage_count
         FROM   automations
         WHERE  is_deleted = 0
           AND  (name    LIKE ?1
                 OR trigger LIKE ?1)
         ORDER BY usage_count DESC, updated_at DESC
         LIMIT  ?2",
    )?;

    let rows = stmt.query_map((pattern, limit), |row| {
        Ok(AutomationSummary {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            trigger: row.get(3)?,
            usage_count: row.get(4)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

