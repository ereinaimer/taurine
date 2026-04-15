use rusqlite::{Connection, Result};

use super::{AutomationAction, AutomationListItem, AutomationRow, AutomationSummary};

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

/// Helper to parse JSON variants that might contain double-quotes from SQLite.
fn parse_json_variant<T: serde::de::DeserializeOwned>(s: Option<String>) -> Option<T> {
    s.and_then(|val| {
        let trimmed = val.trim_matches('"');
        serde_json::from_str::<T>(&format!("\"{}\"", trimmed)).ok()
    })
}

/// Returns the full row for `id`, or `None` if it does not exist.
pub fn get_automation(conn: &Connection, id: &str) -> Result<Option<AutomationRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
            a.id,
            a.name,
            a.description,
            a.trigger,
            a.output,
            a.action_type,
            a.target_os,
            a.tags,
            a.usage_count,
            a.last_used_at,
            a.created_at,
            a.updated_at,
            a.version,
            a.is_deleted,
            a.is_synced,
            a.is_enabled,
            s.interpreter,
            s.behavior,
            s.compressed_content
         FROM automations a
         LEFT JOIN scripts s ON a.id = s.automation_id
         WHERE a.id = ?1",
    )?;

    let result = stmt.query_row([id], |row| {
        let interpreter = parse_json_variant(row.get(16)?);
        let behavior = parse_json_variant(row.get(17)?);

        Ok(AutomationRow {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            trigger: row.get(3)?,
            output: row.get(4)?,
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
            interpreter,
            behavior,
            script_binary: row.get(18)?,
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
        "SELECT a.output, a.action_type, s.interpreter, s.behavior, s.compressed_content
         FROM   automations a
         LEFT JOIN scripts s ON a.id = s.automation_id
         WHERE  a.trigger = ?1
           AND  a.is_deleted = 0
           AND  a.is_enabled = 1
           AND  (a.target_os = 'all' OR a.target_os = ?2)
         ORDER BY a.usage_count DESC
         LIMIT 1",
    )?;

    let result = stmt.query_row(rusqlite::params![trigger, os_str], |row| {
        let interpreter = parse_json_variant(row.get(2)?);
        let behavior = parse_json_variant(row.get(3)?);

        Ok(AutomationAction {
            output: row.get(0)?,
            action_type: row.get(1)?,
            interpreter,
            behavior,
            script_binary: row.get(4)?,
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
        "SELECT a.trigger, a.output, a.action_type, s.interpreter, s.behavior, s.compressed_content
         FROM automations a
         LEFT JOIN scripts s ON a.id = s.automation_id
         WHERE a.is_deleted = 0
           AND a.is_enabled = 1
           AND (a.target_os = 'all' OR a.target_os = ?1)",
    )?;

    let rows = stmt.query_map([os_str], |row| {
        let interpreter = parse_json_variant(row.get(3)?);
        let behavior = parse_json_variant(row.get(4)?);

        Ok((
            row.get(0)?,
            AutomationAction {
                output: row.get(1)?,
                action_type: row.get(2)?,
                interpreter,
                behavior,
                script_binary: row.get(5)?,
            },
        ))
    })?;

    let mut actions = Vec::new();
    for action in rows {
        actions.push(action?);
    }

    Ok(actions)
}

/// Fetches all active automations with enough metadata for sorting/listing in CLI.
pub fn get_automations_list(conn: &Connection) -> Result<Vec<AutomationListItem>> {
    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT a.trigger, a.output, a.action_type, a.usage_count, a.last_used_at, a.created_at,
                s.interpreter, s.behavior
         FROM   automations a
         LEFT JOIN scripts s ON a.id = s.automation_id
         WHERE  a.is_deleted = 0
           AND  a.is_enabled = 1
           AND  (a.target_os = 'all' OR a.target_os = ?1)",
    )?;

    let rows = stmt.query_map([os_str], |row| {
        let interpreter = parse_json_variant(row.get(6)?);
        let behavior = parse_json_variant(row.get(7)?);

        Ok(AutomationListItem {
            trigger: row.get(0)?,
            output: row.get(1)?,
            action_type: row.get(2)?,
            usage_count: row.get(3)?,
            last_used_at: row.get(4)?,
            created_at: row.get(5)?,
            interpreter,
            behavior,
        })
    })?;

    let mut list = Vec::new();
    for row in rows {
        list.push(row?);
    }

    Ok(list)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};

    #[test]
    fn test_parse_json_variant_handles_double_quotes() {
        // Normal case
        let result: Option<ScriptInterpreter> = parse_json_variant(Some("python".to_string()));
        assert_eq!(result, Some(ScriptInterpreter::Python));

        // problematic double-quote case from SQLite
        let result: Option<ScriptInterpreter> = parse_json_variant(Some("\"python\"".to_string()));
        assert_eq!(result, Some(ScriptInterpreter::Python));

        // Behavior case
        let result: Option<ScriptBehavior> = parse_json_variant(Some("\"inline\"".to_string()));
        assert_eq!(result, Some(ScriptBehavior::Inline));

        // Mixed case (trimming matches any number of quotes at ends)
        let result: Option<ScriptInterpreter> =
            parse_json_variant(Some("\"\"bash\"\"".to_string()));
        assert_eq!(result, Some(ScriptInterpreter::Bash));
    }

    #[test]
    fn test_parse_json_variant_handles_none_and_invalid() {
        assert_eq!(parse_json_variant::<ScriptInterpreter>(None), None);
        assert_eq!(
            parse_json_variant::<ScriptInterpreter>(Some("invalid".to_string())),
            None
        );
    }
}
