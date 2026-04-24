use crate::Result;
use rusqlite::Connection;

use crate::db::{crud::get_setting_value, now_unix_secs};

use super::{TriggerConflict, TriggerType};

const INLINE_AI_RESERVED_TRIGGER: &str = "ai";

fn current_trigger_char(conn: &Connection) -> char {
    if let Ok(Some(val)) = get_setting_value(conn, "trigger_char")
        && let Ok(v) = serde_json::from_str::<String>(&val)
        && let Some(c) = v.chars().next()
    {
        return c;
    }

    '>'
}

fn is_reserved_inline_ai_trigger(conn: &Connection, trigger: &str) -> bool {
    if trigger == INLINE_AI_RESERVED_TRIGGER {
        return true;
    }

    trigger
        == format!(
            "{}{}",
            current_trigger_char(conn),
            INLINE_AI_RESERVED_TRIGGER
        )
}

pub fn validate_trigger_not_reserved(conn: &Connection, trigger: &str) -> Result<()> {
    if is_reserved_inline_ai_trigger(conn, trigger) {
        return Err(crate::Error::Config(format!(
            "Trigger '{}' is reserved for Taurine Inline AI Copilot",
            trigger
        )));
    }

    Ok(())
}

fn validate_trigger_type(trigger_type: TriggerType, target_os: &str) -> Result<()> {
    if matches!(trigger_type, TriggerType::Hotkey) && matches!(target_os, "android" | "ios") {
        return Err(crate::Error::Config(format!(
            "Hotkey triggers are only supported for desktop target_os values; got '{}'",
            target_os
        )));
    }

    Ok(())
}

pub fn target_os_values_overlap(left: &str, right: &str) -> bool {
    left == right || left == "all" || right == "all"
}

pub fn find_trigger_overlap_conflict(
    conn: &Connection,
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
    exclude_id: Option<&str>,
) -> Result<Option<TriggerConflict>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, trigger_type, trigger, target_os
         FROM automations
         WHERE trigger_type = ?1
           AND trigger = ?2
           AND is_deleted = 0
         ORDER BY updated_at DESC",
    )?;

    let rows = stmt.query_map([trigger_type.as_db_str(), trigger], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    for row in rows {
        let (id, trigger_type_raw, trigger, existing_target_os) = row?;
        if exclude_id.is_some_and(|excluded| excluded == id) {
            continue;
        }

        if target_os_values_overlap(&existing_target_os, target_os) {
            return Ok(Some(TriggerConflict {
                id,
                trigger_type: TriggerType::parse_db(&trigger_type_raw)?,
                trigger,
                target_os: existing_target_os,
            }));
        }
    }

    Ok(None)
}

pub fn validate_trigger_target_os_conflict(
    conn: &Connection,
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
    exclude_id: Option<&str>,
) -> Result<()> {
    validate_trigger_type(trigger_type, target_os)?;

    if let Some(conflict) =
        find_trigger_overlap_conflict(conn, trigger_type, trigger, target_os, exclude_id)?
    {
        return Err(crate::Error::Config(format!(
            "Trigger conflict for {} '{}' on target_os '{}': overlaps existing target_os '{}'",
            trigger_type.as_db_str(),
            trigger,
            target_os,
            conflict.target_os
        )));
    }

    Ok(())
}

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
    upsert_automation_with_trigger_type(
        conn,
        id,
        name,
        description,
        TriggerType::Word,
        trigger,
        output,
        action_type,
        target_os,
        tags_json,
        usage_count,
        last_used_at,
    )
}

