//! System variables module.
//!
//! Centralizes logic for reserved keywords and system-wide markers like `[cursor]`,
//! and future variables like `[time]`.

pub mod clip;
pub mod date;
pub mod env;
pub mod exec;
pub mod file;
pub mod http;
pub mod img;
pub mod lorem;
pub mod net;
pub mod random;
pub mod time;
pub mod transformers;
pub mod uuid;

use super::tags::*;
use crate::engine::variables::types::{ExpansionStep, FinalExpansion};

const CURSOR_TAG: &str = "[cursor]";
const ESCAPED_CURSOR_LITERAL: &str = r#"\[cursor\]"#;
const MAX_OUTPUT_LENGTH: usize = 100_000;

/// Checks if a keyword is reserved by the system.
pub fn is_reserved(key: &str) -> bool {
    key == "cursor"
        || key == "newline"
        || key == "uuid"
        || clip::is_clip_key(key)
        || key == "lorem"
        || key.starts_with("uuid.")
        || key == "time"
        || key.starts_with("time.")
        || key == "date"
        || key.starts_with("date.")
        || key.starts_with("use(")
        || key.starts_with("env(")
        || key.starts_with("file.")
        || key.starts_with("net.")
        || key.starts_with("http.")
        || key.starts_with("exec.")
        || key.starts_with("img(")
        || key.starts_with("random.")
        || key.starts_with("lorem.")
        || key == "mouse"
        || key.starts_with("mouse.")
        || key == "key"
        || key.starts_with("key(")
        || key == "delay"
        || key.starts_with("delay(")
        || key.contains('.') // Reserve all other dot-namespaces
}

/// Checks if a keyword is a post-processing directive.
///
/// Directives are not replaced during interpolation but are instead handled
/// in the `finalize` phase (e.g., `[cursor]`, `[key(tab)]`, `[delay(200ms)]`).
pub fn is_directive(key: &str) -> bool {
    key == "cursor"
        || parse_key_directive(key).is_some()
        || parse_delay_directive(key).is_some()
        || parse_mouse_directive(key).is_some()
}

/// Checks if a system keyword triggers deferred (async) evaluation.
///
/// Deferred variables are replaced with a special marker during interpolation
/// so the daemon can evaluate them in a non-blocking thread and show a braille spinner.
pub fn is_deferred(key: &str) -> bool {
    key == "net.ip" || key.starts_with("net.dns(") || key.starts_with("http.") || key == "mouse.pos"
}

/// Resolves a content-producing system variable.
pub fn resolve(key: &str) -> Option<String> {
    if key == "newline" {
        return Some("\n".to_string());
    }
    if key == "time" || key.starts_with("time.") {
        return time::resolve(key);
    }
    if key == "date" || key.starts_with("date.") {
        return date::resolve(key);
    }
    if key.starts_with("env(") {
        return env::resolve(key);
    }
    if key.starts_with("file.") {
        return file::resolve(key);
    }
    if key.starts_with("net.") {
        return net::resolve(key);
    }
    if key.starts_with("http.") {
        return http::resolve(key);
    }
    if key.starts_with("random.") {
        return random::resolve(key);
    }
    if key == "lorem" || key.starts_with("lorem.") {
        return lorem::resolve(key);
    }
    if key == "uuid" || key.starts_with("uuid.") {
        return uuid::resolve(key);
    }
    if clip::is_clip_key(key) {
        return clip::resolve(key);
    }

    None
}

pub fn strip_quotes(s: &str) -> Option<&str> {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let first = bytes[0];
        let last = bytes[s.len() - 1];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return Some(&s[1..s.len() - 1]);
        }
    }
    None
}

pub fn strip_argument_quotes(arg: &str) -> &str {
    let trimmed = arg.trim();
    strip_quotes(trimmed).unwrap_or(trimmed)
}

