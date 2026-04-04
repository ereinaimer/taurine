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
            is_synced,
            is_enabled
         FROM automations
         WHERE is_synced = 1
         ORDER BY version, updated_at",
    )?;

    let rows = stmt.query_map([], |row| {
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
            is_enabled: row.get(16)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}
