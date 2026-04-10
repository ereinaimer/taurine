use rusqlite::{Connection, Result};

use crate::db::now_unix_secs;

/// Inserts a new automation or updates an existing one.
///
/// Semantics:
/// - On **insert**: `version` starts at `1`, `created_at`/`updated_at` are set
///   to "now".
/// - On **update**: `version` is incremented by `1` atomically.
/// - `is_deleted` is forced to `0` (reactivates tombstoned rows).
/// - `is_synced` is forced to `0` so the sync layer can enqueue this record.
#[allow(clippy::too_many_arguments)]
pub fn upsert_automation(
    conn: &Connection,
    id: &str,
    name: &str,
    description: Option<&str>,
    trigger: &str,
    output: &str,
    action_type: &str,
    target_os: &str,
    tags_json: &str, // JSON string
    usage_count: i64,
    last_used_at: Option<i64>,
) -> Result<()> {
    let now = now_unix_secs();

    // Keep created_at stable across updates.
    conn.execute(
        "INSERT INTO automations
            (id, name, description, trigger, output, action_type, target_os, tags,
             usage_count, last_used_at, created_at, updated_at, version, is_deleted)
         VALUES
            (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
             ?9, ?10, ?11, ?12, 1, 0)
         ON CONFLICT(id) DO UPDATE SET
            name         = excluded.name,
            description  = excluded.description,
            trigger      = excluded.trigger,
            output       = excluded.output,
            action_type  = excluded.action_type,
            target_os    = excluded.target_os,
            tags         = excluded.tags,
            usage_count  = excluded.usage_count,
            last_used_at = excluded.last_used_at,
            is_deleted   = 0,
            version      = version + 1,
            updated_at   = excluded.updated_at",
        (
            id,
            name,
            description,
            trigger,
            output,
            action_type,
            target_os,
            tags_json,
            usage_count,
            last_used_at,
            now,
            now,
        ),
    )?;

    Ok(())
}

/// Increments the usage_count and updates last_used_at for the given trigger.
pub fn increment_usage_count_by_trigger(conn: &Connection, trigger: &str) -> Result<()> {
    conn.execute(
        "UPDATE automations
         SET usage_count = usage_count + 1,
             last_used_at = ?1
         WHERE trigger = ?2 AND is_deleted = 0",
        rusqlite::params![now_unix_secs(), trigger],
    )?;
    Ok(())
}

/// Opens the production DB and increments `usage_count` for every active row
/// whose trigger matches, while also updating the daily metrics.
///
/// Intended for callers (e.g. the daemon hook thread) that do not hold an open
/// `Connection` and do not want a direct dependency on `rusqlite`.
pub fn record_expansion_usage(trigger: &str, output_len: usize, delete_count: usize) {
    match Connection::open(crate::paths::get_db_path()) {
        Ok(conn) => {
            // 1. Update the automation-specific counter
            if let Err(e) = increment_usage_count_by_trigger(&conn, trigger) {
                tracing::warn!(trigger, error = %e, "Failed to increment usage_count");
            }

            // 2. Update the global daily metrics
            let date = crate::metrics::get_current_date_string();
            let saved = crate::metrics::calculate_saved_keystrokes(output_len, delete_count);

            if let Err(e) = crate::db::crud::increment_metric(&conn, &date, 1, saved) {
                tracing::warn!(date, error = %e, "Failed to update daily metrics");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "record_expansion_usage: could not open DB");
        }
    }
}

/// Result of an `add_automation_by_trigger` call.
#[derive(Debug, Clone, PartialEq)]
pub enum AddOutcome {
    /// A brand-new automation was created.
    Created,
    /// An automation with the same trigger and identical output already exists.
    AlreadyExists,
    /// An automation with the same trigger existed but had a different output;
    /// the output (and `updated_at` / `version`) have been updated.
    Updated,
}

/// Creates or updates an automation using only its trigger and output.
///
/// - If no active automation exists for the trigger, a new row is inserted
///   and `AddOutcome::Created` is returned.
/// - If an active automation exists with the **same** output,
///   `AddOutcome::AlreadyExists` is returned and no writes happen.
/// - If an active automation exists with a **different** output, the output
///   is updated (along with `updated_at` and `version`) and
///   `AddOutcome::Updated` is returned.
pub fn add_automation_by_trigger(
    conn: &Connection,
    trigger: &str,
    output: &str,
) -> Result<AddOutcome> {
    // Check for an existing active row with this trigger.
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT id, output FROM automations WHERE trigger = ?1 AND is_deleted = 0 ORDER BY updated_at DESC LIMIT 1",
            [trigger],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();

    match existing {
        Some((_, existing_output)) if existing_output == output => {
            // Trigger and output are identical — nothing to do.
            Ok(AddOutcome::AlreadyExists)
        }
        Some((id, _)) => {
            // Same trigger, different output — update only the output.
            let now = now_unix_secs();
            conn.execute(
                "UPDATE automations
                 SET output     = ?1,
                     updated_at = ?2,
                     version    = version + 1
                 WHERE id = ?3",
                rusqlite::params![output, now, id],
            )?;
            Ok(AddOutcome::Updated)
        }
        None => {
            // No existing row — create a new one.
            let id = uuid::Uuid::new_v4().to_string();
            upsert_automation(
                conn, &id, trigger, None, trigger, output, "text", "all", "[]", 0, None,
            )?;
            Ok(AddOutcome::Created)
        }
    }
}
