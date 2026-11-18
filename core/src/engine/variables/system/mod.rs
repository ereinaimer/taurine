//! System variables module.
//!
//! Centralizes logic for reserved keywords and system-wide markers like `[cursor]`.

pub mod clipboard;
pub mod datetime;
pub mod env;
pub mod execute;
pub mod file;
pub mod image;
pub mod ip;
pub mod lorem;
pub mod random;
pub mod transformers;
pub mod uuid;

use super::tags::*;
use crate::engine::variables::types::{ExpansionOrigin, ExpansionStep, FinalExpansion};
use crate::keys::MouseButton;

const CURSOR_TAG: &str = "[cursor]";
const ESCAPED_CURSOR_LITERAL: &str = r#"\[cursor\]"#;
pub(crate) const MAX_OUTPUT_LENGTH: usize = 100_000;

/// Checks if a keyword is reserved by the system.
pub fn is_reserved(key: &str) -> bool {
    let Some((ns, _)) = crate::engine::variables::registry::parse_system_call(key) else {
        return false;
    };
    let root = ns.trim().to_ascii_lowercase();
    if !root.contains('.')
        && crate::engine::variables::registry::SYSTEM_ROOTS.contains(&root.as_str())
    {
        return true;
    }
    false
}

/// Checks if a keyword is a post-processing directive.
///
/// Directives are not replaced during interpolation but are instead handled
/// in the `finalize` phase (e.g., `[cursor]`, `[enter]`, `[tab 5]`, `[wait 200ms]`, `[run "cmd"]`).
pub fn is_directive(key: &str) -> bool {
    let key = key.trim();
    key == "cursor"
        || parse_key_action(key).is_some()
        || parse_wait_directive(key).is_some()
        || parse_mouse_directive(key).is_some()
        || key.starts_with("mouse")
        || key.starts_with("run")
        || key.starts_with("execute(")
}

/// Checks if a system keyword triggers deferred (async) evaluation.
///
/// Deferred variables are replaced with a special marker during interpolation
/// so the daemon can evaluate them in a non-blocking thread and show a braille spinner.
pub fn is_deferred(key: &str) -> bool {
    let trimmed = key.trim();
    if trimmed == "mouse pos" || trimmed == "mouse(pos)" {
        return true;
    }
    false
}

