use crate::Result;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutomationActionKind {
    Text,
    Script,
}

fn collect_defined_variables(payload: &str) -> std::collections::HashSet<String> {
    let mut defined = std::collections::HashSet::new();
    let mut ptr = 0;
    while let Some(tag) = find_next_tag(payload, ptr) {
        let inner = trim_slice(&payload[tag.start + 1..tag.end]);
        if inner.contains('[') {
            defined.extend(collect_defined_variables(inner));
        } else {
            let pipeline = crate::engine::variables::system::transformers::split_pipeline(inner);
            let base_expr = pipeline[0];
            let (key, default_value) = split_key_default(base_expr);
            if default_value.is_some() && split_system_tag(key).is_none() {
                let key_unquoted =
                    crate::engine::variables::system::strip_quotes(key).unwrap_or(key);
                defined.insert(key_unquoted.to_string());
            }
        }
        ptr = tag.end + 1;
    }
    defined
}

pub fn audit_payload_tags(payload: &str) -> Result<()> {
    audit_payload_tags_with_trigger_type(payload, TriggerType::Word)
}

pub fn audit_payload_tags_with_trigger_type(
    payload: &str,
    trigger_type: TriggerType,
) -> Result<()> {
    let defined_vars = collect_defined_variables(payload);
    audit_payload_tags_impl(payload, &defined_vars, trigger_type)
}

