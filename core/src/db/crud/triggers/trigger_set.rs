use super::aliases::{
    InvocationType, TriggerAliasRow, add_alias, find_parent_by_invocation, validate_voice_phrase,
};
use super::app_filter::AppFilterPrefix;
use super::assets::*;
use super::overlap::*;
use super::trigger_types::TriggerLimits;
use super::validate::*;
use unicode_normalization::UnicodeNormalization;

use crate::Result;
use rusqlite::{Connection, OptionalExtension};

use crate::db::now_unix_secs;
use crate::engine::{
    shell::{ScriptBehavior, ScriptInterpreter, compress, decompress, infer_interpreter},
    variables::system::validate_output,
};
use crate::keys::{
    HotkeyPlatform, conflicts_with_taurine_global_hotkey, danger_for_platform, parse_hotkey,
};

use super::TriggerType;
use super::trigger_types;

pub(crate) const MAX_TAG_LENGTH: usize = 50;

pub(crate) const MAX_TAGS_COUNT: usize = 20;

pub(crate) const MAX_TRIGGER_LENGTH: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTrigger {
    pub trigger_type: TriggerType,
    pub stored_trigger: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingTriggerUpdate<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub trigger_type: TriggerType,
    pub trigger: &'a str,
    pub content: &'a str,
    pub action_type: &'a str,
    pub target_os: &'a str,
    pub tags_json: &'a str,
    pub auto_case: bool,
    pub usage_count: i64,
    pub last_used_at: Option<i64>,
    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTrigger<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub trigger_type: TriggerType,
    pub trigger: &'a str,
    pub content: &'a str,
    pub action_type: &'a str,
    pub target_os: &'a str,
    pub tags_json: &'a str,
    pub auto_case: bool,
    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
}

pub fn prepare_trigger(
    trigger: &str,
    use_hotkey: bool,
    target_os: &str,
) -> Result<PreparedTrigger> {
    let trigger_type = if use_hotkey {
        TriggerType::Hotkey
    } else {
        TriggerType::Word
    };
    prepare_trigger_with_type(trigger, trigger_type, target_os)
}

pub fn prepare_trigger_with_type(
    trigger: &str,
    trigger_type: TriggerType,
    target_os: &str,
) -> Result<PreparedTrigger> {
    if trigger.trim().is_empty() {
        return Err(crate::Error::Config("Trigger cannot be empty.".to_string()));
    }

    if trigger.len() > MAX_TRIGGER_LENGTH {
        return Err(crate::Error::Config(format!(
            "Trigger exceeds {} character limit",
            MAX_TRIGGER_LENGTH
        )));
    }

    if trigger_type == TriggerType::Word && (trigger.contains('\n') || trigger.contains('\r')) {
        return Err(crate::Error::Config(
            "Word triggers cannot contain newlines.".to_string(),
        ));
    }

    if matches!(trigger_type, TriggerType::Word | TriggerType::Regex) {
        return Ok(PreparedTrigger {
            trigger_type,
            stored_trigger: trigger.to_string(),
        });
    }

    let hotkey = parse_hotkey(trigger)
        .map_err(|error| crate::Error::Config(format!("Invalid hotkey {trigger}: {error}")))?;
    let canonical = hotkey.canonical_string();

    if conflicts_with_taurine_global_hotkey(hotkey).is_some() {
        let diag = crate::diagnostic::Diagnostic::problem(format!(
            "Hotkey {canonical} conflicts with Taurine's global pause hotkey"
        ))
        .help("alt+` is reserved globally to pause and resume Taurine expansion.")
        .example("taurine config set pause_hotkey <NEW_HOTKEY>")
        .render();
        return Err(crate::Error::Config(diag));
    }

    for platform in desktop_platforms_for_target_os(target_os)? {
        if let Some(danger) = danger_for_platform(hotkey, *platform) {
            let key_name = hotkey.logical_key().canonical_name();
            let diag = crate::diagnostic::Diagnostic::problem(format!(
                "Hotkey {canonical} is not allowed for target_os {target_os}: conflicts with the system {desc} on {platform_label}",
                desc = danger.description(),
                platform_label = platform.as_label(),
            ))
            .help(format!(
                "To avoid overriding system shortcuts on {target_os}, use an alternative modifier combination."
            ))
            .example(format!("alt+{key_name}"))
            .example(format!("super+{key_name}"))
            .render();
            return Err(crate::Error::Config(diag));
        }
    }

    Ok(PreparedTrigger {
        trigger_type,
        stored_trigger: canonical,
    })
}

/// Inserts a new trigger or updates an existing one.
///
/// Semantics:
/// - On **insert**: `version` starts at `1`, `created_at`/`updated_at` are set
///   to "now".
/// - On **update**: `version` is incremented by `1` atomically.
/// - `is_deleted` is forced to `0` (reactivates tombstoned rows).
/// - `is_synced` is forced to `0` so the sync layer can enqueue this record.
#[allow(clippy::too_many_arguments)]
pub fn upsert_trigger(
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
    upsert_trigger_with_type_and_case(
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
        false,
    )
}

/// Inserts a new trigger or updates an existing one with an explicit trigger type.
#[allow(clippy::too_many_arguments)]
pub fn upsert_trigger_with_type(
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
    upsert_trigger_with_type_and_case(
        conn,
        id,
        name,
        description,
        trigger_type,
        trigger,
        output,
        action_type,
        target_os,
        tags_json,
        usage_count,
        last_used_at,
        false,
    )
}

