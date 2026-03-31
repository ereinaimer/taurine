use rusqlite::{Connection, Result};

use super::AutomationRow;

/// Returns all automations that have local changes waiting to be synced up.
///
/// Ordered by `(version, updated_at)` to align with the `idx_sync_queue` index,
/// so the sync worker can process the newest state for each record first.
pub fn get_pending_sync_automations(conn: &Connection) -> Result<Vec<AutomationRow>> {
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
         WHERE is_synced = 0
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
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

/// Marks a batch of IDs as successfully synced to the cloud.
///
/// This variant uses a single transaction with one `UPDATE` per ID, which is
/// efficient enough for typical sync batch sizes and works without requiring
/// SQLite virtual table extensions. If you later enable the `vtab`/`array`
/// features and load the `rarray` module on the connection, this can be
/// swapped to an `IN rarray(?1)`-style implementation.
pub fn mark_automations_synced(conn: &Connection, ids: &[&str]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    let tx = conn.unchecked_transaction()?;

    {
        let mut stmt = tx.prepare_cached(
            "UPDATE automations
             SET    is_synced = 1
             WHERE  id = ?1",
        )?;

        for id in ids {
            stmt.execute([id])?;
        }
    }

    tx.commit()?;
    Ok(())
}

