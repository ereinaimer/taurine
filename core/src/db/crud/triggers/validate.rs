use super::TriggerType;
use super::trigger_set::*;
use crate::Result;
use crate::engine::variables::tags::*;
use crate::engine::variables::{
    ValidationError, split_system_tag, valid_modifier_hint, validate_system_tag,
};
use rusqlite::Connection;

pub fn normalize_tags(tags_json: &str) -> Result<String> {
    let tags: Vec<String> = serde_json::from_str(tags_json)
        .map_err(|e| crate::Error::Config(format!("Invalid tags JSON: {}", e)))?;

    let mut normalized: Vec<String> = Vec::new();
    for tag in tags {
        let trimmed = tag.trim().to_lowercase();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.len() > MAX_TAG_LENGTH {
            return Err(crate::Error::Config(format!(
                "tag '{}' exceeds {} character limit",
                trimmed, MAX_TAG_LENGTH,
            )));
        }
        if !normalized.contains(&trimmed) {
            normalized.push(trimmed);
        }
    }

    if normalized.len() > MAX_TAGS_COUNT {
        return Err(crate::Error::Config(format!(
            "tag count ({}) exceeds limit of {}",
            normalized.len(),
            MAX_TAGS_COUNT,
        )));
    }

    serde_json::to_string(&normalized)
        .map_err(|e| crate::Error::Config(format!("Failed to serialize tags: {}", e)))
}

pub(crate) fn collect_defined_variables(payload: &str) -> std::collections::HashSet<String> {
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
    audit_payload_tags_impl_opt(payload, &defined_vars, trigger_type, false)
}

pub fn audit_script_payload_tags(payload: &str, trigger_type: TriggerType) -> Result<()> {
    let defined_vars = collect_defined_variables(payload);
    audit_payload_tags_impl_opt(payload, &defined_vars, trigger_type, true)
}