/// Inserts a new automation or updates an existing one with an explicit trigger type.
#[allow(clippy::too_many_arguments)]
pub fn upsert_automation_with_trigger_type(
    conn: &Connection,
    id: &str,
    name: &str,
    description: Option<&str>,
    trigger_type: TriggerType,
    trigger: &str,
    output: &str,
    action_type: &str,
    target_os: &str,
    tags_json: &str, // JSON string
    usage_count: i64,
    last_used_at: Option<i64>,
) -> Result<()> {
    validate_trigger_not_reserved(conn, trigger)?;
    validate_trigger_target_os_conflict(conn, trigger_type, trigger, target_os, Some(id))?;

    let now = now_unix_secs();

    // Keep created_at stable across updates.
    conn.execute(
        "INSERT INTO automations
            (id, name, description, trigger_type, trigger, output, action_type, target_os, tags,
             usage_count, last_used_at, created_at, updated_at, version, is_deleted)
         VALUES
            (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
             ?10, ?11, ?12, ?13, 1, 0)
         ON CONFLICT(id) DO UPDATE SET
            name         = excluded.name,
            description  = excluded.description,
            trigger_type = excluded.trigger_type,
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
            trigger_type.as_db_str(),
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

/// Inserts or updates a script attachment for an automation.
pub fn upsert_script(
    conn: &Connection,
    automation_id: &str,
    interpreter: crate::engine::shell::ScriptInterpreter,
    behavior: crate::engine::shell::ScriptBehavior,
    compressed_content: &[u8],
) -> Result<()> {
    let now = now_unix_secs();
    conn.execute(
        "INSERT INTO scripts (automation_id, interpreter, behavior, compressed_content, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(automation_id) DO UPDATE SET
            interpreter        = excluded.interpreter,
            behavior           = excluded.behavior,
            compressed_content = excluded.compressed_content,
            updated_at         = excluded.updated_at,
            version            = version + 1",
        (
            automation_id,
            serde_json::to_string(&interpreter)?,
            serde_json::to_string(&behavior)?,
            compressed_content,
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
pub fn record_expansion_usage(
    trigger: &str,
    output_len: usize,
    delete_count: usize,
    left_arrow_count: usize,
) {
    match Connection::open(crate::paths::get_db_path()) {
        Ok(mut conn) => {
            // Use a closure to handle the transaction and custom Result type.
            let tx_result = (|| -> crate::Result<()> {
                let tx = conn.transaction()?;

                // 1. Update the automation-specific counter
                increment_usage_count_by_trigger(&tx, trigger)?;

                // 2. Update the global daily metrics
                let date = crate::metrics::get_current_date_string();
                let saved = crate::metrics::calculate_saved_keystrokes(
                    output_len,
                    delete_count,
                    left_arrow_count,
                );

                crate::db::crud::increment_metric(&tx, &date, 1, saved)?;

                tx.commit()?;
                Ok(())
            })();

            if let Err(e) = tx_result {
                tracing::warn!(
                    trigger,
                    error = %e,
                    "Failed to record expansion usage transactionally"
                );
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
    target_os: &str,
) -> Result<AddOutcome> {
    add_automation_by_trigger_type(conn, TriggerType::Word, trigger, output, target_os)
}

pub fn add_automation_by_trigger_type(
    conn: &Connection,
    trigger_type: TriggerType,
    trigger: &str,
    output: &str,
    target_os: &str,
) -> Result<AddOutcome> {
    validate_trigger_not_reserved(conn, trigger)?;

    // Check for an existing active row with this trigger and target_os.
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT id, output
             FROM automations
             WHERE trigger_type = ?1
               AND trigger = ?2
               AND target_os = ?3
               AND is_deleted = 0
             ORDER BY updated_at DESC
             LIMIT 1",
            [trigger_type.as_db_str(), trigger, target_os],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();

    match existing {
        Some((_, existing_output)) if existing_output == output => {
            // Trigger, OS, and output are identical — nothing to do.
            Ok(AddOutcome::AlreadyExists)
        }
        Some((id, _)) => {
            // Same trigger/OS, different output — update only the output.
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
            validate_trigger_target_os_conflict(conn, trigger_type, trigger, target_os, None)?;

            // No existing row — create a new one.
            let id = uuid::Uuid::new_v4().to_string();
            upsert_automation_with_trigger_type(
                conn,
                &id,
                trigger,
                None,
                trigger_type,
                trigger,
                output,
                "text",
                target_os,
                "[]",
                0,
                None,
            )?;
            Ok(AddOutcome::Created)
        }
    }
}