/// Resolves a content-producing system variable.
pub fn resolve(key: &str) -> Option<String> {
    let trimmed = key.trim();
    if trimmed == "newline" {
        return Some("\n".to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("newline") {
        let rest = rest.trim();
        if !rest.is_empty()
            && let Ok(n) = rest.parse::<usize>()
        {
            return Some("\n".repeat(n));
        }
    }
    if trimmed == "date" || trimmed.starts_with("date ") {
        let rest = trimmed.strip_prefix("date").unwrap_or("").trim();
        return datetime::resolve_date(rest);
    }
    if trimmed == "time" || trimmed.starts_with("time ") {
        let rest = trimmed.strip_prefix("time").unwrap_or("").trim();
        return datetime::resolve_time(rest);
    }
    if trimmed == "ip" {
        return ip::resolve("");
    }
    if trimmed == "calc" || trimmed.starts_with("calc ") {
        let rest = trimmed.strip_prefix("calc").unwrap_or("").trim();
        return crate::engine::math::evaluate(rest);
    }
    if trimmed == "env" || trimmed.starts_with("env ") || trimmed.starts_with("env(") {
        let rest = trimmed
            .strip_prefix("env(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or_else(|| trimmed.strip_prefix("env").unwrap_or("").trim());
        return env::resolve(rest);
    }
    if trimmed == "file" || trimmed.starts_with("file ") || trimmed.starts_with("file(") {
        let rest = trimmed
            .strip_prefix("file(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or_else(|| trimmed.strip_prefix("file").unwrap_or("").trim());
        return file::resolve(rest);
    }
    if trimmed == "random" || trimmed.starts_with("random ") || trimmed.starts_with("random(") {
        let rest = trimmed
            .strip_prefix("random(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or_else(|| trimmed.strip_prefix("random").unwrap_or("").trim());
        return random::resolve(rest);
    }
    if trimmed == "lorem" || trimmed.starts_with("lorem ") || trimmed.starts_with("lorem(") {
        let rest = trimmed
            .strip_prefix("lorem(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or_else(|| trimmed.strip_prefix("lorem").unwrap_or("").trim());
        return lorem::resolve(rest);
    }
    if trimmed == "uuid" || trimmed.starts_with("uuid ") || trimmed.starts_with("uuid(") {
        let rest = trimmed
            .strip_prefix("uuid(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or_else(|| trimmed.strip_prefix("uuid").unwrap_or("").trim());
        return uuid::resolve(rest);
    }
    if trimmed == "clip" || trimmed.starts_with("clip ") || trimmed.starts_with("clip(") {
        let rest = trimmed
            .strip_prefix("clip(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or_else(|| trimmed.strip_prefix("clip").unwrap_or("").trim());
        return clipboard::resolve(rest);
    }
    if trimmed == "chrono" || trimmed.starts_with("chrono(") {
        let inner = trimmed
            .strip_prefix("chrono(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or("");
        return datetime::resolve(inner);
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
/// All directives (`[key(...)]`, `[delay(...)]`, `[mouse(...)]`, `[cursor]`) are resolved into
/// a unified `Vec<ExpansionStep>` sequence.
///
/// **Conflict rule**: `[cursor]` and key, delay, or mouse directives cannot coexist.
/// If any such directive is present, `[cursor]` is treated as literal text.
pub fn finalize(interpolated: &str, trigger: Option<&str>) -> FinalExpansion {
    finalize_with_origin(interpolated, trigger, ExpansionOrigin::User)
}

/// Performs final post-processing on the interpolated string with explicit origin context.
pub fn finalize_with_origin(
    interpolated: &str,
    trigger: Option<&str>,
    origin: ExpansionOrigin,
) -> FinalExpansion {
    // Oversize output is logged, not blocked: large sources like file reads
    // (up to 5MB) legitimately exceed the advisory cap.
    if let Err(error) = validate_output(interpolated, trigger) {
        tracing::warn!("expansion output issue: {error}");
    }

    // Fast path: if there are no tags '[' and no escapes '\', return single text step immediately.
    if !interpolated.contains('[') && !interpolated.contains('\\') {
        return FinalExpansion {
            steps: if interpolated.is_empty() {
                Vec::new()
            } else {
                vec![ExpansionStep::Text(interpolated.to_string())]
            },
            is_calculation: false,
            ai_transformer_template: None,
        };
    }

    let has_key_directives = contains_key_or_delay_directives(interpolated);

    // Unified pipeline: always split into steps.
    let mut steps = split_into_steps_with_origin(interpolated, origin);

    if origin == ExpansionOrigin::Ai || has_key_directives {
        // [cursor] stays as literal text for AI expansions or when key directives exist; restore escaped sentinels.
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

pub fn is_valid_key_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split('+').collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    let Some(main) = parts.last() else {
        return false;
    };
    let main_valid =
        crate::keys::LogicalKey::from_alias(main).is_some_and(|k| k.is_mouse_button().is_none());
    let mods_valid = parts[..parts.len().saturating_sub(1)]
        .iter()
        .all(|m| crate::keys::Modifier::from_alias(m).is_some());
    main_valid && mods_valid
}

pub fn parse_key_action(inner: &str) -> Option<(String, usize)> {
    if let Some(alias) = parse_key_directive(inner) {
        return Some((alias.to_string(), 1));
    }

    let tokens = crate::engine::variables::parser::tokenize_words(inner);
    if tokens.len() == 1 {
        if tokens[0].chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if is_valid_key_name(&tokens[0]) {
            return Some((tokens[0].to_lowercase(), 1));
        }
    }
    if tokens.len() == 2
        && !tokens[0].chars().all(|c| c.is_ascii_digit())
        && is_valid_key_name(&tokens[0])
        && let Ok(count) = tokens[1].parse::<usize>()
        && (1..=10_000).contains(&count)
    {
        return Some((tokens[0].to_lowercase(), count));
    }

    None
}

pub fn parse_wait_directive(inner: &str) -> Option<u64> {
    if let Some(ms) = parse_delay_directive(inner) {
        return Some(ms);
    }

    let tokens = crate::engine::variables::parser::tokenize_words(inner);
    if !tokens.is_empty()
        && (tokens[0].eq_ignore_ascii_case("wait") || tokens[0].eq_ignore_ascii_case("delay"))
        && tokens.len() >= 2
    {
        let dur_str = &tokens[1];
        if let Some(ms) = parse_delay_ms(dur_str) {
            return Some(ms);
        }
        if let Ok(num) = dur_str.parse::<u64>() {
            return Some(num);
        }
    }

    None
}

pub(crate) fn parse_key_directive(inner: &str) -> Option<&str> {
    let rest = inner.strip_prefix("key(")?;
    let alias = rest.strip_suffix(')')?;
    Some(strip_argument_quotes(alias))
}

pub(crate) fn parse_delay_directive(inner: &str) -> Option<u64> {
    let rest = inner.strip_prefix("delay(")?;
    let delay_str = rest.strip_suffix(')')?;
    parse_delay_ms(strip_argument_quotes(delay_str))
}

/// Strict button parser for the mouse directive: `mN` only (`m1..=u8::MAX`,
/// `m0` rejected). Word aliases and bare numbers are None (save-time error).
/// Separate from the shared hotkey `from_alias`, which keeps its aliases.
pub(crate) fn parse_mouse_button(arg: &str) -> Option<MouseButton> {
    let n = arg.strip_prefix('m')?.parse::<u8>().ok()?;
    if n == 0 {
        return None;
    }
    Some(match n {
        1 => MouseButton::Left,
        2 => MouseButton::Right,
        3 => MouseButton::Middle,
        4 => MouseButton::Button4,
        5 => MouseButton::Button5,
        n => MouseButton::Other(n),
    })
}

pub(crate) fn parse_mouse_directive(inner: &str) -> Option<ExpansionStep> {
    use crate::engine::variables::parser::bind_call;

    // Check space-separated mouse directive: [mouse click m1], [mouse move 100 200], etc.
    let tokens = crate::engine::variables::parser::tokenize_words(inner);
    if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("mouse") {
        let action = tokens[1].to_ascii_lowercase();
        match action.as_str() {
            "click" => {
                let btn = tokens.get(2).and_then(|s| parse_mouse_button(s))?;
                let count = tokens
                    .get(3)
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(1);
                return Some(ExpansionStep::MouseClick(btn, count));
            }
            "hold" => {
                let btn = tokens.get(2).and_then(|s| parse_mouse_button(s))?;
                return Some(ExpansionStep::MouseDown(btn));
            }
            "release" => {
                let btn = tokens.get(2).and_then(|s| parse_mouse_button(s))?;
                return Some(ExpansionStep::MouseUp(btn));
            }
            "move" => {
                let x = tokens.get(2).and_then(|s| s.parse::<u16>().ok())?;
                let y = tokens.get(3).and_then(|s| s.parse::<u16>().ok())?;
                return Some(ExpansionStep::MouseMove(x, y));
            }
            "scroll" => {
                let delta = tokens.get(2).and_then(|s| s.parse::<i32>().ok())?;
                return Some(ExpansionStep::MouseScroll(delta));
            }
            "pos" => return None,
            _ => {}
        }
    }

    let raw = inner.strip_prefix("mouse(")?.strip_suffix(')')?;
    let spec = crate::engine::variables::registry::param_spec("mouse")?;
    let bound = bind_call("mouse", raw, &spec).ok()?;
    let action = bound.named.get("action").map(String::as_str).unwrap_or("");
    // Binder fills defaults into named, so an explicit count reads as
    // non-"1"; a bare extra positional only exists when explicitly passed.
    let count_raw = bound.named.get("count").map(String::as_str).unwrap_or("1");
    match action {
        "click" => {
            if bound.positional.len() > 3 {
                return None;
            }
            let btn = parse_mouse_button(bound.named.get("btn").map(String::as_str).unwrap_or(""))?;
            let count = count_raw.parse::<u32>().ok()?;
            Some(ExpansionStep::MouseClick(btn, count))
        }
        "hold" | "release" => {
            if bound.positional.len() > 2 || count_raw != "1" {
                return None;
            }
            let btn = parse_mouse_button(bound.named.get("btn").map(String::as_str).unwrap_or(""))?;
            Some(if action == "hold" {
                ExpansionStep::MouseDown(btn)
            } else {
                ExpansionStep::MouseUp(btn)
            })
        }
        "move" => {
            // X/Y ride the generic btn/count slots; y has no default so
            // a 2-positional call (or defaulted count) is an arity error.
            if bound.positional.len() > 3
                || (bound.positional.len() == 2 && count_raw == "1")
                || (bound.positional.is_empty() && count_raw == "1")
            {
                return None;
            }
            let x = bound
                .named
                .get("btn")
                .map(String::as_str)
                .unwrap_or("")
                .parse::<u16>()
                .ok()?;
            let y = count_raw.parse::<u16>().ok()?;
            Some(ExpansionStep::MouseMove(x, y))
        }
        "scroll" => {
            if bound.positional.len() > 2 || count_raw != "1" {
                return None;
            }
            // Signed delta, positive scrolls up / negative scrolls down.
            let delta = bound
                .named
                .get("btn")
                .map(String::as_str)
                .unwrap_or("")
                .parse::<i32>()
                .ok()?;
            Some(ExpansionStep::MouseScroll(delta))
        }
        // Pos is deferred content (sys marker), not a step; unknown
        // actions are save-time errors.
        _ => None,
    }
}

/// Checks whether the interpolated string contains any key, wait, mouse, or run directives.
fn contains_key_or_delay_directives(text: &str) -> bool {
    let mut ptr = 0;

    while let Some(tag) = find_next_tag(text, ptr) {
        let inner = tag_inner(text, tag);
        if parse_key_action(inner).is_some()
            || parse_wait_directive(inner).is_some()
            || parse_mouse_directive(inner).is_some()
            || inner.starts_with("run")
            || inner.starts_with("execute(")
        {
            return true;
        }
        ptr = tag.end + 1;
    }

    false
}

/// Validates an expansion output for common mistakes like empty output, multiple cursors, or conflicts.
/// Findings are advisory: callers log them and expand anyway, since large
/// sources (file reads up to 5MB) legitimately exceed the length cap.
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
        if parse_key_action(inner).is_some()
            || parse_wait_directive(inner).is_some()
            || parse_mouse_directive(inner).is_some()
            || inner.starts_with("run")
            || inner.starts_with("execute(")
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

    // 2. Conflict check: [cursor] vs key/delay/mouse directives
    if has_key_or_delay && cursor_count > 0 {
        tracing::warn!(
            "[cursor] directive will be ignored because [key(...)], [delay(...)] or [mouse(...)] directives are present{}. \
             Use [key(left)] for precise navigation in multi-action snippets.",
            trigger_ctx
        );
    }

    Ok(())
}

/// Splits an interpolated string into a sequence of [`ExpansionStep`] actions.
///
/// Handles `[key(...)]`, `[delay(...)]`, `[mouse(...)]` directives and escape sequences (`\[`, `\]`).
/// Text between directives becomes `ExpansionStep::Text`.
/// `[cursor]` is preserved as-is for the `apply_cursor_positioning` post-pass.
/// Escaped `\[cursor\]` is stored with a sentinel to avoid false matches.
const ESCAPED_CURSOR_SENTINEL: &str = "\x00ESC_CURSOR\x00";

fn split_into_steps_with_origin(text: &str, origin: ExpansionOrigin) -> Vec<ExpansionStep> {
    let mut steps: Vec<ExpansionStep> = Vec::new();
    let mut current_text = String::new();
    let mut ptr = 0;

    while let Some(tag) = find_next_tag(text, ptr) {
        append_unescaped_segment(&text[ptr..tag.start], &mut current_text);
        let inner = tag_inner(text, tag);

        if origin == ExpansionOrigin::Ai {
            current_text.push_str(&text[tag.start..tag.end + 1]);
        } else {
            let pipeline = transformers::split_pipeline(inner);
            let base_expr = pipeline[0];
            let transformers: Vec<String> = pipeline[1..].iter().map(|s| s.to_string()).collect();

            if base_expr.starts_with("run") || execute::strip_execute_args(base_expr).is_some() {
                flush_text(&mut steps, &mut current_text);
                match execute::to_script_metadata(base_expr) {
                    Ok(metadata) => steps.push(ExpansionStep::InlineRun(metadata, transformers)),
                    Err(error) => {
                        tracing::warn!("Failed to prepare exec script '{}': {}", base_expr, error);
                    }
                }
            } else if let Some((alias, count)) = parse_key_action(inner) {
                flush_text(&mut steps, &mut current_text);
                for _ in 0..count {
                    steps.push(ExpansionStep::KeyPress(alias.to_lowercase()));
                }
            } else if let Some(ms) = parse_wait_directive(inner) {
                flush_text(&mut steps, &mut current_text);
                steps.push(ExpansionStep::Delay(ms));
            } else if let Some(step) = parse_mouse_directive(inner) {
                flush_text(&mut steps, &mut current_text);
                steps.push(step);
            } else if let Some(step) = image::parse_img_directive(inner) {
                flush_text(&mut steps, &mut current_text);
                steps.push(step);
            } else {
                current_text.push_str(&text[tag.start..tag.end + 1]);
            }
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

        // Append cursor positioning steps, unless the run would be absurd:
        // past the output cap the caret stays at the end instead of building
        // one keypress step per character.
        if left_arrow_count > MAX_OUTPUT_LENGTH {
            tracing::warn!(
                "cursor navigation skipped: {left_arrow_count} steps exceeds maximum output length"
            );
        } else {
            for _ in 0..left_arrow_count {
                steps.push(ExpansionStep::KeyPress("left".to_string()));
            }
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

/// Parses a delay string like `200ms` or `200` into a `u64` millisecond value.
/// Negative and non-finite durations are rejected rather than saturated.
pub(crate) fn parse_delay_ms(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(n) = s.strip_suffix("ms") {
        n.parse::<u64>().ok()
    } else if let Some(n) = s.strip_suffix('s') {
        let seconds: f64 = n.parse().ok()?;
        if !seconds.is_finite() || seconds < 0.0 || seconds * 1000.0 > u64::MAX as f64 {
            return None;
        }
        Some((seconds * 1000.0) as u64)
    } else {
        s.parse::<u64>().ok()
    }
}

#[cfg(test)]
mod tests;
