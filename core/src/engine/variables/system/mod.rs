//! System variables module.
//!
//! Centralizes logic for reserved keywords and system-wide markers like `{cursor}`,
//! and future variables like `{time.now}`.

pub mod clipboard;

pub mod date;
pub mod env;
pub mod format;
pub mod time;
pub mod uuid;

use crate::engine::variables::types::{ExpansionStep, FinalExpansion};

/// Checks if a keyword is reserved by the system.
pub fn is_reserved(key: &str) -> bool {
    if split_transformer(key).is_some() {
        return true;
    }

    key == "cursor"
        || key == "uuid"
        || key == "clipboard"
        || key.starts_with("uuid.")
        || key.contains('.')
}

/// Splits a key into a transformer prefix and its content if it matches a known transformer.
/// Example: `upper.time.now` -> `Some(("upper", "time.now"))`
pub fn split_transformer(key: &str) -> Option<(&str, &str)> {
    if let Some((prefix, sub)) = key.split_once('.')
        && format::TRANSFORMERS.contains(&prefix)
    {
        return Some((prefix, sub));
    }
    None
}

/// Checks if a keyword is a post-processing directive.
///
/// Directives are not replaced during interpolation but are instead handled
/// in the `finalize` phase (e.g., `{cursor}`, `{key.tab}`, `{delay.200ms}`).
pub fn is_directive(key: &str) -> bool {
    key == "cursor" || key.starts_with("key.") || key.starts_with("delay.")
}