/// Performs final post-processing on the interpolated string.
///
/// All directives (`[key.*]`, `[delay.*]`, `[cursor]`) are resolved into
/// a unified `Vec<ExpansionStep>` sequence.
///
/// **Conflict rule**: `[cursor]` and `[key.*]` directives cannot coexist.
/// If any `[key.*]` directive is present, `[cursor]` is treated as literal text.
pub fn finalize(interpolated: &str, trigger: Option<&str>) -> FinalExpansion {
    let _ = validate_output(interpolated, trigger);

    let has_key_directives = contains_key_or_delay_directives(interpolated);

    // Unified pipeline: always split into steps.
    let mut steps = split_into_steps(interpolated);

    if has_key_directives {
        // [cursor] stays as literal text; just restore escaped cursor sentinels.
        restore_cursor_sentinels(&mut steps);
    } else {
        // Resolve [cursor] positioning (also restores escaped sentinels).
        apply_cursor_positioning(&mut steps);
    }

    FinalExpansion {
        steps,
        is_calculation: false,
        ai_transformer_template: None,
    }
}

fn append_unescaped_segment(segment: &str, output: &mut String) {
    let bytes = segment.as_bytes();
    let mut ptr = 0;

    while ptr < bytes.len() {
        if bytes[ptr] == b'\\' && ptr + 1 < bytes.len() {
            let next = bytes[ptr + 1];
            if next == TAG_OPEN
                || next == TAG_CLOSE
                || next == b'\\'
                || next == b'\''
                || next == b'"'
            {
                if segment[ptr..].starts_with(ESCAPED_CURSOR_LITERAL) {
                    output.push_str(ESCAPED_CURSOR_SENTINEL);
                    ptr += ESCAPED_CURSOR_LITERAL.len();
                    continue;
                }
                output.push(next as char);
                ptr += 2;
                continue;
            }
        }

        let Some(c) = segment[ptr..].chars().next() else {
            break;
        };
        output.push(c);
        ptr += c.len_utf8();
    }
}

fn parse_key_directive(inner: &str) -> Option<&str> {
    let rest = inner.strip_prefix("key(")?;
    let alias = rest.strip_suffix(')')?;
    Some(strip_argument_quotes(alias))
}

fn parse_delay_directive(inner: &str) -> Option<u64> {
    let rest = inner.strip_prefix("delay(")?;
    let delay_str = rest.strip_suffix(')')?;
    parse_delay_ms(strip_argument_quotes(delay_str))
}

fn parse_mouse_directive(inner: &str) -> Option<ExpansionStep> {
    if inner == "mouse.click" {
        Some(ExpansionStep::MouseClick)
    } else if inner == "mouse.rclick" {
        Some(ExpansionStep::MouseRClick)
    } else if inner == "mouse.mclick" {
        Some(ExpansionStep::MouseMClick)
    } else if inner == "mouse.hold" {
        Some(ExpansionStep::MouseHold)
    } else if inner == "mouse.release" {
        Some(ExpansionStep::MouseRelease)
    } else if let Some(rest) = inner.strip_prefix("mouse.move(") {
        let args = rest.strip_suffix(')')?;
        let parts: Vec<&str> = args.split(',').collect();
        if parts.len() == 2 {
            let x = strip_argument_quotes(parts[0]).parse().ok()?;
            let y = strip_argument_quotes(parts[1]).parse().ok()?;
            Some(ExpansionStep::MouseMove(x, y))
        } else {
            None
        }
    } else if let Some(rest) = inner.strip_prefix("mouse.scroll(") {
        let arg = rest.strip_suffix(')')?.trim();
        let delta = strip_argument_quotes(arg).parse().ok()?;
        Some(ExpansionStep::MouseScroll(delta))
    } else {
        None
    }
}

/// Checks whether the interpolated string contains any `[key.*]`, `[delay.*]`, or `[mouse.*]` directives.
fn contains_key_or_delay_directives(text: &str) -> bool {
    let mut ptr = 0;

    while let Some(tag) = find_next_tag(text, ptr) {
        let inner = tag_inner(text, tag);
        if parse_key_directive(inner).is_some()
            || parse_delay_directive(inner).is_some()
            || parse_mouse_directive(inner).is_some()
        {
            return true;
        }
        ptr = tag.end + 1;
    }

    false
}

