use super::app_filter::AppFilterPrefix;
use super::assets::*;
use super::overlap::*;
use super::trigger_types::TriggerLimits;
use super::validate::*;
use unicode_normalization::UnicodeNormalization;

use crate::Result;
use rusqlite::Connection;

use crate::db::now_unix_secs;
use crate::engine::{
    shell::{ScriptBehavior, ScriptInterpreter, compress, infer_interpreter},
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

    let hotkey = parse_hotkey(trigger).map_err(|error| {
        crate::Error::Config(format!("Invalid hotkey '{}': {}", trigger, error))
    })?;
    let canonical = hotkey.canonical_string();

    if conflicts_with_taurine_global_hotkey(hotkey).is_some() {
        return Err(crate::Error::Config(format!(
            "Hotkey '{}' conflicts with Taurine's global pause hotkey alt+`",
            canonical
        )));
    }

    for platform in desktop_platforms_for_target_os(target_os)? {
        if let Some(danger) = danger_for_platform(hotkey, *platform) {
            return Err(crate::Error::Config(format!(
                "Hotkey '{}' is not allowed for target_os '{}': conflicts with the {} on {}",
                canonical,
                target_os,
                danger.description(),
                platform.as_label(),
            )));
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
    let now = now_unix_secs();

    // Keep created_at stable across updates.
    conn.execute(
        "INSERT INTO triggers
            (id, name, description, trigger_type, trigger, output, action_type, target_os, tags,
             usage_count, last_used_at, created_at, updated_at, version, is_deleted, auto_case)
         VALUES
            (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
             ?10, ?11, ?12, ?13, 1, 0, ?14)
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
            auto_case    = excluded.auto_case,
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

    Ok(())
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

pub fn update_existing_trigger(
    conn: &mut Connection,
    update: ExistingTriggerUpdate<'_>,
) -> Result<()> {
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

    let duplicate_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE name = ?1 AND is_deleted = 0 AND id != ?2",
            rusqlite::params![update.name, update.id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if duplicate_count > 0 {
        tracing::warn!(
            "Trigger name '{}' is already used by {} other trigger(s).",
            update.name,
            duplicate_count,
        );
    }

    let prepared = prepare_trigger_with_type(&trigger_nfc, update.trigger_type, update.target_os)?;

    // Validate conflict before opening the transaction, excluding the row being updated.
    validate_trigger_target_os_conflict(
        conn,
        prepared.trigger_type,
        &prepared.stored_trigger,
        update.target_os,
        None,
        None,
        Some(update.id),
    )?;

    let tx = conn.transaction()?;

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

        upsert_trigger_with_type_and_case(
            &tx,
            update.id,
            update.name,
            update.description,
            prepared.trigger_type,
            &prepared.stored_trigger,
            &script_output,
            "script",
            update.target_os,
            &tags_json,
            update.usage_count,
            update.last_used_at,
            update.auto_case,
        )?;
        upsert_script(
            &tx,
            update.id,
            interpreter,
            behavior,
            &compress(&content_nfc)?,
        )?;
    } else {
        validate_output(&content_nfc, Some(&prepared.stored_trigger))?;
        upsert_trigger_with_type_and_case(
            &tx,
            update.id,
            update.name,
            update.description,
            prepared.trigger_type,
            &prepared.stored_trigger,
            &content_nfc,
            "text",
            update.target_os,
            &tags_json,
            update.usage_count,
            update.last_used_at,
            update.auto_case,
        )?;
        tx.execute(
            "DELETE FROM scripts WHERE trigger_id = ?1",
            rusqlite::params![update.id],
        )?;
    }

    tx.commit()?;
    Ok(())
}

pub fn create_trigger(conn: &mut Connection, new_trigger: NewTrigger<'_>) -> Result<String> {
    validate_target_os_value(new_trigger.target_os)?;
    let is_text = is_text_action(new_trigger.action_type)?;
    let trigger_nfc: String = new_trigger.trigger.nfc().collect();
    let content_nfc: String = new_trigger.content.nfc().collect();
    if is_text {
        audit_payload_tags_with_trigger_type(&content_nfc, new_trigger.trigger_type)?;
    }

    validate_trigger_limits(conn, &trigger_nfc, &content_nfc, new_trigger.action_type)?;

    let prepared = prepare_trigger_with_type(
        &trigger_nfc,
        new_trigger.trigger_type,
        new_trigger.target_os,
    )?;
    let id = uuid::Uuid::new_v4().to_string();
    let generated_name = prepared.stored_trigger.clone();
    let name = new_trigger
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(generated_name.as_str());
    TriggerLimits::validate_name(name)?;
    TriggerLimits::validate_description(new_trigger.description)?;
    let tags_json = normalize_tags(new_trigger.tags_json)?;

    let duplicate_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE name = ?1 AND is_deleted = 0",
            [name],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if duplicate_count > 0 {
        tracing::warn!(
            "Trigger name '{}' is already used by {} other trigger(s).",
            name,
            duplicate_count,
        );
    }

    // Validate conflict before opening the transaction so no partial writes happen.
    validate_trigger_target_os_conflict(
        conn,
        prepared.trigger_type,
        &prepared.stored_trigger,
        new_trigger.target_os,
        None,
        None,
        None,
    )?;

    let tx = conn.transaction()?;

    if !is_text {
        let interpreter = new_trigger
            .interpreter
            .or_else(|| infer_interpreter(None, &content_nfc))
            .ok_or_else(|| {
                crate::Error::Config(
                    "Unable to determine a script language for this trigger.".to_string(),
                )
            })?;
        let behavior = new_trigger.behavior.unwrap_or(ScriptBehavior::Inline);
        let script_output = format!("[Script: {}]", script_interpreter_tag(interpreter));

        upsert_trigger_with_type_and_case(
            &tx,
            &id,
            name,
            new_trigger.description,
            prepared.trigger_type,
            &prepared.stored_trigger,
            &script_output,
            "script",
            new_trigger.target_os,
            &tags_json,
            0,
            None,
            new_trigger.auto_case,
        )?;
        upsert_script(&tx, &id, interpreter, behavior, &compress(&content_nfc)?)?;
    } else {
        validate_output(&content_nfc, Some(&prepared.stored_trigger))?;
        upsert_trigger_with_type_and_case(
            &tx,
            &id,
            name,
            new_trigger.description,
            prepared.trigger_type,
            &prepared.stored_trigger,
            &content_nfc,
            "text",
            new_trigger.target_os,
            &tags_json,
            0,
            None,
            new_trigger.auto_case,
        )?;
    }

    tx.commit()?;
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
            HotkeyPlatform::Windows => "windows",
            HotkeyPlatform::Linux => "linux",
            HotkeyPlatform::Mac => "mac",
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
    let clean = |s: String| -> Result<Option<String>> {
        let mut items = Vec::new();
        for entry in split_app_filters(&s) {
            if entry.is_empty() {
                continue;
            }
            if let Some(pos) = entry.find(':') {
                let prefix = &entry[..pos];
                if AppFilterPrefix::parse_prefix(prefix).is_none() {
                    return Err(crate::Error::Config(format!(
                        "unknown app filter prefix '{}' (use: {})",
                        prefix,
                        AppFilterPrefix::valid_prefixes_hint()
                    )));
                }
            }
            items.push(entry);
        }
        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(items.join(",")))
        }
    };

    let only_cleaned = only_apps.map(clean).transpose()?;
    let except_cleaned = except_apps.map(clean).transpose()?;

    conn.execute(
        "UPDATE triggers
         SET only_apps = ?1, except_apps = ?2
         WHERE id = ?3 AND is_deleted = 0",
        rusqlite::params![only_cleaned, except_cleaned, id],
    )?;
    Ok(())
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
    let trigger_nfc: String = trigger.nfc().collect();
    let output_nfc: String = output.nfc().collect();

    validate_trigger_limits(conn, &trigger_nfc, &output_nfc, "text")?;

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

    if trigger_type == TriggerType::Regex {
        regex::Regex::new(&trigger_nfc)
            .map_err(|e| crate::Error::Config(format!("Invalid regular expression: {e}")))?;
    }

    let tags = tags.map(|t| {
        let mut seen = std::collections::HashSet::new();
        let mut normalized: Vec<String> = Vec::new();
        for s in t {
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
        normalized
    });

    let existing: Option<(String, String, String, bool)> = conn
        .query_row(
            "SELECT id, output, action_type, auto_case
             FROM triggers
             WHERE trigger_type = ?1
               AND trigger = ?2
               AND target_os = ?3
               AND COALESCE(only_apps, '') = ?4
               AND COALESCE(except_apps, '') = ?5
               AND is_deleted = 0
             ORDER BY updated_at DESC
             LIMIT 1",
            [
                trigger_type.as_db_str(),
                &trigger_nfc,
                target_os,
                only_apps.unwrap_or(""),
                except_apps.unwrap_or(""),
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();

    match existing {
        Some((id, existing_output, existing_action, existing_auto_case))
            if existing_output == output_nfc
                && existing_action == "text"
                && existing_auto_case == auto_case =>
        {
            if name.is_none() && description.is_none() && tags.is_none() {
                return Ok(AddOutcome::AlreadyExists);
            }
            let now = now_unix_secs();
            if let Some(ref t) = tags {
                let t_json =
                    serde_json::to_string(t).map_err(|e| crate::Error::Config(e.to_string()))?;
                conn.execute(
                    "UPDATE triggers
                     SET name        = COALESCE(?1, name),
                         description = COALESCE(?2, description),
                         tags        = ?3,
                         updated_at  = ?4,
                         version     = version + 1
                     WHERE id = ?5",
                    rusqlite::params![name, description, t_json, now, id],
                )?;
            } else {
                conn.execute(
                    "UPDATE triggers
                     SET name        = COALESCE(?1, name),
                         description = COALESCE(?2, description),
                         updated_at  = ?3,
                         version     = version + 1
                     WHERE id = ?4",
                    rusqlite::params![name, description, now, id],
                )?;
            }
            Ok(AddOutcome::Updated)
        }
        Some((id, _, _, _)) => {
            let now = now_unix_secs();
            if let Some(ref t) = tags {
                let t_json =
                    serde_json::to_string(t).map_err(|e| crate::Error::Config(e.to_string()))?;
                conn.execute(
                    "UPDATE triggers
                     SET name        = COALESCE(?1, name),
                         description = COALESCE(?2, description),
                         output      = ?3,
                         action_type = 'text',
                         tags        = ?4,
                         updated_at  = ?5,
                         auto_case   = ?6,
                         version     = version + 1
                     WHERE id = ?7",
                    rusqlite::params![name, description, &output_nfc, t_json, now, auto_case, id],
                )?;
            } else {
                conn.execute(
                    "UPDATE triggers
                     SET name        = COALESCE(?1, name),
                         description = COALESCE(?2, description),
                         output      = ?3,
                         action_type = 'text',
                         updated_at  = ?4,
                         auto_case   = ?5,
                         version     = version + 1
                     WHERE id = ?6",
                    rusqlite::params![name, description, &output_nfc, now, auto_case, id],
                )?;
            }
            conn.execute(
                "DELETE FROM scripts WHERE trigger_id = ?1",
                rusqlite::params![id],
            )?;
            Ok(AddOutcome::Updated)
        }
        None => {
            validate_trigger_target_os_conflict(
                conn,
                trigger_type,
                &trigger_nfc,
                target_os,
                only_apps,
                except_apps,
                None,
            )?;

            let id = uuid::Uuid::new_v4().to_string();
            let trigger_name = name.unwrap_or(&trigger_nfc);
            let tags_str = if let Some(ref t) = tags {
                serde_json::to_string(t).map_err(|e| crate::Error::Config(e.to_string()))?
            } else {
                "[]".to_string()
            };
            upsert_trigger_with_type_and_case(
                conn,
                &id,
                trigger_name,
                description,
                trigger_type,
                &trigger_nfc,
                &output_nfc,
                "text",
                target_os,
                &tags_str,
                0,
                None,
                auto_case,
            )?;

            update_trigger_app_filters(
                conn,
                &id,
                only_apps.map(|s| s.to_string()),
                except_apps.map(|s| s.to_string()),
            )?;

            Ok(AddOutcome::Created)
        }
    }
}