/// Resolves a content-producing system variable.
pub fn resolve(key: &str) -> Option<String> {
    // 1. Handle Transformers first (Recursive)
    if let Some((prefix, sub)) = split_transformer(key) {
        let content = if let Some(resolved) = resolve(sub) {
            resolved
        } else if let Some(unquoted) = strip_quotes(sub) {
            unquoted.to_string()
        } else {
            // Fallback: literal string (if no dot or further nesting, it might be a literal)
            // But we return None here so interpolate can try user variables first.
            return None;
        };
        return format::apply(prefix, &content);
    }

    // 2. Base System Variables
    if key.starts_with("time.") {
        return time::resolve(key);
    }
    if key.starts_with("date.") {
        return date::resolve(key);
    }
    if key.starts_with("env.") {
        return env::resolve(key);
    }
    if key == "uuid" || key.starts_with("uuid.") {
        return uuid::resolve(key);
    }
    if key == "clipboard" {
        return clipboard::resolve(key);
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

/// Performs final post-processing on the interpolated string.
///
/// All directives (`{key.*}`, `{delay.*}`, `{cursor}`) are resolved into
/// a unified `Vec<ExpansionStep>` sequence.
///
/// **Conflict rule**: `{cursor}` and `{key.*}` directives cannot coexist.
/// If any `{key.*}` directive is present, `{cursor}` is treated as literal text.
pub fn finalize(interpolated: &str, trigger: Option<&str>) -> FinalExpansion {
    validate_output(interpolated, trigger);

    let has_key_directives = contains_key_or_delay_directives(interpolated);

    // Unified pipeline: always split into steps.
    let mut steps = split_into_steps(interpolated);

    if has_key_directives {
        // {cursor} stays as literal text; just restore escaped cursor sentinels.
        restore_cursor_sentinels(&mut steps);
    } else {
        // Resolve {cursor} positioning (also restores escaped sentinels).
        apply_cursor_positioning(&mut steps);
    }

    FinalExpansion {
        steps,
        is_calculation: false,
    }
}

/// Checks whether the interpolated string contains any `{key.*}` or `{delay.*}` directives.
fn contains_key_or_delay_directives(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut ptr = 0;

    while ptr < bytes.len() {
        // Skip escaped braces.
        if bytes[ptr] == b'\\' && ptr + 1 < bytes.len() && bytes[ptr + 1] == b'{' {
            ptr += 2;
            continue;
        }

        if bytes[ptr] == b'{' {
            let start = ptr + 1;
            if let Some(close) = text[start..].find('}') {
                let inner = &text[start..start + close];
                if inner.starts_with("key.") || inner.starts_with("delay.") {
                    return true;
                }
                ptr = start + close + 1;
                continue;
            }
        }
        ptr += 1;
    }
    false
}

/// Validates an expansion output for common mistakes like multiple cursors or conflicts.
///
/// This serves as an early-warning system during automation creation (CLI)
/// and a failsafe during actual expansion.
pub fn validate_output(output: &str, trigger: Option<&str>) {
    let trigger_ctx = trigger
        .map(|t| format!(" for trigger '{}'", t))
        .unwrap_or_default();

    // 1. Multi-cursor check
    if output.matches("{cursor}").count() > 1 {
        tracing::warn!(
            "Multiple {{cursor}} tags found in output{}. Only the first occurrence will define the final caret position.",
            trigger_ctx
        );
    }

    // 2. Conflict check: {cursor} vs {key.*}/{delay.*}
    if contains_key_or_delay_directives(output) && output.contains("{cursor}") {
        tracing::warn!(
            "{{cursor}} directive will be ignored because {{key.*}} or {{delay.*}} directives are present{}. \
             Use {{key.left}} for precise navigation in multi-action snippets.",
            trigger_ctx
        );
    }

    // 3. Reserved variables with default values: {cursor=...}, {clipboard=...}, etc.
    let bytes = output.as_bytes();
    let mut ptr = 0;
    while ptr < bytes.len() {
        if bytes[ptr] == b'\\'
            && ptr + 1 < bytes.len()
            && (bytes[ptr + 1] == b'{' || bytes[ptr + 1] == b'}')
        {
            ptr += 2;
            continue;
        }

        if bytes[ptr] == b'{' {
            let start = ptr + 1;
            if let Some(close) = output[start..].find('}') {
                let inner = &output[start..start + close];
                if let Some((key, _)) = inner.split_once('=')
                    && is_reserved(key)
                {
                    tracing::warn!(
                        "System variable {{{}}} cannot have a default value assignment and will be ignored{}.",
                        key,
                        trigger_ctx
                    );
                }
                ptr = start + close + 1;
                continue;
            }
        }
        ptr += 1;
    }
}

/// Splits an interpolated string into a sequence of [`ExpansionStep`] actions.
///
/// Handles `{key.*}`, `{delay.*}` directives and escape sequences (`\{`, `\}`).
/// Text between directives becomes `ExpansionStep::Text`.
/// `{cursor}` is preserved as-is for the `apply_cursor_positioning` post-pass.
/// Escaped `\{cursor\}` is stored with a sentinel to avoid false matches.
const ESCAPED_CURSOR_SENTINEL: &str = "\x00ESC_CURSOR\x00";
fn split_into_steps(text: &str) -> Vec<ExpansionStep> {
    let mut steps: Vec<ExpansionStep> = Vec::new();
    let mut current_text = String::new();
    let bytes = text.as_bytes();
    let mut ptr = 0;

    while ptr < bytes.len() {
        // Handle escaped braces: \{ or \}
        if bytes[ptr] == b'\\' && ptr + 1 < bytes.len() {
            let next = bytes[ptr + 1];
            if next == b'{' || next == b'}' {
                // Escaped cursor tag: use sentinel so apply_cursor_positioning
                // won't mistake it for a real {cursor} directive.
                if text[ptr..].starts_with(r#"\{cursor\}"#) {
                    current_text.push_str(ESCAPED_CURSOR_SENTINEL);
                    ptr += 10; // length of \{cursor\}
                    continue;
                }
                current_text.push(next as char);
                ptr += 2;
                continue;
            }
        }

        if bytes[ptr] == b'{' {
            let start = ptr + 1;
            if let Some(close) = text[start..].find('}') {
                let inner = &text[start..start + close];

                if let Some(alias) = inner.strip_prefix("key.") {
                    flush_text(&mut steps, &mut current_text);
                    steps.push(ExpansionStep::KeyPress(alias.to_lowercase()));
                    ptr = start + close + 1;
                    continue;
                }

                if let Some(delay_str) = inner.strip_prefix("delay.")
                    && let Some(ms) = parse_delay_ms(delay_str)
                {
                    flush_text(&mut steps, &mut current_text);
                    steps.push(ExpansionStep::Delay(ms));
                    ptr = start + close + 1;
                    continue;
                    // Invalid delay format — treat as literal text.
                }

                // Not a key/delay directive — treat the whole `{...}` as literal
                // (including `{cursor}`, which is resolved in the post-pass).
            }
        }

        current_text.push(text[ptr..].chars().next().unwrap());
        ptr += text[ptr..].chars().next().unwrap().len_utf8();
    }

    flush_text(&mut steps, &mut current_text);
    steps
}

/// Resolves `{cursor}` directives inside `Text` steps.
///
/// Finds the first `{cursor}`, removes all occurrences, and appends
/// `KeyPress("left")` steps to position the caret at the correct offset.
/// Escaped cursor sentinels are restored to literal `{cursor}` afterwards.
fn apply_cursor_positioning(steps: &mut Vec<ExpansionStep>) {
    // Concatenate all text content to compute cursor offset globally.
    let full_text: String = steps
        .iter()
        .filter_map(|s| match s {
            ExpansionStep::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();

    if full_text.contains("{cursor}") {
        // Calculate left-arrow count from the first {cursor} position.
        let first_idx = full_text.find("{cursor}").unwrap();
        let char_idx = full_text[..first_idx].chars().count();
        let clean_text = full_text.replace("{cursor}", "");
        let left_arrow_count = clean_text.chars().count() - char_idx;

        // Replace all Text steps with the cleaned text (merged into one).
        steps.retain(|s| !matches!(s, ExpansionStep::Text(_)));
        // Restore escaped cursor sentinels to literal {cursor}.
        let final_text = clean_text.replace(ESCAPED_CURSOR_SENTINEL, "{cursor}");
        if !final_text.is_empty() {
            steps.insert(0, ExpansionStep::Text(final_text));
        }

        // Append cursor positioning steps.
        for _ in 0..left_arrow_count {
            steps.push(ExpansionStep::KeyPress("left".to_string()));
        }
    } else {
        // No {cursor} directive — just restore any escaped cursor sentinels.
        restore_cursor_sentinels(steps);
    }
}

/// Replaces sentinel placeholders with literal `{cursor}` in all `Text` steps.
fn restore_cursor_sentinels(steps: &mut [ExpansionStep]) {
    for step in steps.iter_mut() {
        if let ExpansionStep::Text(t) = step
            && t.contains(ESCAPED_CURSOR_SENTINEL)
        {
            *t = t.replace(ESCAPED_CURSOR_SENTINEL, "{cursor}");
        }
    }
}

/// Pushes accumulated text as a `Text` step and clears the buffer.
fn flush_text(steps: &mut Vec<ExpansionStep>, buf: &mut String) {
    if !buf.is_empty() {
        steps.push(ExpansionStep::Text(std::mem::take(buf)));
    }
}

/// Parses a delay string like `200ms` into a `u64` millisecond value.
fn parse_delay_ms(s: &str) -> Option<u64> {
    let s = s.trim();
    s.strip_suffix("ms").and_then(|n| n.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::variables::types::ExpansionStep;

    #[test]
    fn test_is_reserved() {
        assert!(is_reserved("cursor"));
        assert!(is_reserved("uuid"));
        assert!(is_reserved("clipboard"));
        assert!(is_reserved("uuid.v4"));
        assert!(is_reserved("time.now"));
        assert!(is_reserved("lowercase.hello"));
        assert!(!is_reserved("username"));
    }

    #[test]
    fn test_is_directive() {
        assert!(is_directive("cursor"));
        assert!(is_directive("key.tab"));
        assert!(is_directive("key.ctrl+a"));
        assert!(is_directive("delay.200ms"));
        assert!(!is_directive("time.now"));
    }

    #[test]
    fn test_finalize_cursor_positioning() {
        let res = finalize("hello {cursor} world", None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("hello  world".to_string()),
                // 6 left arrows to position cursor after "hello "
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
            ]
        );
        assert!(!res.is_calculation);
    }

    #[test]
    fn test_finalize_text_only() {
        let res = finalize("hello world", None);
        assert_eq!(
            res.steps,
            vec![ExpansionStep::Text("hello world".to_string())]
        );
    }

    #[test]
    fn test_finalize_key_directive_splits_into_steps() {
        let res = finalize("name{key.tab}email", None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("name".to_string()),
                ExpansionStep::KeyPress("tab".to_string()),
                ExpansionStep::Text("email".to_string()),
            ]
        );
    }

    #[test]
    fn test_finalize_delay_directive() {
        let res = finalize("first{delay.200ms}second", None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("first".to_string()),
                ExpansionStep::Delay(200),
                ExpansionStep::Text("second".to_string()),
            ]
        );
    }

    #[test]
    fn test_finalize_cursor_suppressed_when_key_directives_present() {
        let res = finalize("name{cursor}{key.tab}email", None);
        // {cursor} should be kept as literal text, not processed as a directive.
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("name{cursor}".to_string()),
                ExpansionStep::KeyPress("tab".to_string()),
                ExpansionStep::Text("email".to_string()),
            ]
        );
    }

    #[test]
    fn test_finalize_multiple_key_directives() {
        let res = finalize("a{key.tab}b{key.enter}c", None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("a".to_string()),
                ExpansionStep::KeyPress("tab".to_string()),
                ExpansionStep::Text("b".to_string()),
                ExpansionStep::KeyPress("enter".to_string()),
                ExpansionStep::Text("c".to_string()),
            ]
        );
    }

    #[test]
    fn test_finalize_key_alias_case_insensitive() {
        let res = finalize("{key.TAB}", None);
        assert_eq!(res.steps, vec![ExpansionStep::KeyPress("tab".to_string())]);
    }

    #[test]
    fn test_contains_key_or_delay_directives() {
        assert!(contains_key_or_delay_directives("hello {key.tab} world"));
        assert!(contains_key_or_delay_directives("test {delay.100ms}"));
        assert!(!contains_key_or_delay_directives("hello {cursor} world"));
        assert!(!contains_key_or_delay_directives("just plain text"));
        // Escaped should not count.
        assert!(!contains_key_or_delay_directives(r#"\{key.tab}"#));
    }

    #[test]
    fn test_parse_delay_ms() {
        assert_eq!(parse_delay_ms("200ms"), Some(200));
        assert_eq!(parse_delay_ms("0ms"), Some(0));
        assert_eq!(parse_delay_ms("1000ms"), Some(1000));
        assert_eq!(parse_delay_ms("invalid"), None);
        assert_eq!(parse_delay_ms("200s"), None);
        assert_eq!(parse_delay_ms("ms"), None);
    }

    #[test]
    fn test_finalize_escaped_braces_in_key_mode() {
        let res = finalize(r#"\{literal\}{key.tab}after"#, None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("{literal}".to_string()),
                ExpansionStep::KeyPress("tab".to_string()),
                ExpansionStep::Text("after".to_string()),
            ]
        );
    }

    #[test]
    fn test_finalize_modifier_combo_key() {
        let res = finalize("{key.ctrl+a}", None);
        assert_eq!(
            res.steps,
            vec![ExpansionStep::KeyPress("ctrl+a".to_string())]
        );
    }

    #[test]
    fn test_finalize_multi_modifier_combo_case_normalized() {
        let res = finalize("{key.Ctrl+Shift+End}", None);
        assert_eq!(
            res.steps,
            vec![ExpansionStep::KeyPress("ctrl+shift+end".to_string())]
        );
    }

    #[test]
    fn test_finalize_combo_between_text_segments() {
        let res = finalize("Name{key.tab}Address{key.shift+tab}Back", None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("Name".to_string()),
                ExpansionStep::KeyPress("tab".to_string()),
                ExpansionStep::Text("Address".to_string()),
                ExpansionStep::KeyPress("shift+tab".to_string()),
                ExpansionStep::Text("Back".to_string()),
            ]
        );
    }

    #[test]
    fn test_finalize_standalone_modifier_directives() {
        let res = finalize("{key.mod}{key.super}{key.ctrl}", None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::KeyPress("mod".to_string()),
                ExpansionStep::KeyPress("super".to_string()),
                ExpansionStep::KeyPress("ctrl".to_string()),
            ]
        );
    }

    #[test]
    fn test_validate_output_logic_paths() {
        // These calls shouldn't panic. We are primarily testing the path coverage.
        validate_output("valid", None);
        validate_output("{cursor} {cursor}", Some("multi"));
        validate_output("{key.tab} {cursor}", Some("conflict"));
        validate_output("{cursor=invalid}", Some("default"));
        validate_output(r#"\{cursor\} {cursor}"#, Some("escaped"));
        validate_output("{clipboard=invalid}", None);
    }
}
