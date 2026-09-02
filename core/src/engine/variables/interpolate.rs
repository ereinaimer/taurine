use super::registry::split_system_tag;
use super::system;
use super::types::ArgMap;

use indexmap::IndexMap;

use super::tags::*;

const SENTINEL_OPEN: char = '\x01';
const SENTINEL_CLOSE: char = '\x02';
const MAX_INTERPOLATION_DEPTH: usize = 32;
const MAX_ITERATIONS: usize = 128;

#[derive(Debug, PartialEq)]
pub(crate) struct Placeholder<'a> {
    pub key: &'a str,
    pub default_value: Option<&'a str>,
}

fn has_valid_default_value(default_value: Option<&str>) -> bool {
    if let Some(dv) = default_value {
        let unquoted = system::strip_quotes(dv).unwrap_or(dv);
        !unquoted.trim().is_empty()
    } else {
        false
    }
}

pub(crate) fn extract_placeholders<'a>(template: &'a str) -> IndexMap<&'a str, Placeholder<'a>> {
    let mut placeholders = IndexMap::new();

    let mut tags = scan_tag_bounds(template);
    tags.sort_by_key(|tag| tag.start);

    for tag in tags {
        let inner = trim_slice(&template[tag.start + 1..tag.end]);
        let pipeline = system::transformers::split_pipeline(inner);
        let base_expr = pipeline[0];
        let (key, default_value) = split_key_default(base_expr);

        let key_unquoted = system::strip_quotes(key).unwrap_or(key);

        if has_valid_default_value(default_value)
            && !system::is_reserved(key_unquoted)
            && !placeholders.contains_key(key_unquoted)
            && !key_unquoted.contains('[')
            && !key_unquoted.contains(']')
        {
            placeholders.insert(
                key_unquoted,
                Placeholder {
                    key: key_unquoted,
                    default_value,
                },
            );
        }
    }

    placeholders
}

fn is_valid_user_reference(key: &str, default_value: Option<&str>, args: &ArgMap) -> bool {
    let key_unquoted = system::strip_quotes(key).unwrap_or(key);

    if let Some((root, modifier)) = split_system_tag(key_unquoted)
        && super::registry::validate_system_tag(root, modifier).is_ok()
    {
        return false;
    }

    if let Ok(index) = key_unquoted.parse::<usize>() {
        return index < args.positional.len() || has_valid_default_value(default_value);
    }

    args.named.contains_key(key_unquoted)
        || (has_valid_default_value(default_value)
            && !system::is_reserved(key_unquoted)
            && !key_unquoted.contains('[')
            && !key_unquoted.contains(']'))
}

fn resolve_user_placeholder(
    key: &str,
    default_value: Option<&str>,
    args: &ArgMap,
    depth: usize,
) -> Option<String> {
    if !is_valid_user_reference(key, default_value, args) {
        return None;
    }

    let key_unquoted = system::strip_quotes(key).unwrap_or(key);
    let default_val_unquoted = default_value.map(|v| system::strip_quotes(v).unwrap_or(v));

    if let Ok(index) = key_unquoted.parse::<usize>() {
        args.positional
            .get(index)
            .cloned()
            .or_else(|| args.named.get(key_unquoted).cloned())
            .or_else(|| default_val_unquoted.map(|value| resolve_default_value(value, args, depth)))
    } else if let Some(value) = args.named.get(key_unquoted) {
        Some(value.clone())
    } else {
        default_val_unquoted.map(|value| resolve_default_value(value, args, depth))
    }
}

fn resolve_default_value(default_value: &str, args: &ArgMap, depth: usize) -> String {
    if depth >= MAX_INTERPOLATION_DEPTH {
        default_value.to_string()
    } else {
        interpolate_with_depth(default_value, args, depth + 1)
    }
}

