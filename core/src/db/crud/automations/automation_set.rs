use crate::Result;
use rusqlite::Connection;

use crate::db::{crud::get_setting_value, now_unix_secs};
use crate::engine::{
    shell::{ScriptBehavior, ScriptInterpreter, compress, infer_interpreter},
    variables::system::validate_output,
    variables::{ValidationError, split_system_tag, valid_modifier_hint, validate_system_tag},
};
use crate::keys::{
    HotkeyPlatform, conflicts_with_taurine_global_hotkey, danger_for_platform,
    hotkey_strings_overlap, parse_hotkey,
};

use super::{TriggerConflict, TriggerType};

const INLINE_AI_RESERVED_TRIGGER: &str = "ai";
const TAG_OPEN: u8 = b'[';
const TAG_CLOSE: u8 = b']';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TagBounds {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTrigger {
    pub trigger_type: TriggerType,
    pub stored_trigger: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingAutomationUpdate<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub trigger_type: TriggerType,
    pub trigger: &'a str,
    pub content: &'a str,
    pub action_type: &'a str,
    pub target_os: &'a str,
    pub tags_json: &'a str,
    pub usage_count: i64,
    pub last_used_at: Option<i64>,
    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAutomation<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub trigger_type: TriggerType,
    pub trigger: &'a str,
    pub content: &'a str,
    pub action_type: &'a str,
    pub target_os: &'a str,
    pub tags_json: &'a str,
    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
}

pub fn audit_payload_tags(payload: &str) -> Result<()> {
    let mut ptr = 0;

    while let Some(tag) = find_next_tag(payload, ptr) {
        let inner = trim_slice(&payload[tag.start + 1..tag.end]);
        let (key, default_value) = split_key_default(inner);

        if let Some((root, modifier)) = split_system_tag(key) {
            if let Some(_default) = default_value {
                return Err(crate::Error::Config(format!(
                    "Invalid system tag [{}]: system tags cannot use default assignments. {}",
                    inner,
                    valid_modifier_hint(root)
                )));
            }

            if let Err(error) = validate_system_tag(root, modifier) {
                return Err(crate::Error::Config(format_validation_error(
                    inner, root, modifier, &error,
                )));
            }
        }

        ptr = tag.end + 1;
    }

    Ok(())
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

    if matches!(trigger_type, TriggerType::Word) {
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
    if matches!(trigger_type, TriggerType::Hotkey) {
        crate::keys::parse_hotkey(trigger).map_err(|error| {
            crate::Error::Config(format!(
                "Invalid hotkey '{}' during overlap validation: {}",
                trigger, error
            ))
        })?;
    }

    let mut stmt = conn.prepare_cached(
        "SELECT id, trigger_type, trigger, target_os
         FROM automations
         WHERE trigger_type = ?1
           AND is_deleted = 0
         ORDER BY updated_at DESC",
    )?;

    let rows = stmt.query_map([trigger_type.as_db_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    for row in rows {
        let (id, trigger_type_raw, existing_trigger, existing_target_os) = row?;
        if exclude_id.is_some_and(|excluded| excluded == id) {
            continue;
        }

        let overlaps = if matches!(trigger_type, TriggerType::Hotkey) {
            hotkey_strings_overlap(trigger, &existing_trigger).map_err(|error| {
                crate::Error::Config(format!(
                    "Invalid stored hotkey '{}' during overlap validation: {}",
                    existing_trigger, error
                ))
            })?
        } else {
            trigger == existing_trigger
        };

        if overlaps && target_os_values_overlap(&existing_target_os, target_os) {
            return Ok(Some(TriggerConflict {
                id,
                trigger_type: TriggerType::parse_db(&trigger_type_raw)?,
                trigger: existing_trigger,
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

pub fn update_existing_automation(
    conn: &mut Connection,
    update: ExistingAutomationUpdate<'_>,
) -> Result<()> {
    validate_target_os_value(update.target_os)?;
    audit_payload_tags(update.content)?;

    let prepared =
        prepare_trigger_with_type(update.trigger, update.trigger_type, update.target_os)?;
    let tx = conn.transaction()?;

    if update.action_type.eq_ignore_ascii_case("script") {
        let interpreter = update
            .interpreter
            .or_else(|| infer_interpreter(None, update.content))
            .ok_or_else(|| {
                crate::Error::Config(
                    "Unable to determine a script language for this automation.".to_string(),
                )
            })?;
        let behavior = update.behavior.unwrap_or(ScriptBehavior::Inline);
        let script_output = format!("[Script: {}]", script_interpreter_tag(interpreter));

        upsert_automation_with_trigger_type(
            &tx,
            update.id,
            update.name,
            update.description,
            prepared.trigger_type,
            &prepared.stored_trigger,
            &script_output,
            "script",
            update.target_os,
            update.tags_json,
            update.usage_count,
            update.last_used_at,
        )?;
        upsert_script(
            &tx,
            update.id,
            interpreter,
            behavior,
            &compress(update.content)?,
        )?;
    } else {
        validate_output(update.content, Some(&prepared.stored_trigger));
        upsert_automation_with_trigger_type(
            &tx,
            update.id,
            update.name,
            update.description,
            prepared.trigger_type,
            &prepared.stored_trigger,
            update.content,
            "text",
            update.target_os,
            update.tags_json,
            update.usage_count,
            update.last_used_at,
        )?;
        tx.execute(
            "DELETE FROM scripts WHERE automation_id = ?1",
            rusqlite::params![update.id],
        )?;
    }

    tx.commit()?;
    Ok(())
}

pub fn create_automation(
    conn: &mut Connection,
    new_automation: NewAutomation<'_>,
) -> Result<String> {
    validate_target_os_value(new_automation.target_os)?;
    audit_payload_tags(new_automation.content)?;

    let prepared = prepare_trigger_with_type(
        new_automation.trigger,
        new_automation.trigger_type,
        new_automation.target_os,
    )?;
    let id = uuid::Uuid::new_v4().to_string();
    let generated_name = prepared.stored_trigger.clone();
    let name = new_automation
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(generated_name.as_str());
    let tx = conn.transaction()?;

    if new_automation.action_type.eq_ignore_ascii_case("script") {
        let interpreter = new_automation
            .interpreter
            .or_else(|| infer_interpreter(None, new_automation.content))
            .ok_or_else(|| {
                crate::Error::Config(
                    "Unable to determine a script language for this automation.".to_string(),
                )
            })?;
        let behavior = new_automation.behavior.unwrap_or(ScriptBehavior::Inline);
        let script_output = format!("[Script: {}]", script_interpreter_tag(interpreter));

        upsert_automation_with_trigger_type(
            &tx,
            &id,
            name,
            new_automation.description,
            prepared.trigger_type,
            &prepared.stored_trigger,
            &script_output,
            "script",
            new_automation.target_os,
            new_automation.tags_json,
            0,
            None,
        )?;
        upsert_script(
            &tx,
            &id,
            interpreter,
            behavior,
            &compress(new_automation.content)?,
        )?;
    } else {
        validate_output(new_automation.content, Some(&prepared.stored_trigger));
        upsert_automation_with_trigger_type(
            &tx,
            &id,
            name,
            new_automation.description,
            prepared.trigger_type,
            &prepared.stored_trigger,
            new_automation.content,
            "text",
            new_automation.target_os,
            new_automation.tags_json,
            0,
            None,
        )?;
    }

    tx.commit()?;
    Ok(id)
}

fn validate_target_os_value(target_os: &str) -> Result<()> {
    if matches!(
        target_os,
        "all" | "win" | "linux" | "mac" | "android" | "ios"
    ) {
        Ok(())
    } else {
        Err(crate::Error::Config(format!(
            "Unsupported target_os '{}'",
            target_os
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

fn format_validation_error(
    raw_tag: &str,
    root: &str,
    modifier: Option<&str>,
    error: &ValidationError,
) -> String {
    match error {
        ValidationError::MissingModifier { .. } => format!(
            "Invalid system tag [{}]: `{}` requires a modifier. {}",
            raw_tag,
            root,
            valid_modifier_hint(root)
        ),
        ValidationError::UnexpectedModifier { .. } => format!(
            "Invalid system tag [{}]: `{}` does not accept modifier `{}`. {}",
            raw_tag,
            root,
            modifier.unwrap_or_default(),
            valid_modifier_hint(root)
        ),
        ValidationError::InvalidModifier { modifier, .. } => format!(
            "Invalid system tag [{}]: modifier `{}` is not valid for `{}`. {}",
            raw_tag,
            modifier,
            root,
            valid_modifier_hint(root)
        ),
        ValidationError::UnknownRoot(root) => {
            format!("Invalid system tag [{}]: unknown root `{}`.", raw_tag, root)
        }
    }
}

fn is_escaped(bytes: &[u8], idx: usize) -> bool {
    let mut backslashes = 0;
    let mut cursor = idx;

    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        backslashes += 1;
        cursor -= 1;
    }

    backslashes % 2 == 1
}

fn trim_slice(s: &str) -> &str {
    let trimmed = s.trim();
    let start = s.len() - s.trim_start().len();
    &s[start..start + trimmed.len()]
}

fn find_next_tag(text: &str, from: usize) -> Option<TagBounds> {
    let bytes = text.as_bytes();
    let mut ptr = from;
    let mut start = None;
    let mut depth = 0usize;

    while ptr < bytes.len() {
        match bytes[ptr] {
            TAG_OPEN if !is_escaped(bytes, ptr) => {
                if depth == 0 {
                    start = Some(ptr);
                }
                depth += 1;
            }
            TAG_CLOSE if !is_escaped(bytes, ptr) && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return start.map(|tag_start| TagBounds {
                        start: tag_start,
                        end: ptr,
                    });
                }
            }
            _ => {}
        }
        ptr += 1;
    }

    None
}

fn split_key_default(inner: &str) -> (&str, Option<&str>) {
    let inner = trim_slice(inner);
    let bytes = inner.as_bytes();
    let mut depth = 0usize;
    let mut ptr = 0;

    while ptr < bytes.len() {
        if bytes[ptr] == TAG_OPEN && !is_escaped(bytes, ptr) {
            depth += 1;
        } else if bytes[ptr] == TAG_CLOSE && !is_escaped(bytes, ptr) {
            depth -= 1;
        } else if bytes[ptr] == b'=' && depth == 0 {
            return (
                trim_slice(&inner[..ptr]),
                Some(trim_slice(&inner[ptr + 1..])),
            );
        }
        ptr += 1;
    }

    (inner, None)
}

fn script_interpreter_tag(interpreter: ScriptInterpreter) -> &'static str {
    match interpreter {
        ScriptInterpreter::Bash => "bash",
        ScriptInterpreter::PowerShell => "powershell",
        ScriptInterpreter::Python => "python",
        ScriptInterpreter::Node => "node",
        ScriptInterpreter::NodeEsm => "node-esm",
        ScriptInterpreter::Cmd => "cmd",
    }
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
    _delete_count: usize,
    _left_arrow_count: usize,
) {
    crate::db::crud::record_automation_metric(crate::db::crud::AutomationMetricEvent {
        automation_trigger: Some(trigger.to_string()),
        trigger_chars: trigger.chars().count(),
        success: output_len > 0,
        output_chars: output_len,
        kind: crate::db::crud::AutomationMetricKind::Snippet,
        wpm: None,
    });
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
    let existing: Option<(String, String, String)> = conn
        .query_row(
            "SELECT id, output, action_type
             FROM automations
             WHERE trigger_type = ?1
               AND trigger = ?2
               AND target_os = ?3
               AND is_deleted = 0
             ORDER BY updated_at DESC
             LIMIT 1",
            [trigger_type.as_db_str(), trigger, target_os],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();

    match existing {
        Some((_, existing_output, existing_action))
            if existing_output == output && existing_action == "text" =>
        {
            // Trigger, OS, output, and action_type are identical — nothing to do.
            Ok(AddOutcome::AlreadyExists)
        }
        Some((id, _, _)) => {
            // Same trigger/OS, different output or action_type — update it.
            let now = now_unix_secs();
            conn.execute(
                "UPDATE automations
                 SET output      = ?1,
                     action_type = 'text',
                     updated_at  = ?2,
                     version     = version + 1
                 WHERE id = ?3",
                rusqlite::params![output, now, id],
            )?;
            // Clean up any script attachments if it was previously a script.
            conn.execute(
                "DELETE FROM scripts WHERE automation_id = ?1",
                rusqlite::params![id],
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