/// Inserts a new trigger or updates an existing one with an explicit trigger type, plus auto_case.
///
/// New-model body: id-addressed parent write (insert version 1, update
/// version+1, `created_at` stable, tombstones reactivated) plus a monotonic
/// alias ensure — the invocation is added when missing, never removed, and
/// idempotent on re-upsert. A clash with another SCOPE-OVERLAPPING parent's
/// alias is a `Config` conflict error; disjoint scopes share the invocation
/// on separate parents. Parent write + alias ensure run in one transaction.
#[allow(clippy::too_many_arguments)]
pub fn upsert_trigger_with_type_and_case(
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
    auto_case: bool,
) -> Result<()> {
    with_transaction(conn, || {
        let now = now_unix_secs();

        // Keep created_at stable across updates.
        conn.execute(
            "INSERT INTO triggers
            (id, name, description, output, action_type, target_os, tags,
             usage_count, last_used_at, created_at, updated_at, version, is_deleted, auto_case)
         VALUES
            (?1, ?2, ?3, ?4, ?5, ?6, ?7,
             ?8, ?9, ?10, ?10, 1, 0, ?11)
         ON CONFLICT(id) DO UPDATE SET
            name         = excluded.name,
            description  = excluded.description,
            output       = excluded.output,
            action_type  = excluded.action_type,
            target_os    = excluded.target_os,
            tags         = excluded.tags,
            usage_count  = excluded.usage_count,
            last_used_at = excluded.last_used_at,
            is_deleted   = 0,
            auto_case    = excluded.auto_case,
            version      = version + 1,
            updated_at   = excluded.updated_at",
            (
                id,
                name,
                description,
                output,
                action_type,
                target_os,
                tags_json,
                usage_count,
                last_used_at,
                now,
                auto_case,
            ),
        )?;

        if action_type == "text" {
            let processed_output = compile_and_save_assets(conn, id, output)?;
            if processed_output != output {
                conn.execute(
                    "UPDATE triggers SET output = ?1, updated_at = ?2 WHERE id = ?3",
                    (&processed_output, now, id),
                )?;
            }
        }

        // This path carries no app filters, so the written scope is
        // (`target_os`, no filters); only an overlapping holder conflicts.
        let invocation_type = invocation_type_for_trigger(trigger_type);
        let text_nfc: String = trigger.nfc().collect();
        let holders = parents_by_invocation(conn, invocation_type, &text_nfc)?;
        for pid in &holders {
            if pid == id {
                continue;
            }
            let (hit_os, hit_only, hit_except): (String, Option<String>, Option<String>) = conn
                .query_row(
                    "SELECT target_os, only_apps, except_apps FROM triggers WHERE id = ?1",
                    [pid],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
            if target_os_values_overlap(target_os, &hit_os)
                && app_filters_overlap(None, None, hit_only.as_deref(), hit_except.as_deref())
            {
                return Err(crate::Error::Config(format!(
                    "Alias '{text_nfc}' conflicts with an existing trigger."
                )));
            }
        }
        if !holders.iter().any(|pid| pid == id) {
            add_alias(conn, id, invocation_type, &text_nfc, false)?;
        }

        Ok(())
    })
}

/// Inserts or updates a script attachment for an trigger.
pub fn upsert_script(
    conn: &Connection,
    trigger_id: &str,
    interpreter: crate::engine::shell::ScriptInterpreter,
    behavior: crate::engine::shell::ScriptBehavior,
    compressed_content: &[u8],
) -> Result<()> {
    TriggerLimits::validate_script_size(compressed_content.len())?;
    let now = now_unix_secs();
    conn.execute(
        "INSERT INTO scripts (trigger_id, interpreter, behavior, compressed_content, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(trigger_id) DO UPDATE SET
            interpreter        = excluded.interpreter,
            behavior           = excluded.behavior,
            compressed_content = excluded.compressed_content,
            updated_at         = excluded.updated_at,
            version            = version + 1",
        (
            trigger_id,
            serde_json::to_string(&interpreter)?,
            serde_json::to_string(&behavior)?,
            compressed_content,
            now,
        ),
    )?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Multi-invocation entry model (§0.5/§0.9)
// ─────────────────────────────────────────────────────────────────────────────

/// Multi-invocation entry payload for the alias model.
///
/// `name` is stored EXACTLY as passed — including the empty string, which
/// means auto-display per §0.11 (never defaulted to trigger text).
/// Each invocation is `(type, text, require_confirmation)`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEntry {
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub action_type: String,
    pub target_os: String,
    pub only_apps: Option<String>,
    pub except_apps: Option<String>,
    pub tags_json: String,
    pub auto_case: bool,
    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
    pub invocations: Vec<(InvocationType, String, bool)>,
}

/// Runs `f` inside one IMMEDIATE transaction over a shared connection.
/// Rolls back on error. When the caller already holds a transaction (e.g.
/// exchange import), joins it — the outer frame owns commit/rollback.
pub(crate) fn with_transaction<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        return f();
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match f() {
        Ok(value) => {
            conn.execute_batch("COMMIT")?;
            Ok(value)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

const fn invocation_type_for_trigger(trigger_type: TriggerType) -> InvocationType {
    match trigger_type {
        TriggerType::Word => InvocationType::Word,
        TriggerType::Hotkey => InvocationType::Hotkey,
        TriggerType::Regex => InvocationType::Regex,
    }
}

/// Scope-aware overlap check for one requested invocation (§0.14).
/// Voice has no `TriggerType`, so it uses the voice-only equality check
/// with the same conflict message shape.
fn check_invocation_conflict(
    conn: &Connection,
    invocation_type: InvocationType,
    invocation: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    exclude_id: Option<&str>,
) -> Result<()> {
    if invocation_type == InvocationType::Voice {
        if let Some(conflict) = find_voice_overlap_conflict(
            conn,
            invocation,
            target_os,
            only_apps,
            except_apps,
            exclude_id,
        )? {
            return Err(crate::Error::Config(format!(
                "{} '{}' conflicts with existing trigger on target_os '{}' (app filters overlap)",
                conflict.trigger_type.as_db_str(),
                invocation,
                conflict.target_os
            )));
        }
        return Ok(());
    }
    let trigger_type = match invocation_type {
        InvocationType::Word => TriggerType::Word,
        InvocationType::Hotkey => TriggerType::Hotkey,
        InvocationType::Regex => TriggerType::Regex,
        InvocationType::Voice => unreachable!("voice returns early above"),
    };
    validate_trigger_target_os_conflict(
        conn,
        trigger_type,
        invocation,
        target_os,
        only_apps,
        except_apps,
        exclude_id,
    )
}

/// Audit flavor for entry content: regex entries allow bare positional
/// `[use(0)]` refs, so any regex invocation selects the regex flavor.
fn audit_type_for_entry(invocations: &[(InvocationType, String, bool)]) -> TriggerType {
    if invocations
        .iter()
        .any(|(invocation_type, _, _)| *invocation_type == InvocationType::Regex)
    {
        TriggerType::Regex
    } else {
        TriggerType::Word
    }
}

/// Rejects parameterized voice invocations whose `[slot]` names have no
/// matching `[slot=default]` variable in a text output template. Scripts skip
/// the check: bare `[slot]` refs without defaults are idiomatic there and
/// still interpolate at runtime.
fn validate_voice_slot_parity(
    content_nfc: &str,
    invocations: &[(InvocationType, String, bool)],
) -> Result<()> {
    let defined = collect_defined_variables(content_nfc);
    for (invocation_type, invocation, _) in invocations {
        if *invocation_type != InvocationType::Voice {
            continue;
        }
        // Shape errors surface later in `add_alias`; only parity-check here.
        let Ok(pattern) = crate::voice::pattern::VoicePattern::parse(invocation) else {
            continue;
        };
        for slot in pattern.slot_names() {
            if !defined.iter().any(|d| d.eq_ignore_ascii_case(&slot)) {
                return Err(crate::Error::Config(format!(
                    "Voice slot '[{slot}]' has no matching variable in the output template \
                     (voice trigger '{invocation}'). Add '[{slot}=default]' to the output."
                )));
            }
        }
    }
    Ok(())
}

/// Validates entry content/tags/name/description; returns
/// `(content_nfc, normalized_tags)`. Limit validation runs once per word
/// invocation (word-catalog keys); entries without one still get a single
/// pass so dead `[use()]` refs are always caught.
fn validate_entry_payload(
    conn: &Connection,
    content: &str,
    action_type: &str,
    invocations: &[(InvocationType, String, bool)],
    name: &str,
    description: Option<&str>,
    tags_json: &str,
) -> Result<(String, String)> {
    let is_text = is_text_action(action_type)?;
    let content_nfc: String = content.nfc().collect();
    if is_text {
        audit_payload_tags_with_trigger_type(&content_nfc, audit_type_for_entry(invocations))?;
    }
    let mut word_seen = false;
    for (invocation_type, invocation, _) in invocations {
        if *invocation_type == InvocationType::Word {
            let key: String = invocation.nfc().collect();
            validate_trigger_limits(conn, &key, &content_nfc, action_type)?;
            word_seen = true;
        }
    }
    if !word_seen {
        let fallback = invocations
            .first()
            .map(|(_, invocation, _)| invocation.as_str())
            .unwrap_or("");
        validate_trigger_limits(conn, fallback, &content_nfc, action_type)?;
    }
    if name.len() > trigger_types::MAX_NAME_LENGTH {
        return Err(crate::Error::Config(format!(
            "Trigger name exceeds {} character limit",
            trigger_types::MAX_NAME_LENGTH
        )));
    }
    if let Some(desc) = description
        && desc.len() > trigger_types::MAX_DESCRIPTION_LENGTH
    {
        return Err(crate::Error::Config(format!(
            "Trigger description exceeds {} character limit",
            trigger_types::MAX_DESCRIPTION_LENGTH
        )));
    }
    let tags_json = normalize_tags(tags_json)?;
    Ok((content_nfc, tags_json))
}

/// Warns (never errors) when `name` is already used by another live entry.
/// Empty names mean auto-display and are never warned about.
fn warn_on_duplicate_name(conn: &Connection, name: &str, exclude_id: Option<&str>) {
    if name.trim().is_empty() {
        return;
    }
    let duplicate_count: i64 = match exclude_id {
        Some(id) => conn
            .query_row(
                "SELECT COUNT(*) FROM triggers WHERE name = ?1 AND is_deleted = 0 AND id != ?2",
                rusqlite::params![name, id],
                |r| r.get(0),
            )
            .unwrap_or(0),
        None => conn
            .query_row(
                "SELECT COUNT(*) FROM triggers WHERE name = ?1 AND is_deleted = 0",
                rusqlite::params![name],
                |r| r.get(0),
            )
            .unwrap_or(0),
    };
    if duplicate_count > 0 {
        tracing::warn!(
            "Trigger name '{}' is already used by {} other trigger(s).",
            name,
            duplicate_count,
        );
    }
}

/// Resolves interpreter/behavior for a script entry, mirroring the old
/// create/update paths exactly (infer when absent, Inline default).
fn resolve_script_parts(
    content_nfc: &str,
    interpreter: Option<ScriptInterpreter>,
    behavior: Option<ScriptBehavior>,
) -> Result<(ScriptInterpreter, ScriptBehavior, String)> {
    let resolved = interpreter
        .or_else(|| infer_interpreter(None, content_nfc))
        .ok_or_else(|| {
            crate::Error::Config(
                "Unable to determine a script language for this trigger.".to_string(),
            )
        })?;
    let resolved_behavior = behavior.unwrap_or(ScriptBehavior::Inline);
    let script_output = format!("[Script: {}]", script_interpreter_tag(resolved));
    Ok((resolved, resolved_behavior, script_output))
}

/// Cleans one raw app-filter string into its stored form (`None` = no filter).
fn clean_app_filter_value(input: Option<&str>) -> Result<Option<String>> {
    let Some(raw) = input else {
        return Ok(None);
    };
    let mut items = Vec::new();
    for item in split_app_filters(raw) {
        if item.is_empty() {
            continue;
        }
        if let Some(pos) = item.find(':') {
            let prefix = &item[..pos];
            if AppFilterPrefix::parse_prefix(prefix).is_none() {
                return Err(crate::Error::Config(format!(
                    "unknown app filter prefix '{}' (use: {})",
                    prefix,
                    AppFilterPrefix::valid_prefixes_hint()
                )));
            }
        }
        items.push(item);
    }
    if items.is_empty() {
        Ok(None)
    } else {
        Ok(Some(items.join(",")))
    }
}

fn create_entry_inner(
    conn: &Connection,
    entry: &NewEntry,
) -> Result<(String, Vec<TriggerAliasRow>)> {
    if entry.invocations.is_empty() {
        return Err(crate::Error::Config(
            "Entry requires at least one invocation.".to_string(),
        ));
    }
    validate_target_os_value(&entry.target_os)?;
    // NFC once so validation, storage, and later lookups agree byte-for-byte.
    let invocations: Vec<(InvocationType, String, bool)> = entry
        .invocations
        .iter()
        .map(|(t, s, rc)| (*t, s.nfc().collect::<String>(), *rc))
        .collect();
    let is_text = is_text_action(&entry.action_type)?;
    let (content_nfc, tags_json) = validate_entry_payload(
        conn,
        &entry.content,
        &entry.action_type,
        &invocations,
        &entry.name,
        entry.description.as_deref(),
        &entry.tags_json,
    )?;
    if is_text {
        validate_voice_slot_parity(&content_nfc, &invocations)?;
    }
    warn_on_duplicate_name(conn, &entry.name, None);
    // Overlap pre-check (catches hotkey overlaps that share no UNIQUE row).
    for (invocation_type, invocation, _) in &invocations {
        check_invocation_conflict(
            conn,
            *invocation_type,
            invocation,
            &entry.target_os,
            entry.only_apps.as_deref(),
            entry.except_apps.as_deref(),
            None,
        )?;
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = now_unix_secs();
    if !is_text {
        let (interpreter, behavior, script_output) =
            resolve_script_parts(&content_nfc, entry.interpreter, entry.behavior)?;
        conn.execute(
            "INSERT INTO triggers
                (id, name, description, output, action_type, target_os, tags,
                 usage_count, last_used_at, created_at, updated_at, version, is_deleted, auto_case)
             VALUES (?1, ?2, ?3, ?4, 'script', ?5, ?6, 0, NULL, ?7, ?7, 1, 0, ?8)",
            (
                &id,
                &entry.name,
                entry.description.as_deref(),
                &script_output,
                &entry.target_os,
                &tags_json,
                now,
                entry.auto_case,
            ),
        )?;
        upsert_script(conn, &id, interpreter, behavior, &compress(&content_nfc)?)?;
    } else {
        validate_output(
            &content_nfc,
            invocations
                .first()
                .map(|(_, invocation, _)| invocation.as_str()),
        )?;
        conn.execute(
            "INSERT INTO triggers
                (id, name, description, output, action_type, target_os, tags,
                 usage_count, last_used_at, created_at, updated_at, version, is_deleted, auto_case)
             VALUES (?1, ?2, ?3, ?4, 'text', ?5, ?6, 0, NULL, ?7, ?7, 1, 0, ?8)",
            (
                &id,
                &entry.name,
                entry.description.as_deref(),
                &content_nfc,
                &entry.target_os,
                &tags_json,
                now,
                entry.auto_case,
            ),
        )?;
        let processed_output = compile_and_save_assets(conn, &id, &content_nfc)?;
        if processed_output != content_nfc {
            conn.execute(
                "UPDATE triggers SET output = ?1, updated_at = ?2 WHERE id = ?3",
                (&processed_output, now, &id),
            )?;
        }
    }

    update_trigger_app_filters(
        conn,
        &id,
        entry.only_apps.clone(),
        entry.except_apps.clone(),
    )?;

    let mut aliases = Vec::with_capacity(invocations.len());
    for (invocation_type, invocation, require_confirmation) in &invocations {
        aliases.push(add_alias(
            conn,
            &id,
            *invocation_type,
            invocation,
            *require_confirmation,
        )?);
    }
    Ok((id, aliases))
}

/// Inserts a new multi-invocation entry: one parent row (name stored
/// EXACTLY, empty included) plus one alias row per invocation, atomically.
pub fn create_entry(conn: &Connection, entry: NewEntry) -> Result<(String, Vec<TriggerAliasRow>)> {
    with_transaction(conn, || create_entry_inner(conn, &entry))
}

/// Parent columns compared by the upsert update path.
struct ParentSnapshot {
    output: String,
    action_type: String,
    target_os: String,
    only_apps: Option<String>,
    except_apps: Option<String>,
    tags: String,
    name: String,
    description: Option<String>,
    auto_case: bool,
}

fn load_parent_snapshot(conn: &Connection, parent_id: &str) -> Result<ParentSnapshot> {
    Ok(conn.query_row(
        "SELECT output, action_type, target_os, only_apps, except_apps, tags, name, description, auto_case
          FROM triggers WHERE id = ?1",
        [parent_id],
        |row| {
            Ok(ParentSnapshot {
                output: row.get(0)?,
                action_type: row.get(1)?,
                target_os: row.get(2)?,
                only_apps: row.get(3)?,
                except_apps: row.get(4)?,
                tags: row.get(5)?,
                name: row.get(6)?,
                description: row.get(7)?,
                auto_case: row.get(8)?,
            })
        },
    )?)
}

/// True when the stored script blob matches the requested script payload
/// (decompressed content plus serialized interpreter/behavior).
fn script_blob_matches(
    conn: &Connection,
    parent_id: &str,
    content_nfc: &str,
    interpreter: ScriptInterpreter,
    behavior: ScriptBehavior,
) -> Result<bool> {
    let row: Option<(String, String, Vec<u8>)> = conn
        .query_row(
            "SELECT interpreter, behavior, compressed_content FROM scripts WHERE trigger_id = ?1",
            [parent_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((stored_interpreter, stored_behavior, blob)) = row else {
        return Ok(false);
    };
    if stored_interpreter
        != serde_json::to_string(&interpreter).map_err(|e| crate::Error::Config(e.to_string()))?
    {
        return Ok(false);
    }
    if stored_behavior
        != serde_json::to_string(&behavior).map_err(|e| crate::Error::Config(e.to_string()))?
    {
        return Ok(false);
    }
    Ok(decompress(&blob)? == content_nfc)
}

fn update_entry_inner(
    conn: &Connection,
    parent_id: &str,
    entry: &NewEntry,
    requested: &[(InvocationType, String, bool)],
) -> Result<AddOutcome> {
    validate_target_os_value(&entry.target_os)?;
    let is_text = is_text_action(&entry.action_type)?;
    let (content_nfc, tags_json) = validate_entry_payload(
        conn,
        &entry.content,
        &entry.action_type,
        requested,
        &entry.name,
        entry.description.as_deref(),
        &entry.tags_json,
    )?;
    if is_text {
        validate_voice_slot_parity(&content_nfc, requested)?;
    }
    warn_on_duplicate_name(conn, &entry.name, Some(parent_id));

    let current = load_parent_snapshot(conn, parent_id)?;

    // Missing invocations, resolved normalization-aware: anything not already
    // on this parent. (A hit on another parent is impossible here — the
    // caller resolved exactly one distinct parent.)
    let mut missing: Vec<(InvocationType, String, bool)> = Vec::new();
    for (invocation_type, invocation, require_confirmation) in requested {
        match find_parent_by_invocation(conn, *invocation_type, invocation)? {
            Some(pid) if pid == parent_id => {}
            _ => missing.push((*invocation_type, invocation.clone(), *require_confirmation)),
        }
    }
    for (invocation_type, invocation, _) in &missing {
        check_invocation_conflict(
            conn,
            *invocation_type,
            invocation,
            &entry.target_os,
            entry.only_apps.as_deref(),
            entry.except_apps.as_deref(),
            Some(parent_id),
        )?;
    }

    let only_cleaned = clean_app_filter_value(entry.only_apps.as_deref())?;
    let except_cleaned = clean_app_filter_value(entry.except_apps.as_deref())?;
    let scope_same = current.target_os == entry.target_os
        && current.only_apps == only_cleaned
        && current.except_apps == except_cleaned;
    let meta_same = current.tags == tags_json
        && current.name == entry.name
        && current.description == entry.description
        && current.auto_case == entry.auto_case;

    let now = now_unix_secs();
    if !is_text {
        let (interpreter, behavior, script_output) =
            resolve_script_parts(&content_nfc, entry.interpreter, entry.behavior)?;
        let script_same = current.action_type.eq_ignore_ascii_case("script")
            && script_blob_matches(conn, parent_id, &content_nfc, interpreter, behavior)?;
        if current.output == script_output
            && script_same
            && scope_same
            && meta_same
            && missing.is_empty()
        {
            return Ok(AddOutcome::AlreadyExists);
        }
        conn.execute(
            "UPDATE triggers SET name = ?1, description = ?2, output = ?3, action_type = 'script',
                    target_os = ?4, tags = ?5, auto_case = ?6, is_deleted = 0,
                    version = version + 1, updated_at = ?7 WHERE id = ?8",
            (
                &entry.name,
                entry.description.as_deref(),
                &script_output,
                &entry.target_os,
                &tags_json,
                entry.auto_case,
                now,
                parent_id,
            ),
        )?;
        upsert_script(
            conn,
            parent_id,
            interpreter,
            behavior,
            &compress(&content_nfc)?,
        )?;
    } else {
        validate_output(&content_nfc, requested.first().map(|(_, s, _)| s.as_str()))?;
        if current.output == content_nfc
            && current.action_type.eq_ignore_ascii_case("text")
            && scope_same
            && meta_same
            && missing.is_empty()
        {
            return Ok(AddOutcome::AlreadyExists);
        }
        conn.execute(
            "UPDATE triggers SET name = ?1, description = ?2, output = ?3, action_type = 'text',
                    target_os = ?4, tags = ?5, auto_case = ?6, is_deleted = 0,
                    version = version + 1, updated_at = ?7 WHERE id = ?8",
            (
                &entry.name,
                entry.description.as_deref(),
                &content_nfc,
                &entry.target_os,
                &tags_json,
                entry.auto_case,
                now,
                parent_id,
            ),
        )?;
        conn.execute(
            "DELETE FROM scripts WHERE trigger_id = ?1",
            rusqlite::params![parent_id],
        )?;
        let processed_output = compile_and_save_assets(conn, parent_id, &content_nfc)?;
        if processed_output != content_nfc {
            conn.execute(
                "UPDATE triggers SET output = ?1, updated_at = ?2 WHERE id = ?3",
                (&processed_output, now, parent_id),
            )?;
        }
    }

    update_trigger_app_filters(
        conn,
        parent_id,
        entry.only_apps.clone(),
        entry.except_apps.clone(),
    )?;
    for (invocation_type, invocation, require_confirmation) in &missing {
        add_alias(
            conn,
            parent_id,
            *invocation_type,
            invocation,
            *require_confirmation,
        )?;
    }
    Ok(AddOutcome::Updated)
}

/// All live parent ids sharing one invocation (scope-unfiltered).
/// Normalization mirrors `find_parent_by_invocation` (NFC + Hotkey/Voice
/// canonical forms); unparseable Hotkey/Voice lookups match nothing.
pub(crate) fn parents_by_invocation(
    conn: &Connection,
    invocation_type: InvocationType,
    invocation: &str,
) -> Result<Vec<String>> {
    let stored: String = match invocation_type {
        InvocationType::Hotkey => {
            match prepare_trigger_with_type(invocation, TriggerType::Hotkey, "all") {
                Ok(prepared) => prepared.stored_trigger,
                Err(_) => return Ok(Vec::new()),
            }
        }
        InvocationType::Voice => match validate_voice_phrase(invocation) {
            Ok(normalized) => normalized,
            Err(_) => return Ok(Vec::new()),
        },
        InvocationType::Word | InvocationType::Regex => invocation.nfc().collect(),
    };
    let mut stmt = conn.prepare_cached(
        "SELECT al.trigger_id FROM trigger_aliases al
          JOIN triggers t ON t.id = al.trigger_id
          WHERE al.invocation_type = ?1 AND al.invocation = ?2 AND t.is_deleted = 0",
    )?;
    Ok(stmt
        .query_map(
            rusqlite::params![invocation_type.as_db_str(), stored],
            |row| row.get(0),
        )?
        .collect::<std::result::Result<Vec<String>, _>>()?)
}

fn upsert_entry_inner(conn: &Connection, entry: &NewEntry) -> Result<AddOutcome> {
    if entry.invocations.is_empty() {
        return Err(crate::Error::Config(
            "Entry requires at least one invocation.".to_string(),
        ));
    }
    validate_target_os_value(&entry.target_os)?;
    let requested: Vec<(InvocationType, String, bool)> = entry
        .invocations
        .iter()
        .map(|(t, s, rc)| (*t, s.nfc().collect::<String>(), *rc))
        .collect();
    // Resolve each invocation; collect distinct SCOPE-OVERLAPPING parents for
    // the §0.9 rule. A hit whose scope is disjoint from the request is not
    // the same entry — treated as no hit (the create path re-checks scope via
    // `add_alias`, which allows disjoint duplicates).
    let mut parents: Vec<(String, String)> = Vec::new();
    for (invocation_type, invocation, _) in &requested {
        for pid in parents_by_invocation(conn, *invocation_type, invocation)? {
            if parents.iter().any(|(id, _)| id == &pid) {
                continue;
            }
            let (hit_os, hit_only, hit_except): (String, Option<String>, Option<String>) = conn
                .query_row(
                    "SELECT target_os, only_apps, except_apps FROM triggers WHERE id = ?1",
                    [&pid],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
            if target_os_values_overlap(&entry.target_os, &hit_os)
                && app_filters_overlap(
                    entry.only_apps.as_deref(),
                    entry.except_apps.as_deref(),
                    hit_only.as_deref(),
                    hit_except.as_deref(),
                )
            {
                parents.push((pid, invocation.clone()));
            }
        }
    }
    match parents.len() {
        0 => {
            let (_, _) = create_entry_inner(conn, entry)?;
            Ok(AddOutcome::Created)
        }
        1 => update_entry_inner(conn, &parents[0].0.clone(), entry, &requested),
        _ => Err(crate::Error::Config(format!(
            "Invocations '{}' and '{}' belong to two different entries; move or delete one entry before merging.",
            parents[0].1, parents[1].1
        ))),
    }
}

/// Upserts a full multi-invocation entry per §0.9: zero resolution hits →
/// `create_entry` (`Created`); exactly one distinct parent → update columns +
/// add missing invocations (`Updated`, or `AlreadyExists` when outputs, scope,
/// and aliases are byte-identical); two or more distinct parents → `Config`
/// error. One transaction (all-or-nothing).
pub fn upsert_entry_full(conn: &Connection, entry: NewEntry) -> Result<AddOutcome> {
    with_transaction(conn, || upsert_entry_inner(conn, &entry))
}

pub fn update_existing_trigger(
    conn: &mut Connection,
    update: ExistingTriggerUpdate<'_>,
) -> Result<()> {
    with_transaction(conn, || {
        validate_target_os_value(update.target_os)?;
        let is_text = is_text_action(update.action_type)?;
        let trigger_nfc: String = update.trigger.nfc().collect();
        let content_nfc: String = update.content.nfc().collect();
        if is_text {
            audit_payload_tags_with_trigger_type(&content_nfc, update.trigger_type)?;
        }

        // We only enforce limits for text snippets, as nested limits apply to the `use` variable
        validate_trigger_limits(conn, &trigger_nfc, &content_nfc, update.action_type)?;

        if update.name.len() > trigger_types::MAX_NAME_LENGTH {
            return Err(crate::Error::Config(format!(
                "Trigger name exceeds {} character limit",
                trigger_types::MAX_NAME_LENGTH
            )));
        }
        if let Some(desc) = update.description
            && desc.len() > trigger_types::MAX_DESCRIPTION_LENGTH
        {
            return Err(crate::Error::Config(format!(
                "Trigger description exceeds {} character limit",
                trigger_types::MAX_DESCRIPTION_LENGTH
            )));
        }

        let tags_json = normalize_tags(update.tags_json)?;

        warn_on_duplicate_name(conn, update.name, Some(update.id));

        let prepared =
            prepare_trigger_with_type(&trigger_nfc, update.trigger_type, update.target_os)?;

        // Validate conflict before writing, excluding the row being updated.
        validate_trigger_target_os_conflict(
            conn,
            prepared.trigger_type,
            &prepared.stored_trigger,
            update.target_os,
            None,
            None,
            Some(update.id),
        )?;

        let now = now_unix_secs();
        if !is_text {
            let interpreter = update
                .interpreter
                .or_else(|| infer_interpreter(None, &content_nfc))
                .ok_or_else(|| {
                    crate::Error::Config(
                        "Unable to determine a script language for this trigger.".to_string(),
                    )
                })?;
            let behavior = update.behavior.unwrap_or(ScriptBehavior::Inline);
            let script_output = format!("[Script: {}]", script_interpreter_tag(interpreter));

            conn.execute(
                "UPDATE triggers SET name = ?1, description = ?2, output = ?3, action_type = 'script',
                        target_os = ?4, tags = ?5, usage_count = ?6, last_used_at = ?7,
                        auto_case = ?8, is_deleted = 0, version = version + 1, updated_at = ?9
                  WHERE id = ?10",
                (
                    update.name,
                    update.description,
                    &script_output,
                    update.target_os,
                    &tags_json,
                    update.usage_count,
                    update.last_used_at,
                    update.auto_case,
                    now,
                    update.id,
                ),
            )?;
            upsert_script(
                conn,
                update.id,
                interpreter,
                behavior,
                &compress(&content_nfc)?,
            )?;
        } else {
            validate_output(&content_nfc, Some(&prepared.stored_trigger))?;
            conn.execute(
                "UPDATE triggers SET name = ?1, description = ?2, output = ?3, action_type = 'text',
                        target_os = ?4, tags = ?5, usage_count = ?6, last_used_at = ?7,
                        auto_case = ?8, is_deleted = 0, version = version + 1, updated_at = ?9
                  WHERE id = ?10",
                (
                    update.name,
                    update.description,
                    &content_nfc,
                    update.target_os,
                    &tags_json,
                    update.usage_count,
                    update.last_used_at,
                    update.auto_case,
                    now,
                    update.id,
                ),
            )?;
            conn.execute(
                "DELETE FROM scripts WHERE trigger_id = ?1",
                rusqlite::params![update.id],
            )?;
            let processed_output = compile_and_save_assets(conn, update.id, &content_nfc)?;
            if processed_output != content_nfc {
                conn.execute(
                    "UPDATE triggers SET output = ?1, updated_at = ?2 WHERE id = ?3",
                    (&processed_output, now, update.id),
                )?;
            }
        }

        // Single-invocation edit replaces the invocation (old model had exactly
        // one trigger per row); the conflict check above runs before the swap
        // so a rejection leaves existing aliases untouched.
        conn.execute(
            "DELETE FROM trigger_aliases WHERE trigger_id = ?1",
            rusqlite::params![update.id],
        )?;
        add_alias(
            conn,
            update.id,
            invocation_type_for_trigger(prepared.trigger_type),
            &prepared.stored_trigger,
            false,
        )?;
        Ok(())
    })
}

pub fn create_trigger(conn: &mut Connection, new_trigger: NewTrigger<'_>) -> Result<String> {
    let entry = NewEntry {
        // Stored EXACTLY (empty included); display falls back to the invocation.
        name: new_trigger.name.unwrap_or("").to_string(),
        description: new_trigger.description.map(str::to_string),
        content: new_trigger.content.to_string(),
        action_type: new_trigger.action_type.to_string(),
        target_os: new_trigger.target_os.to_string(),
        only_apps: None,
        except_apps: None,
        tags_json: new_trigger.tags_json.to_string(),
        auto_case: new_trigger.auto_case,
        interpreter: new_trigger.interpreter,
        behavior: new_trigger.behavior,
        invocations: vec![(
            invocation_type_for_trigger(new_trigger.trigger_type),
            new_trigger.trigger.to_string(),
            false,
        )],
    };
    let (id, _) = create_entry(conn, entry)?;
    Ok(id)
}

fn is_text_action(action_type: &str) -> Result<bool> {
    if action_type.eq_ignore_ascii_case("text") {
        Ok(true)
    } else if action_type.eq_ignore_ascii_case("script") {
        Ok(false)
    } else {
        Err(crate::Error::Config(format!(
            "Unsupported action_type '{}'. Expected 'text' or 'script'.",
            action_type
        )))
    }
}

fn desktop_platforms_for_target_os(target_os: &str) -> Result<&'static [HotkeyPlatform]> {
    match target_os {
        "all" => Ok(&[
            HotkeyPlatform::Windows,
            HotkeyPlatform::Linux,
            HotkeyPlatform::Mac,
        ]),
        "win" => Ok(&[HotkeyPlatform::Windows]),
        "linux" => Ok(&[HotkeyPlatform::Linux]),
        "mac" => Ok(&[HotkeyPlatform::Mac]),
        "android" | "ios" => Err(crate::Error::Config(format!(
            "Hotkey triggers are only supported for desktop target_os values; got '{}'",
            target_os
        ))),
        other => Err(crate::Error::Config(format!(
            "Unsupported target_os '{}' for hotkey validation",
            other
        ))),
    }
}

trait PlatformLabel {
    fn as_label(&self) -> &'static str;
}

impl PlatformLabel for HotkeyPlatform {
    fn as_label(&self) -> &'static str {
        match self {
            HotkeyPlatform::Windows => "Windows",
            HotkeyPlatform::Linux => "Linux",
            HotkeyPlatform::Mac => "macOS",
        }
    }
}

fn script_interpreter_tag(interpreter: ScriptInterpreter) -> &'static str {
    match interpreter {
        ScriptInterpreter::Bash => "bash",
        ScriptInterpreter::PowerShell => "powershell",
        ScriptInterpreter::Python => "python",
        ScriptInterpreter::Node => "node",
        ScriptInterpreter::Cmd => "cmd",
    }
}

/// Increments the usage_count and updates last_used_at for the given trigger.
///
/// Opens the production DB and increments `usage_count` for every active row
/// whose trigger matches, while also updating the daily stats.
///
/// Intended for callers (e.g. the daemon hook thread) that do not hold an open
/// `Connection` and do not want a direct dependency on `rusqlite`.
///
/// Result of an `add_trigger` call.
#[derive(Debug, Clone, PartialEq)]
pub enum AddOutcome {
    /// A brand-new trigger was created.
    Created,
    /// An trigger with the same trigger and identical output already exists.
    AlreadyExists,
    /// An trigger with the same trigger existed but had a different output;
    /// the output (and `updated_at` / `version`) have been updated.
    Updated,
}

pub fn split_app_filters(input: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&',') => {
                current.push(',');
                chars.next();
            }
            ',' => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    items.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        items.push(trimmed);
    }
    items
}

pub fn update_trigger_app_filters(
    conn: &Connection,
    id: &str,
    only_apps: Option<String>,
    except_apps: Option<String>,
) -> Result<()> {
    let only_cleaned = clean_app_filter_value(only_apps.as_deref())?;
    let except_cleaned = clean_app_filter_value(except_apps.as_deref())?;

    conn.execute(
        "UPDATE triggers
         SET only_apps = ?1, except_apps = ?2
         WHERE id = ?3 AND is_deleted = 0",
        rusqlite::params![only_cleaned, except_cleaned, id],
    )?;
    Ok(())
}

/// Lenient tag normalization for the `add_trigger` path (drops over-long
/// tags, truncates to the cap — the user isn't directly managing tags).
fn normalize_add_tags(tags: Option<Vec<String>>) -> Result<String> {
    let mut normalized: Vec<String> = Vec::new();
    if let Some(list) = tags {
        let mut seen = std::collections::HashSet::new();
        for s in list {
            let trimmed = s.trim().to_lowercase();
            if trimmed.is_empty() || !seen.insert(trimmed.clone()) {
                continue;
            }
            if trimmed.len() > MAX_TAG_LENGTH {
                // Silently reject over-length tags in this path (add-trigger will
                // still create the trigger; the user isn't directly managing tags).
                continue;
            }
            normalized.push(trimmed);
        }
        if normalized.len() > MAX_TAGS_COUNT {
            normalized.truncate(MAX_TAGS_COUNT);
        }
    }
    serde_json::to_string(&normalized).map_err(|e| crate::Error::Config(e.to_string()))
}

/// Creates or updates an trigger using only its trigger and output.
///
/// - If no active trigger exists for the trigger, a new row is inserted
///   and `AddOutcome::Created` is returned.
/// - If an active trigger exists with the **same** output,
///   `AddOutcome::AlreadyExists` is returned and no writes happen.
/// - If an active trigger exists with a **different** output, the output
///   is updated (along with `updated_at` and `version`) and
///   `AddOutcome::Updated` is returned.
#[allow(clippy::too_many_arguments)]
pub fn add_trigger(
    conn: &Connection,
    trigger: &str,
    output: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    tags: Option<Vec<String>>,
) -> Result<AddOutcome> {
    add_trigger_by_type_with_case(
        conn,
        TriggerType::Word,
        trigger,
        output,
        target_os,
        only_apps,
        except_apps,
        tags,
        None,
        None,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_trigger_by_type(
    conn: &Connection,
    trigger_type: TriggerType,
    trigger: &str,
    output: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    tags: Option<Vec<String>>,
) -> Result<AddOutcome> {
    add_trigger_by_type_with_case(
        conn,
        trigger_type,
        trigger,
        output,
        target_os,
        only_apps,
        except_apps,
        tags,
        None,
        None,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_trigger_with_case(
    conn: &Connection,
    trigger: &str,
    output: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    tags: Option<Vec<String>>,
    name: Option<&str>,
    description: Option<&str>,
    auto_case: bool,
) -> Result<AddOutcome> {
    add_trigger_by_type_with_case(
        conn,
        TriggerType::Word,
        trigger,
        output,
        target_os,
        only_apps,
        except_apps,
        tags,
        name,
        description,
        auto_case,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_trigger_by_type_with_case(
    conn: &Connection,
    trigger_type: TriggerType,
    trigger: &str,
    output: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    tags: Option<Vec<String>>,
    name: Option<&str>,
    description: Option<&str>,
    auto_case: bool,
) -> Result<AddOutcome> {
    if name.is_some_and(|n| n.len() > trigger_types::MAX_NAME_LENGTH) {
        return Err(crate::Error::Config(format!(
            "Name exceeds {} character limit",
            trigger_types::MAX_NAME_LENGTH
        )));
    }
    if description.is_some_and(|d| d.len() > trigger_types::MAX_DESCRIPTION_LENGTH) {
        return Err(crate::Error::Config(format!(
            "Description exceeds {} character limit",
            trigger_types::MAX_DESCRIPTION_LENGTH
        )));
    }

    let tags_json = normalize_add_tags(tags)?;
    upsert_entry_full(
        conn,
        NewEntry {
            // Stored EXACTLY (empty included); display falls back to the invocation.
            name: name.unwrap_or("").to_string(),
            description: description.map(str::to_string),
            content: output.to_string(),
            action_type: "text".to_string(),
            target_os: target_os.to_string(),
            only_apps: only_apps.map(str::to_string),
            except_apps: except_apps.map(str::to_string),
            tags_json,
            auto_case,
            interpreter: None,
            behavior: None,
            invocations: vec![(
                invocation_type_for_trigger(trigger_type),
                trigger.to_string(),
                false,
            )],
        },
    )
}