pub fn interpolate(template: &str, args: &ArgMap) -> String {
    // Fast path: if template contains no variable tags, pipelines, or escape characters,
    // and args are empty, return template immediately without pipeline splitting or placeholder extraction.
    if args.positional.is_empty()
        && args.named.is_empty()
        && !template.contains('[')
        && !template.contains('|')
        && !template.contains('\\')
    {
        return template.to_string();
    }

    let segments = system::transformers::split_pipeline(template);
    if segments.len() > 1 {
        let mut all_valid = true;
        for tr in &segments[1..] {
            if !system::transformers::is_valid_transformer(tr) {
                all_valid = false;
                break;
            }
        }
        if all_valid {
            let base_expr = system::strip_quotes(segments[0]).unwrap_or(segments[0]);
            let mut base_result = interpolate_with_depth(base_expr, args, 0);
            for tr in &segments[1..] {
                if system::transformers::is_ai_transformer(tr) {
                    let prompt = system::transformers::extract_ai_prompt(tr).to_string();
                    base_result = format!("\x03{}\x1F{}\x04", base_result, prompt);
                } else if let Some(transformed) = system::transformers::apply(tr, &base_result) {
                    base_result = transformed;
                }
            }
            return base_result.replace("\\|", "|");
        }
    }

    interpolate_with_depth(template, args, 0).replace("\\|", "|")
}

fn interpolate_with_depth(template: &str, args: &ArgMap, depth: usize) -> String {
    if depth >= MAX_INTERPOLATION_DEPTH {
        return template.to_string();
    }

    let placeholders = extract_placeholders(template);
    let mut user_resolutions = std::collections::HashMap::new();

    for (key, placeholder) in placeholders.iter() {
        if let Some(resolved) =
            resolve_user_placeholder(key, placeholder.default_value, args, depth)
        {
            user_resolutions.insert(*key, resolved);
        }
    }

    let mut output = template.to_string();
    let mut iterations = 0;

    while iterations < MAX_ITERATIONS {
        if let Some((start, end)) = find_innermost_tag(&output) {
            let inner = trim_slice(&output[start + 1..end]);
            let pipeline = system::transformers::split_pipeline(inner);
            let base_expr = pipeline[0];
            let transformers = &pipeline[1..];
            let (key, default_value) = split_key_default(base_expr);

            let key_unquoted = system::strip_quotes(key).unwrap_or(key);

            let base_resolved = if let Some(user) = user_resolutions.get(key_unquoted) {
                Some(user.clone())
            } else if key_unquoted.starts_with('\x03') && key_unquoted.ends_with('\x04') {
                Some(key_unquoted.to_string())
            } else if key_unquoted.starts_with("use(") && key_unquoted.ends_with(')') {
                Some(resolve_use_placeholder(key_unquoted, args, depth))
            } else if let Some(sys) = resolve_system_placeholder(key_unquoted) {
                Some(sys)
            } else if is_valid_user_reference(key_unquoted, default_value, args) {
                resolve_user_placeholder(key, default_value, args, depth)
            } else if let Some(unquoted) = system::strip_quotes(key) {
                Some(unquoted.to_string())
            } else if key.chars().all(|c| c.is_ascii_digit()) {
                resolve_user_placeholder(key, default_value, args, depth)
            } else if !transformers.is_empty()
                && !system::is_reserved(key_unquoted)
                && (key.contains(' ') || key.contains(SENTINEL_OPEN))
            {
                Some(key.to_string())
            } else {
                None
            };

            let resolved = if let Some(mut text) = base_resolved {
                let mut valid_pipeline = true;
                for tr in transformers {
                    if system::transformers::is_ai_transformer(tr) {
                        let prompt = system::transformers::extract_ai_prompt(tr).to_string();
                        // Embed AI marker: \x03 + input + \x1F (unit sep) + prompt + \x04
                        // \x1F is a C0 control character never present in normal text output
                        text = format!("\x03{text}\x1F{prompt}\x04");
                    } else if text.starts_with("\x03\x1Fsys:") && text.ends_with('\x04') {
                        let inner_sys = &text[..text.len() - 1]; // strip trailing \x04
                        text = format!("{} | {}\x04", inner_sys, tr);
                    } else if let Some(transformed) = system::transformers::apply(tr, &text) {
                        text = transformed;
                    } else {
                        valid_pipeline = false;
                        break;
                    }
                }
                if valid_pipeline {
                    text
                } else {
                    format!("{SENTINEL_OPEN}{inner}{SENTINEL_CLOSE}")
                }
            } else if system::is_directive(key_unquoted) && transformers.is_empty() {
                format!("{SENTINEL_OPEN}{key_unquoted}{SENTINEL_CLOSE}")
            } else {
                format!("{SENTINEL_OPEN}{inner}{SENTINEL_CLOSE}")
            };

            output.replace_range(start..end + 1, &resolved);
            iterations += 1;
        } else {
            break;
        }
    }

    // Restore sentinels and handle escapes
    finalize_interpolation(output)
}

