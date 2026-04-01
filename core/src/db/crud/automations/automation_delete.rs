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
                is_synced  = 0,
                version    = version + 1,
                updated_at = ?1
         WHERE id = ?2 AND is_deleted = 0",
        (now, id),
    )?;

    Ok(rows_changed > 0)
}
