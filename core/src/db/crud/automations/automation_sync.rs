use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
use rusqlite::{Connection, Result};

use super::AutomationRow;

/// Returns all automations that are configured by the user to be synced to the cloud.
///
/// Under a Last-Write-Wins (LWW) architecture, the sync worker pulls these
/// configured rows and compares their `version` and `updated_at` against the cloud
/// to resolve state.
pub fn get_syncable_automations(conn: &Connection) -> Result<Vec<AutomationRow>> {
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
         WHERE a.is_synced = 1
         ORDER BY a.version, a.updated_at",
    )?;

    let rows = stmt.query_map([], |row| {
        let interpreter_str: Option<String> = row.get(16)?;
        let behavior_str: Option<String> = row.get(17)?;

        let interpreter = interpreter_str
            .and_then(|s| serde_json::from_str::<ScriptInterpreter>(&format!("\"{}\"", s)).ok());
        let behavior = behavior_str
            .and_then(|s| serde_json::from_str::<ScriptBehavior>(&format!("\"{}\"", s)).ok());

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
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}
