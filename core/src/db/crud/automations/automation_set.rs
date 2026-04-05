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
    payload: &str,
    action_type: &str,
    is_regex: bool,
    target_os: &str,
    tags_json: &str, // JSON string
    usage_count: i64,
    last_used_at: Option<i64>,
) -> Result<()> {
    let now = now_unix_secs();

    // Keep created_at stable across updates.
    conn.execute(
        "INSERT INTO automations
            (id, name, description, trigger, payload, action_type, is_regex, target_os, tags,
             usage_count, last_used_at, created_at, updated_at, version, is_deleted)
         VALUES
            (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
             ?10, ?11, ?12, ?13, 1, 0)
         ON CONFLICT(id) DO UPDATE SET
            name         = excluded.name,
            description  = excluded.description,
            trigger      = excluded.trigger,
            payload      = excluded.payload,
            action_type  = excluded.action_type,
            is_regex     = excluded.is_regex,
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
            payload,
            action_type,
            is_regex,
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
/// whose trigger matches.
///
/// Intended for callers (e.g. the daemon hook thread) that do not hold an open
/// `Connection` and do not want a direct dependency on `rusqlite`.
pub fn record_expansion_usage(trigger: &str) {
    match Connection::open(crate::paths::get_db_path()) {
        Ok(conn) => {
            if let Err(e) = increment_usage_count_by_trigger(&conn, trigger) {
                tracing::warn!(trigger, error = %e, "Failed to increment usage_count");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "record_expansion_usage: could not open DB");
        }
    }
}
