use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
use rusqlite::{Connection, Result};

use super::{TriggerRow, display_for_aliases, list_aliases};

/// Returns all triggers that are configured by the user to be synced to the cloud.
///
/// Under a Last-Write-Wins (LWW) architecture, the sync worker pulls these
/// configured rows and compares their `version` and `updated_at` against the cloud
/// to resolve state.
///
/// NOTE (Task 4 owns this): alias attachment below is the Task-2 compile
/// restoration; the full JOIN rewrite lands with the entry-model tasks.
pub fn get_syncable_triggers(conn: &Connection) -> Result<Vec<TriggerRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
            a.id,
            a.name,
            a.description,
            a.output,
            a.action_type,
            a.target_os,
            a.only_apps,
            a.except_apps,
            a.tags,
            a.usage_count,
            a.last_used_at,
            a.created_at,
            a.updated_at,
            a.version,
            a.is_deleted,
            a.is_synced,
            a.is_enabled,
            a.auto_case,
            s.interpreter,
            s.behavior,
            s.compressed_content
         FROM triggers a
         LEFT JOIN scripts s ON a.id = s.trigger_id
         WHERE a.is_synced = 1
         ORDER BY a.version, a.updated_at",
    )?;

    let rows = stmt.query_map([], |row| {
        let interpreter_str: Option<String> = row.get(18)?;
        let behavior_str: Option<String> = row.get(19)?;

        let interpreter = interpreter_str
            .and_then(|s| serde_json::from_str::<ScriptInterpreter>(&format!("\"{}\"", s)).ok());
        let behavior = behavior_str
            .and_then(|s| serde_json::from_str::<ScriptBehavior>(&format!("\"{}\"", s)).ok());

        Ok(TriggerRow {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            invocations: Vec::new(),
            display: String::new(),
            output: row.get(3)?,
            action_type: row.get(4)?,
            target_os: row.get(5)?,
            only_apps: row.get(6)?,
            except_apps: row.get(7)?,
            tags: row.get(8)?,
            usage_count: row.get(9)?,
            last_used_at: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            version: row.get(13)?,
            is_deleted: row.get(14)?,
            is_synced: row.get(15)?,
            is_enabled: row.get(16)?,
            auto_case: row.get(17)?,
            interpreter,
            behavior,
            script_binary: row.get(20)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    drop(stmt);

    // honey: N+1, fine under ~1k rows; batch with a single IN query if it grows
    for row in &mut results {
        row.invocations = list_aliases(conn, &row.id).map_err(|err| match err {
            crate::Error::Database(inner) => inner,
            other => rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(other),
            ),
        })?;
        row.display = display_for_aliases(&row.name, &row.invocations);
    }

    Ok(results)
}
