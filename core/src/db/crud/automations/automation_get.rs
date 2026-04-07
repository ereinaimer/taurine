use rusqlite::{Connection, Result};

use super::{AutomationAction, AutomationRow, AutomationSummary};

fn get_current_os_db_string() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win",
        "macos" => "mac",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        _ => "unknown",
    }
}

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
            target_os,
            tags,
            usage_count,
            last_used_at,
            created_at,
            updated_at,
            version,
            is_deleted,
            is_synced,
            is_enabled
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
            target_os: row.get(6)?,
            tags: row.get(7)?,
            usage_count: row.get(8)?,
            last_used_at: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
            version: row.get(12)?,
            is_deleted: row.get(13)?,
            is_synced: row.get(14)?,
            is_enabled: row.get(15)?,
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
pub fn get_action_by_trigger(conn: &Connection, trigger: &str) -> Result<Option<AutomationAction>> {
    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT payload, action_type
         FROM   automations
         WHERE  trigger = ?1
           AND  is_deleted = 0
           AND  is_enabled = 1
           AND  (target_os = 'all' OR target_os = ?2)
         ORDER BY usage_count DESC
         LIMIT 1",
    )?;

    let result = stmt.query_row(rusqlite::params![trigger, os_str], |row| {
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
pub fn get_all_active_automations(conn: &Connection) -> Result<Vec<(String, AutomationAction)>> {
    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT trigger, payload, action_type
         FROM automations
         WHERE is_deleted = 0
           AND is_enabled = 1
           AND (target_os = 'all' OR target_os = ?1)",
    )?;

    let rows = stmt.query_map([os_str], |row| {
        Ok((
            row.get(0)?,
            AutomationAction {
                payload: row.get(1)?,
                action_type: row.get(2)?,
            },
        ))
    })?;

    let mut actions = Vec::new();
    for action in rows {
        actions.push(action?);
    }

    Ok(actions)
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

    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, description, trigger, usage_count
         FROM   automations
         WHERE  is_deleted = 0
           AND  is_enabled = 1
           AND  (target_os = 'all' OR target_os = ?3)
           AND  (name    LIKE ?1
                 OR trigger LIKE ?1)
         ORDER BY usage_count DESC, updated_at DESC
         LIMIT  ?2",
    )?;

    let rows = stmt.query_map((pattern, limit, os_str), |row| {
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