pub(crate) fn audit_payload_tags_impl_opt(
    payload: &str,
    defined_vars: &std::collections::HashSet<String>,
    trigger_type: TriggerType,
    is_script: bool,
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
            audit_payload_tags_impl_opt(inner, defined_vars, trigger_type, is_script)?;
        } else if !is_nested {
            if let Some((root, modifier)) = split_system_tag(key) {
                if root == "cursor" {
                    cursor_count += 1;
                    if cursor_count > 1 {
                        return Err(crate::Error::Config(
                            "[cursor]: multiple cursor directives".to_string(),
                        ));
                    }
                }

                if matches!(root, "key" | "delay" | "mouse") {
                    has_key_or_delay = true;
                }

                if let Some(_default) = default_value {
                    return Err(crate::Error::Config(format!(
                        "[{}]: system tags cannot have defaults. {}",
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
                let is_user_var = default_value.is_some() || defined_vars.contains(key_unquoted);

                if (!is_script || is_user_var) && key_unquoted.contains('.') {
                    return Err(crate::Error::Config(format!(
                        "[{}]: dots reserved for system variables",
                        inner
                    )));
                }

                if !is_script || is_user_var {
                    match default_value {
                        None => {
                            let is_positional = !key_unquoted.is_empty()
                                && key_unquoted.chars().all(|c| c.is_ascii_digit());
                            let is_allowed_regex_positional =
                                matches!(trigger_type, TriggerType::Regex) && is_positional;
                            if !defined_vars.contains(key_unquoted) && !is_allowed_regex_positional
                            {
                                return Err(crate::Error::Config(format!(
                                    "[{}]: dynamic variables need a default (e.g., [key=default])",
                                    inner
                                )));
                            }
                        }
                        Some(val) => {
                            let unquoted =
                                crate::engine::variables::system::strip_quotes(val).unwrap_or(val);
                            if unquoted.trim().is_empty() {
                                return Err(crate::Error::Config(format!(
                                    "[{}]: default value cannot be empty",
                                    inner
                                )));
                            }
                        }
                    }
                }
            }
        }

        ptr = tag.end + 1;
    }

    if cursor_count > 0 && has_key_or_delay {
        return Err(crate::Error::Config(
            "[cursor] conflicts with key/delay/mouse directives".to_string(),
        ));
    }

    Ok(())
}

pub(crate) fn validate_trigger_type(trigger_type: TriggerType, target_os: &str) -> Result<()> {
    if matches!(trigger_type, TriggerType::Hotkey) && matches!(target_os, "android" | "ios") {
        return Err(crate::Error::Config(format!(
            "Hotkey triggers are only supported for desktop target_os values; got '{}'",
            target_os
        )));
    }

    Ok(())
}

pub(crate) fn validate_target_os_value(target_os: &str) -> Result<()> {
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

pub(crate) fn format_validation_error(
    raw_tag: &str,
    root: &str,
    modifier: Option<&str>,
    error: &ValidationError,
) -> String {
    match error {
        ValidationError::MissingModifier { .. } => {
            let hint = valid_modifier_hint(root);
            if hint.contains('\n') {
                format!("[{}]: `{}` needs a modifier.\n\n{}", raw_tag, root, hint)
            } else {
                format!("[{}]: `{}` needs a modifier. {}", raw_tag, root, hint)
            }
        }
        ValidationError::UnexpectedModifier { .. } => {
            let hint = valid_modifier_hint(root);
            if hint.contains('\n') {
                format!(
                    "[{}]: `{}` has no modifier `{}`.\n\n{}",
                    raw_tag,
                    root,
                    modifier.unwrap_or_default(),
                    hint
                )
            } else {
                format!(
                    "[{}]: `{}` has no modifier `{}`. {}",
                    raw_tag,
                    root,
                    modifier.unwrap_or_default(),
                    hint
                )
            }
        }
        ValidationError::InvalidModifier { modifier, .. } => {
            let hint = valid_modifier_hint(root);
            if hint.contains('\n') {
                format!(
                    "[{}]: modifier `{}` invalid for `{}`.\n\n{}",
                    raw_tag, modifier, root, hint
                )
            } else {
                format!(
                    "[{}]: modifier `{}` invalid for `{}`. {}",
                    raw_tag, modifier, root, hint
                )
            }
        }
        ValidationError::UnknownRoot(root) => {
            format!("[{}]: unknown root `{}`", raw_tag, root)
        }
    }
}

pub(crate) fn count_directives_in_template(payload: &str) -> (usize, bool) {
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

pub(crate) fn count_ai_calls_in_template(payload: &str) -> usize {
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

pub(crate) fn get_referenced_triggers(payload: &str) -> Vec<String> {
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
pub(crate) fn check_limits_recursive(
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
            "Circular reference: '{}'",
            trigger
        )));
    }

    *max_depth = std::cmp::max(*max_depth, depth);
    if *max_depth > 5 {
        return Err(crate::Error::Config(
            "Nested snippet depth limit (5) exceeded".to_string(),
        ));
    }

    visited.insert(trigger.to_string());

    if let Some(template) = catalog.get(trigger) {
        let nested_ai = count_ai_calls_in_template(template);
        *ai_count += nested_ai;
        if *ai_count > 3 {
            return Err(crate::Error::Config(format!(
                "AI call limit (3) exceeded, total: {}",
                ai_count
            )));
        }

        let (c_count, has_kd) = count_directives_in_template(template);
        *cursor_count += c_count;
        *has_key_or_delay = *has_key_or_delay || has_kd;

        if *cursor_count > 1 {
            return Err(crate::Error::Config(
                "Multiple [cursor] tags found (max 1)".to_string(),
            ));
        }

        if *cursor_count > 0 && *has_key_or_delay {
            return Err(crate::Error::Config(
                "[cursor] conflicts with key/delay/mouse directives".to_string(),
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

pub fn validate_trigger_limits(
    conn: &Connection,
    new_trigger: &str,
    new_content: &str,
    action_type: &str,
) -> Result<()> {
    let mut catalog = std::collections::HashMap::new();

    if let Ok(actions) = super::trigger_get::get_all_active_triggers(conn) {
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
                "Snippet '{}': AI call limit (3) exceeded, has {}",
                trigger, ai_count
            )));
        }

        if cursor_count > 1 {
            return Err(crate::Error::Config(
                "Multiple [cursor] tags found (max 1)".to_string(),
            ));
        }

        if cursor_count > 0 && has_key_or_delay {
            return Err(crate::Error::Config(
                "[cursor] conflicts with key/delay/mouse directives".to_string(),
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

    let mut all_referenced = std::collections::HashSet::new();
    for template in catalog.values() {
        for r in get_referenced_triggers(template) {
            all_referenced.insert(r);
        }
    }

    for ref_trigger in &all_referenced {
        if !catalog.contains_key(ref_trigger) {
            return Err(crate::Error::Config(format!(
                "[use(\"{}\")] does not exist",
                ref_trigger
            )));
        }
    }

    Ok(())
}
