use super::{ExchangePayload, StatExport, TriggerExport};
use crate::db::crud::{
    TriggerType, increment_stat, target_os_values_overlap, upsert_script, upsert_setting,
    upsert_trigger_with_type,
};
use crate::engine::shell::compress;
use crate::keys::normalize_hotkey;
use rusqlite::{Connection, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportConflictAction {
    Overwrite,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportStatsMode {
    #[default]
    Ignore,
    Merge,
    Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportOptions {
    pub include_settings: bool,
    pub stats_mode: ImportStatsMode,
    pub include_sensitive_settings: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingTriggerConflict {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger_type: TriggerType,
    pub trigger: String,
    pub output: String,
    pub action_type: String,
    pub target_os: String,
    pub is_enabled: bool,
    pub usage_count: i64,
    pub last_used_at: Option<i64>,
}

pub fn import_triggers<F>(
    tx: &Transaction<'_>,
    payload: &ExchangePayload,
    options: ImportOptions,
    mut resolve_conflict: F,
) -> crate::Result<usize>
where
    F: FnMut(&TriggerExport, &ExistingTriggerConflict) -> crate::Result<ImportConflictAction>,
{
    payload.validate_schema_version()?;

    let mut imported = 0usize;
    for trigger in &payload.triggers {
        let canonical_trigger = match trigger.trigger_type {
            TriggerType::Hotkey => {
                normalize_hotkey(&trigger.trigger).unwrap_or_else(|_| trigger.trigger.clone())
            }
            _ => trigger.trigger.clone(),
        };

        let existing = find_conflicting_trigger(
            tx,
            trigger.trigger_type,
            &canonical_trigger,
            trigger.target_os.as_str(),
        )?;

        if let Some(existing) = existing.as_ref() {
            match resolve_conflict(trigger, existing)? {
                ImportConflictAction::Overwrite => {
                    tombstone_conflicting_triggers(
                        tx,
                        trigger.trigger_type,
                        &canonical_trigger,
                        trigger.target_os.as_str(),
                    )?;
                }
                ImportConflictAction::Skip => continue,
            }
        }

        let mut canonical_trigger_export = trigger.clone();
        canonical_trigger_export.trigger = canonical_trigger;
        insert_imported_trigger(
            tx,
            &canonical_trigger_export,
            existing.as_ref(),
            options.stats_mode,
        )?;
        imported += 1;
    }

    if options.include_settings {
        import_settings(tx, payload, options.include_sensitive_settings)?;
    }

    import_global_stats(tx, payload, options.stats_mode)?;

    Ok(imported)
}

pub fn import_payload_transactionally<F>(
    conn: &mut Connection,
    payload: &ExchangePayload,
    options: ImportOptions,
    mut resolve_conflict: F,
) -> crate::Result<usize>
where
    F: FnMut(&TriggerExport, &ExistingTriggerConflict) -> crate::Result<ImportConflictAction>,
{
    let tx = conn.transaction()?;
    let result = import_triggers(&tx, payload, options, |incoming, existing| {
        resolve_conflict(incoming, existing)
    });

    match result {
        Ok(imported) => {
            tx.commit()?;
            Ok(imported)
        }
        Err(err) => {
            tx.rollback()?;
            Err(err)
        }
    }
}

fn insert_imported_trigger(
    tx: &Transaction<'_>,
    trigger: &TriggerExport,
    existing: Option<&ExistingTriggerConflict>,
    stats_mode: ImportStatsMode,
) -> crate::Result<()> {
    let id = Uuid::new_v4().to_string();
    let tags_json = serde_json::to_string(&trigger.tags)?;
    let (usage_count, last_used_at) = resolve_trigger_stats(trigger, existing, stats_mode);

    upsert_trigger_with_type(
        tx,
        &id,
        &trigger.name,
        trigger.description.as_deref(),
        trigger.trigger_type,
        &trigger.trigger,
        &trigger.output,
        &trigger.action_type,
        &trigger.target_os,
        &tags_json,
        usage_count,
        last_used_at,
    )?;

    if !trigger.is_enabled {
        tx.execute(
            "UPDATE triggers
             SET is_enabled = 0
             WHERE id = ?1",
            [&id],
        )?;
    }

    if trigger.action_type == "script" {
        let script = trigger.script.as_ref().ok_or_else(|| {
            crate::Error::Config(format!(
                "Script trigger '{}' is missing script data",
                trigger.trigger
            ))
        })?;
        let compressed = compress(&script.content)?;
        upsert_script(tx, &id, script.interpreter, script.behavior, &compressed)?;
    }

    let now = crate::db::now_unix_secs();
    // To prevent asset link corruption, rewrite asset UUIDs.
    // Map of old asset ID to new asset ID
    let mut asset_id_map = std::collections::HashMap::new();

    for asset in &trigger.assets {
        let new_asset_id = Uuid::new_v4().to_string();
        asset_id_map.insert(asset.id.clone(), new_asset_id.clone());

        let compressed = hex::decode(&asset.compressed_content_hex)
            .map_err(|e| crate::Error::Config(format!("Failed to decode asset hex: {}", e)))?;
        tx.execute(
            "INSERT OR REPLACE INTO assets (id, trigger_id, mime_type, compressed_content, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                &new_asset_id,
                &id,
                &asset.mime_type,
                &compressed,
                now,
            ),
        )?;
    }

    // Now, rewrite any references to the old asset UUIDs in the output and script content
    if !asset_id_map.is_empty() {
        let mut final_output = trigger.output.clone();
        for (old_id, new_id) in &asset_id_map {
            final_output = final_output.replace(old_id, new_id);
        }

        if final_output != trigger.output {
            tx.execute(
                "UPDATE triggers SET output = ?1 WHERE id = ?2",
                [&final_output, &id],
            )?;
        }

        if trigger.action_type == "script" {
            let script = trigger.script.as_ref().unwrap();
            let mut final_script_content = script.content.clone();
            for (old_id, new_id) in &asset_id_map {
                final_script_content = final_script_content.replace(old_id, new_id);
            }
            if final_script_content != script.content {
                let compressed = compress(&final_script_content)?;
                tx.execute(
                    "UPDATE scripts SET content = ?1 WHERE trigger_id = ?2",
                    rusqlite::params![&compressed, &id],
                )?;
            }
        }
    }

    Ok(())
}

fn find_conflicting_trigger(
    tx: &Transaction<'_>,
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
) -> crate::Result<Option<ExistingTriggerConflict>> {
    let mut stmt = tx.prepare_cached(
        "SELECT id, name, description, trigger_type, trigger, output, action_type, target_os, is_enabled,
                usage_count, last_used_at
         FROM triggers
         WHERE trigger_type = ?1
           AND trigger = ?2
           AND is_deleted = 0
         ORDER BY updated_at DESC",
    )?;

    let rows = stmt.query_map([trigger_type.as_db_str(), trigger], |row| {
        let trigger_type_raw: String = row.get(3)?;
        Ok(ExistingTriggerConflict {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            trigger_type: TriggerType::parse_db(&trigger_type_raw).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?,
            trigger: row.get(4)?,
            output: row.get(5)?,
            action_type: row.get(6)?,
            target_os: row.get(7)?,
            is_enabled: row.get(8)?,
            usage_count: row.get(9)?,
            last_used_at: row.get(10)?,
        })
    })?;

    for row in rows {
        let row = row?;
        if target_os_values_overlap(&row.target_os, target_os) {
            return Ok(Some(row));
        }
    }

    Ok(None)
}

fn tombstone_conflicting_triggers(
    tx: &Transaction<'_>,
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
) -> crate::Result<()> {
    let mut stmt = tx.prepare_cached(
        "SELECT id, target_os
         FROM triggers
         WHERE trigger_type = ?1
           AND trigger = ?2
           AND is_deleted = 0",
    )?;
    let rows = stmt.query_map([trigger_type.as_db_str(), trigger], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let now = crate::db::now_unix_secs();
    for row in rows {
        let (id, existing_target_os) = row?;
        if target_os_values_overlap(&existing_target_os, target_os) {
            tx.execute(
                "UPDATE triggers
                 SET is_deleted = 1,
                     version = version + 1,
                     updated_at = ?1
                 WHERE id = ?2",
                rusqlite::params![now, id],
            )?;
        }
    }

    Ok(())
}

fn resolve_trigger_stats(
    trigger: &TriggerExport,
    existing: Option<&ExistingTriggerConflict>,
    stats_mode: ImportStatsMode,
) -> (i64, Option<i64>) {
    let imported_usage_count = trigger.usage_count.unwrap_or(0);
    let imported_last_used_at = trigger.last_used_at;

    match stats_mode {
        ImportStatsMode::Ignore => (0, None),
        ImportStatsMode::Overwrite => (imported_usage_count, imported_last_used_at),
        ImportStatsMode::Merge => {
            if let Some(existing) = existing {
                (
                    existing.usage_count + imported_usage_count,
                    max_option_i64(existing.last_used_at, imported_last_used_at),
                )
            } else {
                (imported_usage_count, imported_last_used_at)
            }
        }
    }
}

fn import_settings(
    tx: &Transaction<'_>,
    payload: &ExchangePayload,
    include_sensitive_settings: bool,
) -> crate::Result<()> {
    if let Some(settings) = payload.settings.as_ref() {
        for setting in settings {
            if !include_sensitive_settings
                && crate::exchange::export::is_sensitive_setting_key(&setting.key)
            {
                continue;
            }
            upsert_setting(tx, &setting.key, &setting.value)?;
        }
    }

    Ok(())
}

fn import_global_stats(
    tx: &Transaction<'_>,
    payload: &ExchangePayload,
    stats_mode: ImportStatsMode,
) -> crate::Result<()> {
    let Some(stats) = payload.stats.as_ref() else {
        return Ok(());
    };

    match stats_mode {
        ImportStatsMode::Ignore => Ok(()),
        ImportStatsMode::Merge => {
            for stat in stats {
                increment_stat(
                    tx,
                    &stat.date,
                    stat.executions,
                    stat.ai_executions,
                    stat.keystrokes_saved,
                    stat.time_saved_ms,
                )?;
            }
            Ok(())
        }
        ImportStatsMode::Overwrite => {
            for stat in stats {
                overwrite_stat_row(tx, stat)?;
            }
            Ok(())
        }
    }
}

fn overwrite_stat_row(tx: &Transaction<'_>, stat: &StatExport) -> crate::Result<()> {
    tx.execute(
        "INSERT INTO stats (
             date,
             executions,
             ai_executions,
             keystrokes_saved,
             time_saved_ms,
             version,
             updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
         ON CONFLICT(date) DO UPDATE SET
             executions = excluded.executions,
             ai_executions = excluded.ai_executions,
             keystrokes_saved = excluded.keystrokes_saved,
             time_saved_ms = excluded.time_saved_ms,
             version = version + 1,
             updated_at = excluded.updated_at",
        (
            &stat.date,
            stat.executions,
            stat.ai_executions,
            stat.keystrokes_saved,
            stat.time_saved_ms,
            crate::db::now_unix_secs(),
        ),
    )?;

    Ok(())
}

fn max_option_i64(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}
