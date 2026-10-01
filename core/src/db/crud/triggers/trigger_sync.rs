use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
use rusqlite::{Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};

use super::aliases::TriggerAliasRow;
use super::trigger_set::with_transaction;
use super::{TriggerRow, display_for_aliases, list_aliases};

/// Encapsulates all trigger data and its aliases for bidirectional cloud synchronization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudSnippetPayload {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub output: String,
    pub description: Option<String>,
    pub action_type: String,
    pub target_os: Option<String>,
    pub only_apps: Option<String>,
    pub except_apps: Option<String>,
    pub tags: Option<String>,
    pub is_enabled: bool,
    pub auto_case: bool,
    pub version: i64,
    pub is_deleted: bool,
    pub updated_at: i64,
    pub created_at: i64,
    pub aliases: Vec<TriggerAliasRow>,
}

/// The outcome of reconciling a remote cloud snippet payload with local SQLite state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    InsertedRemote,
    UpdatedFromRemote,
    LocalNewer,
    InSync,
}

/// Packages a local `TriggerRow` and its associated `TriggerAliasRow`s into a `CloudSnippetPayload`.
pub fn serialize_trigger_to_cloud_snippet(
    trigger: &TriggerRow,
    aliases: &[TriggerAliasRow],
) -> CloudSnippetPayload {
    let resolved_aliases = if aliases.is_empty() && !trigger.invocations.is_empty() {
        trigger.invocations.clone()
    } else {
        aliases.to_vec()
    };

    CloudSnippetPayload {
        id: trigger.id.clone(),
        workspace_id: trigger.workspace_id.clone(),
        name: trigger.name.clone(),
        output: trigger.output.clone(),
        description: trigger.description.clone(),
        action_type: trigger.action_type.clone(),
        target_os: if trigger.target_os.is_empty() {
            None
        } else {
            Some(trigger.target_os.clone())
        },
        only_apps: trigger.only_apps.clone(),
        except_apps: trigger.except_apps.clone(),
        tags: if trigger.tags.is_empty() {
            None
        } else {
            Some(trigger.tags.clone())
        },
        is_enabled: trigger.is_enabled,
        auto_case: trigger.auto_case,
        version: trigger.version,
        is_deleted: trigger.is_deleted,
        updated_at: trigger.updated_at,
        created_at: trigger.created_at,
        aliases: resolved_aliases,
    }
}

