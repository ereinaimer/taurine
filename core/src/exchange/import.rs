use super::{ExchangePayload, TriggerExport};
use crate::db::crud::{
    InvocationType, NewEntry, TriggerAliasRow, TriggerType, create_entry, display_for_aliases,
    list_aliases, target_os_values_overlap, tombstone_entry, validate_voice_phrase,
};
use crate::engine::shell::{compress, decompress};
use crate::keys::normalize_hotkey;
use rusqlite::{Connection, Transaction};
use uuid::Uuid;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportConflictAction {
    #[default]
    Overwrite,
    Skip,
}

impl ImportConflictAction {
    pub const ALL: [Self; 2] = [Self::Overwrite, Self::Skip];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Overwrite => "overwrite",
            Self::Skip => "skip",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Overwrite => "Overwrite",
            Self::Skip => "Skip",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "overwrite" | "over" | "replace" => Some(Self::Overwrite),
            "skip" | "ignore" => Some(Self::Skip),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExistingTriggerConflict {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub invocations: Vec<TriggerAliasRow>,
    pub display: String,
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
    mut resolve_conflict: F,
) -> crate::Result<usize>
where
    F: FnMut(&TriggerExport, &ExistingTriggerConflict) -> crate::Result<ImportConflictAction>,
{
    payload.validate_schema_version()?;

    let mut imported = 0usize;
    for trigger in &payload.triggers {
        let invocations = import_invocations(trigger);

        let mut conflicts: Vec<ExistingTriggerConflict> = Vec::new();
        for (invocation_type, invocation, _) in &invocations {
            if let Some(existing) =
                find_conflicting_trigger(tx, *invocation_type, invocation, &trigger.target_os)?
                && !conflicts.iter().any(|conflict| conflict.id == existing.id)
            {
                conflicts.push(existing);
            }
        }

        if let Some(first) = conflicts.first() {
            match resolve_conflict(trigger, first)? {
                ImportConflictAction::Overwrite => {
                    let now = crate::db::now_unix_secs();
                    for conflict in &conflicts {
                        tombstone_entry(tx, &conflict.id, now)?;
                    }
                }
                ImportConflictAction::Skip => continue,
            }
        }

        insert_imported_entry(tx, trigger, &invocations)?;
        imported += 1;
    }

    Ok(imported)
}

pub fn import_payload_transactionally<F>(
    conn: &mut Connection,
    payload: &ExchangePayload,
    mut resolve_conflict: F,
) -> crate::Result<usize>
where
    F: FnMut(&TriggerExport, &ExistingTriggerConflict) -> crate::Result<ImportConflictAction>,
{
    let tx = conn.transaction()?;
    let result = import_triggers(&tx, payload, |incoming, existing| {
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

fn import_invocations(trigger: &TriggerExport) -> Vec<(InvocationType, String, bool)> {
    if trigger.aliases.is_empty() {
        let invocation_type = match trigger.trigger_type {
            TriggerType::Word => InvocationType::Word,
            TriggerType::Hotkey => InvocationType::Hotkey,
            TriggerType::Regex => InvocationType::Regex,
        };
        vec![(invocation_type, trigger.trigger.clone(), false)]
    } else {
        trigger
            .aliases
            .iter()
            .map(|alias| {
                (
                    alias.invocation_type,
                    alias.invocation.clone(),
                    alias.require_confirmation,
                )
            })
            .collect()
    }
}

fn insert_imported_entry(
    tx: &Transaction<'_>,
    trigger: &TriggerExport,
    invocations: &[(InvocationType, String, bool)],
) -> crate::Result<String> {
    let script = if trigger.action_type == "script" {
        Some(trigger.script.as_ref().ok_or_else(|| {
            crate::Error::Config(format!(
                "Script trigger '{}' is missing script data",
                trigger.trigger
            ))
        })?)
    } else {
        None
    };
    let (content, interpreter, behavior) = match script {
        Some(script) => (
            script.content.clone(),
            Some(script.interpreter),
            Some(script.behavior),
        ),
        None => (trigger.output.clone(), None, None),
    };
    let tags_json = serde_json::to_string(&trigger.tags)?;

    let (id, _) = create_entry(
        tx,
        NewEntry {
            name: trigger.name.clone(),
            description: trigger.description.clone(),
            content,
            action_type: trigger.action_type.clone(),
            target_os: trigger.target_os.clone(),
            only_apps: None,
            except_apps: None,
            tags_json,
            auto_case: false,
            interpreter,
            behavior,
            invocations: invocations.to_vec(),
        },
    )?;

    if !trigger.is_enabled {
        tx.execute(
            "UPDATE triggers
             SET is_enabled = 0
             WHERE id = ?1",
            [&id],
        )?;
    }

    rewrite_imported_asset_ids(tx, &id, trigger)?;

    Ok(id)
}

fn rewrite_imported_asset_ids(
    tx: &Transaction<'_>,
    id: &str,
    trigger: &TriggerExport,
) -> crate::Result<()> {
    if trigger.assets.is_empty() {
        return Ok(());
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
    let stored_output: String =
        tx.query_row("SELECT output FROM triggers WHERE id = ?1", [id], |row| {
            row.get(0)
        })?;
    let mut final_output = stored_output.clone();
    for (old_id, new_id) in &asset_id_map {
        final_output = final_output.replace(old_id, new_id);
    }
    if final_output != stored_output {
        tx.execute(
            "UPDATE triggers SET output = ?1 WHERE id = ?2",
            [&final_output, id],
        )?;
    }

    if trigger.action_type == "script" && trigger.script.is_some() {
        let blob: Vec<u8> = tx.query_row(
            "SELECT compressed_content FROM scripts WHERE trigger_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let stored_content = decompress(&blob)?;
        let mut final_script_content = stored_content.clone();
        for (old_id, new_id) in &asset_id_map {
            final_script_content = final_script_content.replace(old_id, new_id);
        }
        if final_script_content != stored_content {
            let compressed = compress(&final_script_content)?;
            tx.execute(
                "UPDATE scripts SET compressed_content = ?1 WHERE trigger_id = ?2",
                rusqlite::params![&compressed, &id],
            )?;
        }
    }

    Ok(())
}

fn find_conflicting_trigger(
    tx: &Transaction<'_>,
    invocation_type: InvocationType,
    invocation: &str,
    target_os: &str,
) -> crate::Result<Option<ExistingTriggerConflict>> {
    let lookup = match invocation_type {
        InvocationType::Hotkey => {
            normalize_hotkey(invocation).unwrap_or_else(|_| invocation.to_string())
        }
        InvocationType::Voice => {
            validate_voice_phrase(invocation).unwrap_or_else(|_| invocation.to_string())
        }
        InvocationType::Word | InvocationType::Regex => invocation.to_string(),
    };
    let mut stmt = tx.prepare_cached(
        "SELECT id, name, description, output, action_type, target_os, is_enabled,
                usage_count, last_used_at
         FROM triggers
         WHERE id IN (SELECT trigger_id FROM trigger_aliases
                      WHERE invocation_type = ?1 AND invocation = ?2)
           AND is_deleted = 0
         ORDER BY updated_at DESC",
    )?;

    let rows = stmt.query_map([invocation_type.as_db_str(), lookup.as_str()], |row| {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        // N+1, fine under ~1k rows; batch with a single IN query if it grows
        let invocations = list_aliases(tx, &id).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })?;
        Ok(ExistingTriggerConflict {
            display: display_for_aliases(&name, &invocations),
            id,
            name,
            description: row.get(2)?,
            invocations,
            output: row.get(3)?,
            action_type: row.get(4)?,
            target_os: row.get(5)?,
            is_enabled: row.get(6)?,
            usage_count: row.get(7)?,
            last_used_at: row.get(8)?,
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