fn audit_payload_tags_impl(
    payload: &str,
    defined_vars: &std::collections::HashSet<String>,
    trigger_type: TriggerType,
) -> Result<()> {
    let mut ptr = 0;
    let mut cursor_count = 0;
    let mut has_key_or_delay = false;

    while let Some(tag) = find_next_tag(payload, ptr) {
        let inner = trim_slice(&payload[tag.start + 1..tag.end]);
        let pipeline = crate::engine::variables::system::transformers::split_pipeline(inner);
        let base_expr = pipeline[0];
        let (key, default_value) = split_key_default(base_expr);

        let is_nested = key.contains('[') || key.contains(']') || key.starts_with('\x03');

        if is_nested && inner.contains('[') {
            audit_payload_tags_impl(inner, defined_vars, trigger_type)?;
        } else if !is_nested {
            if let Some((root, modifier)) = split_system_tag(key) {
                if root == "cursor" {
                    cursor_count += 1;
                    if cursor_count > 1 {
                        return Err(crate::Error::Config(
                            "Invalid variable [cursor]: multiple cursor directives are not allowed. Only one final caret position can be defined.".to_string()
                        ));
                    }
                }

                if matches!(root, "key" | "delay" | "mouse") {
                    has_key_or_delay = true;
                }

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
            } else {
                let key_unquoted =
                    crate::engine::variables::system::strip_quotes(key).unwrap_or(key);
                if key_unquoted.contains('.') {
                    return Err(crate::Error::Config(format!(
                        "Invalid variable [{}]: user-defined variables cannot contain dots. Dot-namespaces are reserved for system variables.",
                        inner
                    )));
                }
                match default_value {
                    None => {
                        let is_positional = !key_unquoted.is_empty()
                            && key_unquoted.chars().all(|c| c.is_ascii_digit());
                        let is_allowed_regex_positional =
                            matches!(trigger_type, TriggerType::Regex) && is_positional;
                        if !defined_vars.contains(key_unquoted) && !is_allowed_regex_positional {
                            return Err(crate::Error::Config(format!(
                                "Invalid variable [{}]: dynamic variables must have a default value assignment (e.g., [key=default]). If you intended to write literal text, escape the brackets like \\[{}\\].",
                                inner, inner
                            )));
                        }
                    }
                    Some(val) => {
                        let unquoted =
                            crate::engine::variables::system::strip_quotes(val).unwrap_or(val);
                        if unquoted.trim().is_empty() {
                            return Err(crate::Error::Config(format!(
                                "Invalid variable [{}]: default assignments cannot be empty.",
                                inner
                            )));
                        }
                    }
                }
            }
        }

        ptr = tag.end + 1;
    }

    if cursor_count > 0 && has_key_or_delay {
        return Err(crate::Error::Config(
            "The [cursor] directive cannot be used alongside [key.*], [delay.*], or [mouse.*] directives.".to_string()
        ));
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

pub fn app_filters_overlap(
    only_a: Option<&str>,
    except_a: Option<&str>,
    only_b: Option<&str>,
    except_b: Option<&str>,
) -> bool {
    let clean_list = |s: &str| -> Vec<String> {
        s.split(',')
            .map(|x| x.trim().to_lowercase())
            .filter(|x| !x.is_empty())
            .collect()
    };

    let o_a = only_a.map(clean_list);
    let e_a = except_a.map(clean_list);
    let o_b = only_b.map(clean_list);
    let e_b = except_b.map(clean_list);

    if o_a.is_none() && e_a.is_none() && o_b.is_none() && e_b.is_none() {
        return true;
    }

    match (o_a, e_a, o_b, e_b) {
        (Some(only_a), None, Some(only_b), None) => only_a.iter().any(|a| only_b.contains(a)),
        (Some(only_a), None, None, Some(except_b)) => only_a.iter().any(|a| !except_b.contains(a)),
        (None, Some(except_a), Some(only_b), None) => only_b.iter().any(|b| !except_a.contains(b)),
        _ => true,
    }
}

pub fn find_trigger_overlap_conflict(
    conn: &Connection,
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
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
        "SELECT id, trigger_type, trigger, target_os, only_apps, except_apps
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
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    for row in rows {
        let (
            id,
            trigger_type_raw,
            existing_trigger,
            existing_target_os,
            existing_only,
            existing_except,
        ) = row?;
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

        if overlaps
            && target_os_values_overlap(&existing_target_os, target_os)
            && app_filters_overlap(
                only_apps,
                except_apps,
                existing_only.as_deref(),
                existing_except.as_deref(),
            )
        {
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
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    exclude_id: Option<&str>,
) -> Result<()> {
    validate_trigger_type(trigger_type, target_os)?;

    if let Some(conflict) = find_trigger_overlap_conflict(
        conn,
        trigger_type,
        trigger,
        target_os,
        only_apps,
        except_apps,
        exclude_id,
    )? {
        return Err(crate::Error::Config(format!(
            "Trigger conflict for {} '{}' on target_os '{}': overlaps existing target_os '{}' with overlapping app filters",
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

fn compile_and_save_assets(conn: &Connection, automation_id: &str, output: &str) -> Result<String> {
    let mut processed = String::new();
    let mut ptr = 0;
    let mut active_hashes = std::collections::HashSet::new();

    while let Some(tag) = find_next_tag(output, ptr) {
        processed.push_str(&output[ptr..tag.start]);

        let inner = trim_slice(&output[tag.start + 1..tag.end]);
        let mut rewritten_tag = None;

        if let Some(rest) = inner.strip_prefix("img(")
            && rest.ends_with(')')
        {
            let path = trim_slice(&rest[..rest.len() - 1]);
            if path.starts_with("asset(") && path.ends_with(')') {
                let hash = trim_slice(&path[6..path.len() - 1]);
                active_hashes.insert(hash.to_string());
                rewritten_tag = Some(format!("[img(asset({}))]", hash));
            } else if !path.is_empty()
                && let Some(path_buf) = crate::engine::variables::system::file::expand_path(path)
            {
                let bytes = std::fs::read(&path_buf).map_err(|_| {
                    crate::Error::Config(format!("img: file not found: {}", path_buf.display()))
                })?;

                // Validate the bytes are a recognized image format.
                image::guess_format(&bytes).map_err(|_| {
                    crate::Error::Config(format!(
                        "img: '{}' is not a supported image file (PNG or JPEG required)",
                        path_buf.display()
                    ))
                })?;

                let compressed = zstd::bulk::compress(&bytes, 3).map_err(|e| {
                    crate::Error::Service(format!("zstd compression failed: {}", e))
                })?;
                let mut hasher = Sha256::new();
                hasher.update(&compressed);
                let hash = hex::encode(hasher.finalize());

                let mime_type = match path_buf.extension().and_then(|ext| ext.to_str()) {
                    Some(ext) => match ext.to_lowercase().as_str() {
                        "png" => "image/png",
                        "jpg" | "jpeg" => "image/jpeg",
                        "gif" => "image/gif",
                        "bmp" => "image/bmp",
                        _ => "application/octet-stream",
                    },
                    None => "application/octet-stream",
                };

                let now = now_unix_secs();
                conn.execute(
                    "INSERT OR REPLACE INTO assets (id, automation_id, mime_type, compressed_content, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    (
                        &hash,
                        automation_id,
                        mime_type,
                        &compressed,
                        now,
                    ),
                )?;

                active_hashes.insert(hash.clone());
                rewritten_tag = Some(format!("[img(asset({}))]", hash));
            }
        } else if inner.starts_with("exec.")
            && inner.contains(".file(")
            && let Ok(invocation) = crate::engine::variables::system::exec::parse_invocation(inner)
            && invocation.file
        {
            let path = invocation.subject.trim();
            if path.starts_with("asset(") && path.ends_with(')') {
                let hash = trim_slice(&path[6..path.len() - 1]);
                active_hashes.insert(hash.to_string());
                rewritten_tag = Some(format!("[{}]", inner));
            } else if !path.is_empty()
                && let Some(path_buf) = crate::engine::variables::system::file::expand_path(path)
                && let Ok(bytes) = std::fs::read(&path_buf)
            {
                let compressed = zstd::bulk::compress(&bytes, 3).map_err(|e| {
                    crate::Error::Service(format!("zstd compression failed: {}", e))
                })?;
                let mut hasher = Sha256::new();
                hasher.update(&compressed);
                let hash = hex::encode(hasher.finalize());

                let mime_type = match path_buf.extension().and_then(|ext| ext.to_str()) {
                    Some(ext) => match ext.to_lowercase().as_str() {
                        "sh" | "bash" => "text/x-shellscript",
                        "py" => "text/x-python",
                        "js" => "text/javascript",
                        "ps1" => "text/x-powershell",
                        _ => "application/octet-stream",
                    },
                    None => "application/octet-stream",
                };

                let now = now_unix_secs();
                conn.execute(
                    "INSERT OR REPLACE INTO assets (id, automation_id, mime_type, compressed_content, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    (
                        &hash,
                        automation_id,
                        mime_type,
                        &compressed,
                        now,
                    ),
                )?;

                active_hashes.insert(hash.clone());
                let file_pattern = format!("file({})", invocation.subject);
                let replacement = format!("file(asset({}))", hash);
                let new_inner = inner.replace(&file_pattern, &replacement);
                rewritten_tag = Some(format!("[{}]", new_inner));
            }
        }

        if let Some(rewritten) = rewritten_tag {
            processed.push_str(&rewritten);
        } else {
            processed.push_str(&output[tag.start..tag.end + 1]);
        }

        ptr = tag.end + 1;
    }
    processed.push_str(&output[ptr..]);

    if active_hashes.is_empty() {
        conn.execute(
            "DELETE FROM assets WHERE automation_id = ?1",
            [automation_id],
        )?;
    } else {
        let placeholders: Vec<String> = active_hashes.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "DELETE FROM assets WHERE automation_id = ?1 AND id NOT IN ({})",
            placeholders.join(",")
        );
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&automation_id];
        for h in &active_hashes {
            params.push(h);
        }
        conn.execute(&query, rusqlite::params_from_iter(params))?;
    }

    Ok(processed)
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

    if action_type == "text" {
        let processed_output = compile_and_save_assets(conn, id, output)?;
        if processed_output != output {
            conn.execute(
                "UPDATE automations SET output = ?1, updated_at = ?2 WHERE id = ?3",
                (&processed_output, now, id),
            )?;
        }
    }

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

fn count_directives_in_template(payload: &str) -> (usize, bool) {
    let mut cursor_count = 0;
    let mut has_key_or_delay = false;
    let mut ptr = 0;
    while let Some(tag) = find_next_tag(payload, ptr) {
        let inner = trim_slice(&payload[tag.start + 1..tag.end]);
        let (key, _) = split_key_default(inner);
        if key == "cursor" {
            cursor_count += 1;
        } else if key.starts_with("key(")
            || key.starts_with("delay(")
            || key.starts_with("mouse.")
            || key.starts_with("mouse(")
        {
            has_key_or_delay = true;
        }
        ptr = tag.end + 1;
    }
    (cursor_count, has_key_or_delay)
}

fn count_ai_calls_in_template(payload: &str) -> usize {
    let mut count = 0;
    let mut ptr = 0;
    while let Some(tag) = find_next_tag(payload, ptr) {
        let inner = trim_slice(&payload[tag.start + 1..tag.end]);
        let pipeline = crate::engine::variables::system::transformers::split_pipeline(inner);
        for part in &pipeline[1..] {
            if crate::engine::variables::system::transformers::is_ai_transformer(part) {
                count += 1;
            }
        }
        ptr = tag.end + 1;
    }
    count
}

fn get_referenced_triggers(payload: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut ptr = 0;
    while let Some(tag) = find_next_tag(payload, ptr) {
        let inner = trim_slice(&payload[tag.start + 1..tag.end]);
        let (key, _) = split_key_default(inner);
        if key.starts_with("use(")
            && key.ends_with(')')
            && let Some(inner_key) = key.strip_prefix("use(").and_then(|k| k.strip_suffix(')'))
        {
            let unquoted = crate::engine::variables::system::strip_quotes(inner_key.trim())
                .map(|s| s.to_string())
                .unwrap_or_else(|| inner_key.trim().to_string());
            refs.push(unquoted);
        }
        ptr = tag.end + 1;
    }
    refs
}

#[allow(clippy::too_many_arguments)]
fn check_limits_recursive(
    catalog: &std::collections::HashMap<String, String>,
    trigger: &str,
    visited: &mut std::collections::HashSet<String>,
    depth: usize,
    max_depth: &mut usize,
    ai_count: &mut usize,
    cursor_count: &mut usize,
    has_key_or_delay: &mut bool,
) -> Result<()> {
    if visited.contains(trigger) {
        return Err(crate::Error::Config(format!(
            "Circular reference detected involving trigger '{}'",
            trigger
        )));
    }

    *max_depth = std::cmp::max(*max_depth, depth);
    if *max_depth > 5 {
        return Err(crate::Error::Config(
            "Nested snippet depth exceeds the maximum limit of 5".to_string(),
        ));
    }

    visited.insert(trigger.to_string());

    if let Some(template) = catalog.get(trigger) {
        let nested_ai = count_ai_calls_in_template(template);
        *ai_count += nested_ai;
        if *ai_count > 3 {
            return Err(crate::Error::Config(format!(
                "Total expanded AI calls ({}) exceeds the limit of 3",
                ai_count
            )));
        }

        let (c_count, has_kd) = count_directives_in_template(template);
        *cursor_count += c_count;
        *has_key_or_delay = *has_key_or_delay || has_kd;

        if *cursor_count > 1 {
            return Err(crate::Error::Config(
                "Multiple [cursor] tags found in expanded snippet sequence. Only one [cursor] is allowed.".to_string()
            ));
        }

        if *cursor_count > 0 && *has_key_or_delay {
            return Err(crate::Error::Config(
                "The [cursor] directive cannot be used alongside [key.*], [delay.*], or [mouse.*] directives in the same expanded snippet sequence.".to_string()
            ));
        }

        let refs = get_referenced_triggers(template);
        for r in refs {
            check_limits_recursive(
                catalog,
                &r,
                visited,
                depth + 1,
                max_depth,
                ai_count,
                cursor_count,
                has_key_or_delay,
            )?;
        }
    }

    visited.remove(trigger);
    Ok(())
}

pub fn validate_automation_limits(
    conn: &Connection,
    new_trigger: &str,
    new_content: &str,
    action_type: &str,
) -> Result<()> {
    let mut catalog = std::collections::HashMap::new();

    if let Ok(actions) = super::automation_get::get_all_active_automations(conn) {
        for (trigger, action) in actions {
            if action.action_type == "text" {
                catalog.insert(trigger, action.output);
            }
        }
    }

    if action_type == "text" {
        catalog.insert(new_trigger.to_string(), new_content.to_string());
    } else {
        catalog.remove(new_trigger);
    }

    for (trigger, template) in &catalog {
        let mut visited = std::collections::HashSet::new();
        let mut max_depth = 0;
        let mut ai_count = count_ai_calls_in_template(template);
        let (mut cursor_count, mut has_key_or_delay) = count_directives_in_template(template);

        if ai_count > 3 {
            return Err(crate::Error::Config(format!(
                "Snippet '{}' contains {} AI calls, exceeding the limit of 3",
                trigger, ai_count
            )));
        }

        if cursor_count > 1 {
            return Err(crate::Error::Config(
                "Multiple [cursor] tags found. Only one [cursor] is allowed.".to_string(),
            ));
        }

        if cursor_count > 0 && has_key_or_delay {
            return Err(crate::Error::Config(
                "The [cursor] directive cannot be used alongside [key.*], [delay.*], or [mouse.*] directives.".to_string()
            ));
        }

        visited.insert(trigger.clone());
        let refs = get_referenced_triggers(template);
        for r in refs {
            check_limits_recursive(
                &catalog,
                &r,
                &mut visited,
                1,
                &mut max_depth,
                &mut ai_count,
                &mut cursor_count,
                &mut has_key_or_delay,
            )?;
        }
    }

    Ok(())
}

pub fn update_existing_automation(
    conn: &mut Connection,
    update: ExistingAutomationUpdate<'_>,
) -> Result<()> {
    validate_target_os_value(update.target_os)?;
    let action_kind = parse_action_kind(update.action_type)?;
    if action_kind == AutomationActionKind::Text {
        audit_payload_tags_with_trigger_type(update.content, update.trigger_type)?;
    }

    // We only enforce limits for text snippets, as nested limits apply to the `use` variable
    validate_automation_limits(conn, update.trigger, update.content, update.action_type)?;

    let prepared =
        prepare_trigger_with_type(update.trigger, update.trigger_type, update.target_os)?;

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

    if action_kind == AutomationActionKind::Script {
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
    let action_kind = parse_action_kind(new_automation.action_type)?;
    if action_kind == AutomationActionKind::Text {
        audit_payload_tags_with_trigger_type(new_automation.content, new_automation.trigger_type)?;
    }

    validate_automation_limits(
        conn,
        new_automation.trigger,
        new_automation.content,
        new_automation.action_type,
    )?;

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
    // Validate conflict before opening the transaction so no partial writes happen.
    validate_trigger_target_os_conflict(
        conn,
        prepared.trigger_type,
        &prepared.stored_trigger,
        new_automation.target_os,
        None,
        None,
        None,
    )?;

    let tx = conn.transaction()?;

    if action_kind == AutomationActionKind::Script {
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

fn parse_action_kind(action_type: &str) -> Result<AutomationActionKind> {
    if action_type.eq_ignore_ascii_case("text") {
        Ok(AutomationActionKind::Text)
    } else if action_type.eq_ignore_ascii_case("script") {
        Ok(AutomationActionKind::Script)
    } else {
        Err(crate::Error::Config(format!(
            "Unsupported action_type '{}'. Expected 'text' or 'script'.",
            action_type
        )))
    }
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
    let mut paren_depth = 0usize;
    let mut ptr = 0;

    while ptr < bytes.len() {
        if bytes[ptr] == TAG_OPEN && !is_escaped(bytes, ptr) {
            depth += 1;
        } else if bytes[ptr] == TAG_CLOSE && !is_escaped(bytes, ptr) {
            depth = depth.saturating_sub(1);
        } else if bytes[ptr] == b'(' && !is_escaped(bytes, ptr) {
            paren_depth += 1;
        } else if bytes[ptr] == b')' && !is_escaped(bytes, ptr) {
            paren_depth = paren_depth.saturating_sub(1);
        } else if bytes[ptr] == b'=' && depth == 0 && paren_depth == 0 {
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

pub fn update_automation_app_filters(
    conn: &Connection,
    id: &str,
    only_apps: Option<String>,
    except_apps: Option<String>,
) -> Result<()> {
    conn.execute(
        "UPDATE automations
         SET only_apps = ?1, except_apps = ?2
         WHERE id = ?3 AND is_deleted = 0",
        rusqlite::params![only_apps, except_apps, id],
    )?;
    Ok(())
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
#[allow(clippy::too_many_arguments)]
pub fn add_automation_by_trigger(
    conn: &Connection,
    trigger: &str,
    output: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    tags: Option<Vec<String>>,
) -> Result<AddOutcome> {
    add_automation_by_trigger_type(
        conn,
        TriggerType::Word,
        trigger,
        output,
        target_os,
        only_apps,
        except_apps,
        tags,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_automation_by_trigger_type(
    conn: &Connection,
    trigger_type: TriggerType,
    trigger: &str,
    output: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    tags: Option<Vec<String>>,
) -> Result<AddOutcome> {
    validate_trigger_not_reserved(conn, trigger)?;

    let existing: Option<(String, String, String)> = conn
        .query_row(
            "SELECT id, output, action_type
             FROM automations
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
                trigger,
                target_os,
                only_apps.unwrap_or(""),
                except_apps.unwrap_or(""),
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();

    match existing {
        Some((id, existing_output, existing_action))
            if existing_output == output && existing_action == "text" =>
        {
            if let Some(ref t) = tags {
                let now = now_unix_secs();
                let t_json =
                    serde_json::to_string(t).map_err(|e| crate::Error::Config(e.to_string()))?;
                conn.execute(
                    "UPDATE automations
                     SET tags        = ?1,
                         updated_at  = ?2,
                         version     = version + 1
                     WHERE id = ?3",
                    rusqlite::params![t_json, now, id],
                )?;
                Ok(AddOutcome::Updated)
            } else {
                Ok(AddOutcome::AlreadyExists)
            }
        }
        Some((id, _, _)) => {
            let now = now_unix_secs();
            if let Some(ref t) = tags {
                let t_json =
                    serde_json::to_string(t).map_err(|e| crate::Error::Config(e.to_string()))?;
                conn.execute(
                    "UPDATE automations
                     SET output      = ?1,
                         action_type = 'text',
                         tags        = ?2,
                         updated_at  = ?3,
                         version     = version + 1
                     WHERE id = ?4",
                    rusqlite::params![output, t_json, now, id],
                )?;
            } else {
                conn.execute(
                    "UPDATE automations
                     SET output      = ?1,
                         action_type = 'text',
                         updated_at  = ?2,
                         version     = version + 1
                     WHERE id = ?3",
                    rusqlite::params![output, now, id],
                )?;
            }
            conn.execute(
                "DELETE FROM scripts WHERE automation_id = ?1",
                rusqlite::params![id],
            )?;
            Ok(AddOutcome::Updated)
        }
        None => {
            validate_trigger_target_os_conflict(
                conn,
                trigger_type,
                trigger,
                target_os,
                only_apps,
                except_apps,
                None,
            )?;

            let id = uuid::Uuid::new_v4().to_string();
            let tags_str = if let Some(ref t) = tags {
                serde_json::to_string(t).map_err(|e| crate::Error::Config(e.to_string()))?
            } else {
                "[]".to_string()
            };
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
                &tags_str,
                0,
                None,
            )?;

            conn.execute(
                "UPDATE automations
                 SET only_apps = ?1, except_apps = ?2
                 WHERE id = ?3",
                rusqlite::params![only_apps, except_apps, id],
            )?;

            Ok(AddOutcome::Created)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_ai_calls_in_template() {
        assert_eq!(count_ai_calls_in_template("Hello world"), 0);
        assert_eq!(count_ai_calls_in_template("Hello [clip | upper]"), 0);
        assert_eq!(count_ai_calls_in_template("[clip | ai(summarize)]"), 1);
        assert_eq!(
            count_ai_calls_in_template("[clip | ai(a) | upper | ai(b)] and [date | ai(c)]"),
            3
        );
    }

    #[test]
    fn test_get_referenced_triggers() {
        assert!(get_referenced_triggers("Hello world").is_empty());
        assert_eq!(get_referenced_triggers("Hello [use(\"foo\")]"), vec!["foo"]);
        assert_eq!(
            get_referenced_triggers("[use('bar')] [use(baz)]"),
            vec!["bar", "baz"]
        );
    }

    #[test]
    fn test_check_limits_recursive_detects_cycles() {
        let mut catalog = std::collections::HashMap::new();
        catalog.insert("A".to_string(), "calls [use(\"B\")]".to_string());
        catalog.insert("B".to_string(), "calls [use(\"A\")]".to_string());

        let mut visited = std::collections::HashSet::new();
        let mut max_depth = 0;
        let mut ai_count = 0;
        let mut cursor_count = 0;
        let mut has_key_or_delay = false;

        let result = check_limits_recursive(
            &catalog,
            "A",
            &mut visited,
            1,
            &mut max_depth,
            &mut ai_count,
            &mut cursor_count,
            &mut has_key_or_delay,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Circular reference")
        );
    }

    #[test]
    fn test_check_limits_recursive_enforces_depth() {
        let mut catalog = std::collections::HashMap::new();
        catalog.insert("1".to_string(), "[use(\"2\")]".to_string());
        catalog.insert("2".to_string(), "[use(\"3\")]".to_string());
        catalog.insert("3".to_string(), "[use(\"4\")]".to_string());
        catalog.insert("4".to_string(), "[use(\"5\")]".to_string());
        catalog.insert("5".to_string(), "[use(\"6\")]".to_string());
        catalog.insert("6".to_string(), "done".to_string());

        let mut visited = std::collections::HashSet::new();
        let mut max_depth = 0;
        let mut ai_count = 0;
        let mut cursor_count = 0;
        let mut has_key_or_delay = false;

        let result = check_limits_recursive(
            &catalog,
            "1",
            &mut visited,
            1,
            &mut max_depth,
            &mut ai_count,
            &mut cursor_count,
            &mut has_key_or_delay,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("maximum limit of 5")
        );
    }

    #[test]
    fn test_check_limits_recursive_enforces_ai_count() {
        let mut catalog = std::collections::HashMap::new();
        catalog.insert("A".to_string(), "[clip | ai(1)] [use(\"B\")]".to_string());
        catalog.insert("B".to_string(), "[clip | ai(2)] [use(\"C\")]".to_string());
        catalog.insert("C".to_string(), "[clip | ai(3)] [clip | ai(4)]".to_string());

        let mut visited = std::collections::HashSet::new();
        let mut max_depth = 0;
        let mut ai_count = 0;
        let mut cursor_count = 0;
        let mut has_key_or_delay = false;

        let result = check_limits_recursive(
            &catalog,
            "A",
            &mut visited,
            1,
            &mut max_depth,
            &mut ai_count,
            &mut cursor_count,
            &mut has_key_or_delay,
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("exceeds the limit of 3")
        );
    }

    #[test]
    fn test_app_filters_overlap_logic() {
        // No filters -> overlaps
        assert!(app_filters_overlap(None, None, None, None));

        // One only_apps vs no filters -> overlaps
        assert!(app_filters_overlap(Some("exe:code"), None, None, None));
        assert!(app_filters_overlap(None, None, Some("exe:notepad"), None));

        // Non-intersecting only_apps -> no overlap
        assert!(!app_filters_overlap(
            Some("exe:code"),
            None,
            Some("exe:notepad"),
            None
        ));

        // Intersecting only_apps -> overlaps
        assert!(app_filters_overlap(
            Some("exe:code,exe:notepad"),
            None,
            Some("exe:notepad"),
            None
        ));

        // Non-intersecting only_apps vs except_apps -> overlaps
        assert!(app_filters_overlap(
            Some("exe:code"),
            None,
            None,
            Some("exe:notepad")
        ));

        // Intersecting only_apps vs except_apps -> no overlap (if only_a is in except_b)
        assert!(!app_filters_overlap(
            Some("exe:code"),
            None,
            None,
            Some("exe:code")
        ));
    }
}