fn resolve_system_placeholder(key: &str) -> Option<String> {
    if super::system::is_deferred(key) {
        return Some(format!("\x03\x1Fsys:{key}\x04"));
    }
    super::system::resolve(key)
}

fn parse_use_key(key: &str) -> Option<String> {
    let inner = key.strip_prefix("use(")?.strip_suffix(')')?;
    let unquoted = super::system::strip_quotes(inner.trim())
        .map(|s| s.to_string())
        .unwrap_or_else(|| inner.trim().to_string());
    Some(unquoted)
}

fn resolve_use_placeholder(key: &str, args: &ArgMap, depth: usize) -> String {
    if depth >= 5 {
        return "[Error: Max recursion depth reached]".to_string();
    }

    let trigger_name = match parse_use_key(key) {
        Some(name) => name,
        None => return "[Error: Malformed use key]".to_string(),
    };

    let conn = match crate::db::get_conn() {
        Ok(c) => c,
        Err(e) => return format!("[Error: Database pool error: {}]", e),
    };

    let action = match crate::db::crud::triggers::get_action_by_trigger(&conn, &trigger_name) {
        Ok(Some(act)) => act,
        Ok(None) => return format!("[Error: Snippet '{}' does not exist]", trigger_name),
        Err(e) => return format!("[Error: Database query error: {}]", e),
    };

    if !action.is_text() {
        return format!("[Error: Cannot invoke non-text snippet '{}']", trigger_name);
    }

    interpolate_with_depth(&action.output, args, depth + 1)
}

/// Returns true if the interpolated string contains any embedded AI transformer markers.
pub fn contains_ai_markers(s: &str) -> bool {
    s.contains('\x03')
}

/// Returns true if the interpolated string contains any markers that require AI LLM invocation (i.e. non-sys markers).
pub fn contains_non_sys_markers(s: &str) -> bool {
    let mut rest = s;
    while let Some(sot) = rest.find('\x03') {
        let after = &rest[sot + 1..];
        if let Some(eot) = after.find('\x04') {
            let content = &after[..eot];
            if let Some(sep) = content.find('\x1F') {
                if !content[sep + 1..].starts_with("sys:") {
                    return true;
                }
            } else {
                return true;
            }
            rest = &after[eot + 1..];
        } else {
            break;
        }
    }
    false
}

/// Extracts all AI transformer markers from an interpolated string.
/// Returns a list of `(input, prompt)` pairs in the order they appear.
/// The `template_with_markers` string itself should be passed to the daemon for async resolution.
pub fn extract_ai_markers(s: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let mut rest = s;
    while let Some(sot) = rest.find('\x03') {
        let after_sot = &rest[sot + 1..];
        if let Some(eot) = after_sot.find('\x04') {
            let content = &after_sot[..eot];
            if let Some(sep) = content.find('\x1F') {
                let input = content[..sep].to_string();
                let prompt = content[sep + 1..].to_string();
                results.push((input, prompt));
            }
            rest = &after_sot[eot + 1..];
        } else {
            break;
        }
    }
    results
}

fn find_innermost_tag(s: &str) -> Option<(usize, usize)> {
    scan_tag_bounds(s)
        .into_iter()
        .next()
        .map(|tag| (tag.start, tag.end))
}

fn finalize_interpolation(s: String) -> String {
    // 1. Remove sentinel markers
    s.replace(SENTINEL_OPEN, "[").replace(SENTINEL_CLOSE, "]")
}