/// Reconciles an incoming remote `CloudSnippetPayload` with the local SQLite database.
///
/// Implements Last-Write-Wins (LWW) conflict resolution using `(version, updated_at)`:
/// - If no local record exists: inserts remote trigger and its aliases atomically.
/// - If remote is strictly newer (`version > local_version` or `version == local_version && updated_at > local_updated_at`):
///   overwrites local trigger row and atomically replaces all its aliases.
/// - If local is strictly newer: keeps local state without modification.
/// - If identical: marks in sync.
pub fn reconcile_cloud_snippet(
    conn: &Connection,
    snippet: &CloudSnippetPayload,
) -> crate::Result<ReconcileOutcome> {
    let local_row: Option<(i64, i64)> = conn
        .query_row(
            "SELECT version, updated_at FROM triggers WHERE id = ?1",
            [&snippet.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    match local_row {
        None => with_transaction(conn, || {
            conn.execute(
                "INSERT OR IGNORE INTO workspaces (id, name, is_default, created_at, updated_at, version, is_deleted, is_synced)
                 VALUES (?1, ?1, 0, ?2, ?2, 1, 0, 1)",
                rusqlite::params![snippet.workspace_id, snippet.created_at],
            )?;

            conn.execute(
                "INSERT INTO triggers (
                    id, workspace_id, name, description, output, action_type,
                    target_os, only_apps, except_apps, tags, is_enabled, auto_case,
                    version, is_deleted, is_synced, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 1, ?15, ?16)",
                rusqlite::params![
                    snippet.id,
                    snippet.workspace_id,
                    snippet.name,
                    snippet.description,
                    snippet.output,
                    snippet.action_type,
                    snippet.target_os.as_deref().unwrap_or("all"),
                    snippet.only_apps,
                    snippet.except_apps,
                    snippet.tags.as_deref().unwrap_or("[]"),
                    if snippet.is_enabled { 1 } else { 0 },
                    if snippet.auto_case { 1 } else { 0 },
                    snippet.version,
                    if snippet.is_deleted { 1 } else { 0 },
                    snippet.created_at,
                    snippet.updated_at,
                ],
            )?;

            for alias in &snippet.aliases {
                conn.execute(
                    "INSERT INTO trigger_aliases (
                        id, trigger_id, invocation, invocation_type, require_confirmation, strict_threshold, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        alias.id,
                        snippet.id,
                        alias.invocation,
                        alias.invocation_type.as_db_str(),
                        if alias.require_confirmation { 1 } else { 0 },
                        alias.strict_threshold,
                        alias.created_at,
                    ],
                )?;
            }

            Ok(ReconcileOutcome::InsertedRemote)
        }),
        Some((local_version, local_updated_at)) => {
            if snippet.version > local_version
                || (snippet.version == local_version && snippet.updated_at > local_updated_at)
            {
                with_transaction(conn, || {
                    conn.execute(
                        "INSERT OR IGNORE INTO workspaces (id, name, is_default, created_at, updated_at, version, is_deleted, is_synced)
                         VALUES (?1, ?1, 0, ?2, ?2, 1, 0, 1)",
                        rusqlite::params![snippet.workspace_id, snippet.created_at],
                    )?;

                    conn.execute(
                        "UPDATE triggers SET
                            workspace_id = ?1,
                            name = ?2,
                            description = ?3,
                            output = ?4,
                            action_type = ?5,
                            target_os = ?6,
                            only_apps = ?7,
                            except_apps = ?8,
                            tags = ?9,
                            is_enabled = ?10,
                            auto_case = ?11,
                            version = ?12,
                            is_deleted = ?13,
                            is_synced = 1,
                            updated_at = ?14
                         WHERE id = ?15",
                        rusqlite::params![
                            snippet.workspace_id,
                            snippet.name,
                            snippet.description,
                            snippet.output,
                            snippet.action_type,
                            snippet.target_os.as_deref().unwrap_or("all"),
                            snippet.only_apps,
                            snippet.except_apps,
                            snippet.tags.as_deref().unwrap_or("[]"),
                            if snippet.is_enabled { 1 } else { 0 },
                            if snippet.auto_case { 1 } else { 0 },
                            snippet.version,
                            if snippet.is_deleted { 1 } else { 0 },
                            snippet.updated_at,
                            snippet.id,
                        ],
                    )?;

                    conn.execute(
                        "DELETE FROM trigger_aliases WHERE trigger_id = ?1",
                        [&snippet.id],
                    )?;

                    for alias in &snippet.aliases {
                        conn.execute(
                            "INSERT INTO trigger_aliases (
                                id, trigger_id, invocation, invocation_type, require_confirmation, strict_threshold, created_at
                            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                            rusqlite::params![
                                alias.id,
                                snippet.id,
                                alias.invocation,
                                alias.invocation_type.as_db_str(),
                                if alias.require_confirmation { 1 } else { 0 },
                                alias.strict_threshold,
                                alias.created_at,
                            ],
                        )?;
                    }

                    Ok(ReconcileOutcome::UpdatedFromRemote)
                })
            } else if local_version > snippet.version
                || (local_version == snippet.version && local_updated_at > snippet.updated_at)
            {
                Ok(ReconcileOutcome::LocalNewer)
            } else {
                Ok(ReconcileOutcome::InSync)
            }
        }
    }
}

/// Marks a local trigger row as synchronized (`is_synced = 1`).
pub fn mark_trigger_synced(conn: &Connection, trigger_id: &str) -> crate::Result<()> {
    conn.execute(
        "UPDATE triggers SET is_synced = 1 WHERE id = ?1",
        [trigger_id],
    )?;
    Ok(())
}

/// Purges soft-deleted rows older than `max_age_seconds` permanently from the database.
pub fn purge_deleted_tombstones(conn: &Connection, max_age_seconds: i64) -> crate::Result<usize> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let cutoff = now.saturating_sub(max_age_seconds);

    with_transaction(conn, || {
        conn.execute(
            "DELETE FROM trigger_aliases WHERE trigger_id IN (SELECT id FROM triggers WHERE is_deleted = 1 AND updated_at <= ?1)",
            [cutoff],
        )?;
        let count = conn.execute(
            "DELETE FROM triggers WHERE is_deleted = 1 AND updated_at <= ?1",
            [cutoff],
        )?;
        Ok(count)
    })
}

/// Returns all triggers that have local modifications pending upload (`is_synced = 0`).
pub fn get_unsynced_triggers(conn: &Connection) -> Result<Vec<TriggerRow>> {
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
            s.compressed_content,
            a.workspace_id
         FROM triggers a
         LEFT JOIN scripts s ON a.id = s.trigger_id
         WHERE a.is_synced = 0
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
            workspace_id: row.get(21)?,
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

/// Returns all triggers that are configured by the user to be synced to the cloud.
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
            s.compressed_content,
            a.workspace_id
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
            workspace_id: row.get(21)?,
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