/// Validates an expansion output for common mistakes like empty output, multiple cursors, or conflicts.
pub fn validate_output(output: &str, trigger: Option<&str>) -> crate::error::Result<()> {
    let trigger_ctx = trigger
        .map(|t| format!(" for trigger '{}'", t))
        .unwrap_or_default();

    if output.trim().is_empty() {
        return Err(crate::Error::Config(format!(
            "Output cannot be empty{}.",
            trigger_ctx
        )));
    }

    if output.len() > MAX_OUTPUT_LENGTH {
        return Err(crate::Error::Config(format!(
            "Output exceeds maximum length of {} characters{}.",
            MAX_OUTPUT_LENGTH, trigger_ctx,
        )));
    }

    let mut cursor_count = 0usize;
    let mut has_key_or_delay = false;

    let mut ptr = 0;
    while let Some(tag) = find_next_tag(output, ptr) {
        let inner = tag_inner(output, tag);

        if inner == "cursor" {
            cursor_count += 1;
        }
        if parse_key_directive(inner).is_some()
            || parse_delay_directive(inner).is_some()
            || parse_mouse_directive(inner).is_some()
        {
            has_key_or_delay = true;
        }
        let (key, default_value) = split_key_default(inner);
        if default_value.is_some() && is_reserved(trim_slice(key)) {
            tracing::warn!(
                "System variable [{}] cannot have a default value assignment and will be ignored{}.",
                trim_slice(key),
                trigger_ctx
            );
        }

        ptr = tag.end + 1;
    }

    // 1. Multi-cursor check
    if cursor_count > 1 {
        tracing::warn!(
            "Multiple [cursor] tags found in output{}. Only the first occurrence will define the final caret position.",
            trigger_ctx
        );
    }

    // 2. Conflict check: [cursor] vs [key.*]/[delay.*]
    if has_key_or_delay && cursor_count > 0 {
        tracing::warn!(
            "[cursor] directive will be ignored because [key.*] or [delay.*] directives are present{}. \
             Use [key.left] for precise navigation in multi-action snippets.",
            trigger_ctx
        );
    }

    Ok(())
}

/// Splits an interpolated string into a sequence of [`ExpansionStep`] actions.
///
/// Handles `[key.*]`, `[delay.*]` directives and escape sequences (`\[`, `\]`).
/// Text between directives becomes `ExpansionStep::Text`.
/// `[cursor]` is preserved as-is for the `apply_cursor_positioning` post-pass.
/// Escaped `\[cursor\]` is stored with a sentinel to avoid false matches.
const ESCAPED_CURSOR_SENTINEL: &str = "\x00ESC_CURSOR\x00";
fn split_into_steps(text: &str) -> Vec<ExpansionStep> {
    let mut steps: Vec<ExpansionStep> = Vec::new();
    let mut current_text = String::new();
    let mut ptr = 0;

    while let Some(tag) = find_next_tag(text, ptr) {
        append_unescaped_segment(&text[ptr..tag.start], &mut current_text);
        let inner = tag_inner(text, tag);

        let pipeline = transformers::split_pipeline(inner);
        let base_expr = pipeline[0];
        let transformers: Vec<String> = pipeline[1..].iter().map(|s| s.to_string()).collect();

        if base_expr.starts_with("exec.") {
            flush_text(&mut steps, &mut current_text);
            if !crate::settings::get_cached_scripts_enabled() {
                tracing::warn!(
                    "Blocked execution of [exec.*] block because scripts are disabled globally."
                );
                steps.push(ExpansionStep::Text(
                    "[Error: Script execution is disabled globally]".to_string(),
                ));
            } else {
                match exec::to_script_metadata(base_expr) {
                    Ok(metadata) => steps.push(ExpansionStep::InlineRun(metadata, transformers)),
                    Err(error) => steps.push(ExpansionStep::Text(format_run_error(error))),
                }
            }
        } else if let Some(alias) = parse_key_directive(inner) {
            flush_text(&mut steps, &mut current_text);
            steps.push(ExpansionStep::KeyPress(alias.to_lowercase()));
        } else if let Some(ms) = parse_delay_directive(inner) {
            flush_text(&mut steps, &mut current_text);
            steps.push(ExpansionStep::Delay(ms));
        } else if let Some(step) = parse_mouse_directive(inner) {
            flush_text(&mut steps, &mut current_text);
            steps.push(step);
        } else if let Some(step) = img::parse_img_directive(inner) {
            flush_text(&mut steps, &mut current_text);
            steps.push(step);
        } else {
            current_text.push_str(&text[tag.start..tag.end + 1]);
        }

        ptr = tag.end + 1;
    }

    append_unescaped_segment(&text[ptr..], &mut current_text);
    flush_text(&mut steps, &mut current_text);
    steps
}

/// Resolves `[cursor]` directives inside `Text` steps.
///
/// Finds the first `[cursor]`, removes all occurrences, and appends
/// `KeyPress("left")` steps to position the caret at the correct offset.
/// Escaped cursor sentinels are restored to literal `[cursor]` afterwards.
fn apply_cursor_positioning(steps: &mut Vec<ExpansionStep>) {
    // Concatenate all text content to compute cursor offset globally.
    let full_text: String = steps
        .iter()
        .filter_map(|s| match s {
            ExpansionStep::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();

    if let Some(first_idx) = full_text.find(CURSOR_TAG) {
        // Calculate left-arrow count from the first [cursor] position.
        let char_idx = full_text[..first_idx].chars().count();
        let clean_text = full_text.replace(CURSOR_TAG, "");
        let left_arrow_count = clean_text.chars().count() - char_idx;

        // Replace all Text steps with the cleaned text (merged into one).
        steps.retain(|s| !matches!(s, ExpansionStep::Text(_)));
        // Restore escaped cursor sentinels to literal [cursor].
        let final_text = clean_text.replace(ESCAPED_CURSOR_SENTINEL, CURSOR_TAG);
        if !final_text.is_empty() {
            steps.insert(0, ExpansionStep::Text(final_text));
        }

        // Append cursor positioning steps.
        for _ in 0..left_arrow_count {
            steps.push(ExpansionStep::KeyPress("left".to_string()));
        }
    } else {
        // No [cursor] directive â€” just restore any escaped cursor sentinels.
        restore_cursor_sentinels(steps);
    }
}

/// Replaces sentinel placeholders with literal `[cursor]` in all `Text` steps.
fn restore_cursor_sentinels(steps: &mut [ExpansionStep]) {
    for step in steps.iter_mut() {
        if let ExpansionStep::Text(t) = step
            && t.contains(ESCAPED_CURSOR_SENTINEL)
        {
            *t = t.replace(ESCAPED_CURSOR_SENTINEL, CURSOR_TAG);
        }
    }
}

/// Pushes accumulated text as a `Text` step and clears the buffer.
fn flush_text(steps: &mut Vec<ExpansionStep>, buf: &mut String) {
    if !buf.is_empty() {
        steps.push(ExpansionStep::Text(std::mem::take(buf)));
    }
}

fn format_run_error(error: String) -> String {
    if error.starts_with("[Error:") {
        error
    } else {
        format!("[Error: {error}]")
    }
}

/// Parses a delay string like `200ms` or `200` into a `u64` millisecond value.
fn parse_delay_ms(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(n) = s.strip_suffix("ms") {
        n.parse::<u64>().ok()
    } else if let Some(n) = s.strip_suffix('s') {
        n.parse::<f64>()
            .ok()
            .map(|seconds| (seconds * 1000.0) as u64)
    } else {
        s.parse::<u64>().ok()
    }
}

#[cfg(test)]
mod tests;
