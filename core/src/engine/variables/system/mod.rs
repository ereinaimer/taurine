//! System variables module.
//!
//! Centralizes logic for reserved keywords and system-wide markers like `[cursor]`,
//! and future variables like `[time.now]`.

pub mod clipboard;

pub mod date;
pub mod env;
pub mod file;
pub mod lorem;
pub mod mock;
pub mod net;
pub mod random;
pub mod run;
pub mod sys;
pub mod time;
pub mod transformers;
pub mod uuid;

use crate::engine::variables::types::{ExpansionStep, FinalExpansion};

const TAG_OPEN: u8 = b'[';
const TAG_CLOSE: u8 = b']';
const CURSOR_TAG: &str = "[cursor]";
const ESCAPED_CURSOR_LITERAL: &str = r#"\[cursor\]"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TagBounds {
    start: usize,
    end: usize,
}

/// Checks if a keyword is reserved by the system.
pub fn is_reserved(mut key: &str) -> bool {
    // 1. Strip all valid format modifiers to find the base variable
    while let Some((sub, _)) = split_modifier(key) {
        key = sub;
    }

    // 2. Check if the resulting base is a reserved system variable
    key == "cursor"
        || key == "uuid"
        || clipboard::is_clipboard_key(key)
        || key == "sys"
        || key == "lorem"
        || key == "mock"
        || key.starts_with("uuid.")
        || key.starts_with("sys.")
        || key.starts_with("time.")
        || key.starts_with("date.")
        || key.starts_with("env.")
        || key.starts_with("file.")
        || key.starts_with("net.")
        || key.starts_with("run.")
        || key.starts_with("random.")
        || key.starts_with("lorem.")
        || key.starts_with("mock.")
        || key == "key"
        || key.starts_with("key(")
        || key == "delay"
        || key.starts_with("delay(")
        || key.contains('.') // Reserve all other dot-namespaces
}

/// Splits a key into its base and a modifier suffix if it matches a known transformer.
/// Example: `time.now.upper` -> `Some(("time.now", "upper"))`
pub fn split_modifier(key: &str) -> Option<(&str, &str)> {
    transformers::split_suffix(key)
}

/// Checks if a keyword is a post-processing directive.
///
/// Directives are not replaced during interpolation but are instead handled
/// in the `finalize` phase (e.g., `[cursor]`, `[key(tab)]`, `[delay(200ms)]`).
pub fn is_directive(key: &str) -> bool {
    key == "cursor" || parse_key_directive(key).is_some() || parse_delay_directive(key).is_some()
}

/// Resolves a content-producing system variable.
pub fn resolve(key: &str) -> Option<String> {
    // 1. Handle Transformers first (Recursive)
    if let Some(resolved) = transformers::resolve(key, resolve) {
        return Some(resolved);
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
    if key.starts_with("file.") {
        return file::resolve(key);
    }
    if key.starts_with("net.") {
        return net::resolve(key);
    }
    if key.starts_with("sys.") {
        return sys::resolve(key);
    }
    if key.starts_with("random.") {
        return random::resolve(key);
    }
    if key == "lorem" || key.starts_with("lorem.") {
        return lorem::resolve(key);
    }
    if key.starts_with("mock.") {
        return mock::resolve(key);
    }
    if key == "uuid" || key.starts_with("uuid.") {
        return uuid::resolve(key);
    }
    if clipboard::is_clipboard_key(key) {
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
/// All directives (`[key.*]`, `[delay.*]`, `[cursor]`) are resolved into
/// a unified `Vec<ExpansionStep>` sequence.
///
/// **Conflict rule**: `[cursor]` and `[key.*]` directives cannot coexist.
/// If any `[key.*]` directive is present, `[cursor]` is treated as literal text.
pub fn finalize(interpolated: &str, trigger: Option<&str>) -> FinalExpansion {
    validate_output(interpolated, trigger);

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

fn split_key_default(inner: &str) -> (&str, Option<&str>) {
    let inner = trim_slice(inner);
    let bytes = inner.as_bytes();
    let mut depth = 0usize;
    let mut ptr = 0usize;

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

fn tag_inner(text: &str, tag: TagBounds) -> &str {
    trim_slice(&text[tag.start + 1..tag.end])
}

fn append_unescaped_segment(segment: &str, output: &mut String) {
    let bytes = segment.as_bytes();
    let mut ptr = 0;

    while ptr < bytes.len() {
        if bytes[ptr] == b'\\' && ptr + 1 < bytes.len() {
            let next = bytes[ptr + 1];
            if next == TAG_OPEN || next == TAG_CLOSE || next == b'\\' {
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

        let c = segment[ptr..].chars().next().unwrap();
        output.push(c);
        ptr += c.len_utf8();
    }
}

fn parse_key_directive(inner: &str) -> Option<&str> {
    let rest = inner.strip_prefix("key(")?;
    let alias = rest.strip_suffix(')')?;
    Some(alias)
}

fn parse_delay_directive(inner: &str) -> Option<u64> {
    let rest = inner.strip_prefix("delay(")?;
    let delay_str = rest.strip_suffix(')')?;
    parse_delay_ms(delay_str)
}

/// Checks whether the interpolated string contains any `[key.*]` or `[delay.*]` directives.
fn contains_key_or_delay_directives(text: &str) -> bool {
    let mut ptr = 0;

    while let Some(tag) = find_next_tag(text, ptr) {
        let inner = tag_inner(text, tag);
        if parse_key_directive(inner).is_some() || parse_delay_directive(inner).is_some() {
            return true;
        }
        ptr = tag.end + 1;
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

    let mut cursor_count = 0usize;
    let mut has_key_or_delay = false;

    let mut ptr = 0;
    while let Some(tag) = find_next_tag(output, ptr) {
        let inner = tag_inner(output, tag);

        if inner == "cursor" {
            cursor_count += 1;
        }
        if parse_key_directive(inner).is_some() || parse_delay_directive(inner).is_some() {
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

        if inner.starts_with("run.") {
            flush_text(&mut steps, &mut current_text);
            match run::to_script_metadata(inner) {
                Ok(metadata) => steps.push(ExpansionStep::InlineRun(metadata)),
                Err(error) => steps.push(ExpansionStep::Text(format_run_error(error))),
            }
        } else if let Some(alias) = parse_key_directive(inner) {
            flush_text(&mut steps, &mut current_text);
            steps.push(ExpansionStep::KeyPress(alias.to_lowercase()));
        } else if let Some(ms) = parse_delay_directive(inner) {
            flush_text(&mut steps, &mut current_text);
            steps.push(ExpansionStep::Delay(ms));
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

    if full_text.contains(CURSOR_TAG) {
        // Calculate left-arrow count from the first [cursor] position.
        let first_idx = full_text.find(CURSOR_TAG).unwrap();
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
        // No [cursor] directive — just restore any escaped cursor sentinels.
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
    } else {
        s.parse::<u64>().ok()
    }
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
        assert!(is_reserved("clipboard(1)"));
        assert!(is_reserved("clipboard.truncate(5)"));
        assert!(is_reserved("clipboard(2).upper"));
        assert!(is_reserved("uuid.v4"));
        assert!(is_reserved("time.now"));
        assert!(is_reserved("time.now.upper"));
        assert!(is_reserved("net.localip"));
        assert!(is_reserved("net.mac.upper"));
        assert!(is_reserved("run.bash(echo hi)"));
        assert!(is_reserved("run.bash(echo hi).upper"));
        assert!(is_reserved("random.int(1, 9)"));
        assert!(is_reserved("random.int(1, 9).upper"));
        assert!(is_reserved("lorem"));
        assert!(is_reserved("lorem.words(3)"));
        assert!(is_reserved("lorem.words(3).upper"));
        assert!(is_reserved("mock"));
        assert!(is_reserved("mock.email"));
        assert!(is_reserved("mock.password(12).upper"));
        assert!(is_reserved("sys"));
        assert!(is_reserved("sys.os"));
        assert!(is_reserved("sys.os.upper"));
        assert!(is_reserved("lowercase.hello"));

        // These are valid user variables and should not be reserved
        assert!(!is_reserved("username"));
        assert!(!is_reserved("name.upper"));
        assert!(!is_reserved("name.truncate(5)"));
        assert!(!is_reserved("my_var.shoutysnake"));
    }

    #[test]
    fn test_split_modifier_supports_parenthesized_transformers() {
        assert_eq!(
            split_modifier("clipboard.truncate(5)"),
            Some(("clipboard", "truncate(5)"))
        );
        assert_eq!(
            split_modifier("clipboard.replace(\",\", \";\").upper"),
            Some(("clipboard.replace(\",\", \";\")", "upper"))
        );
        assert_eq!(
            split_modifier("'a.b'.replace(\".\", \"-\")"),
            Some(("'a.b'", "replace(\".\", \"-\")"))
        );
    }

    #[test]
    fn test_is_directive() {
        assert!(is_directive("cursor"));
        assert!(is_directive("key(tab)"));
        assert!(is_directive("key(ctrl+a)"));
        assert!(is_directive("delay(200ms)"));
        assert!(is_directive("delay(200)"));
        assert!(!is_directive("key.tab"));
        assert!(!is_directive("delay.200ms"));
        assert!(!is_directive("time.now"));
    }

    #[test]
    fn test_resolve_random_int_interpolation() {
        assert_eq!(
            crate::engine::variables::interpolate::interpolate(
                "[random.int(5, 5)]",
                &crate::engine::variables::types::ArgMap::default()
            ),
            "5"
        );
    }

    #[test]
    fn test_resolve_lorem_words_interpolation_count() {
        let resolved = crate::engine::variables::interpolate::interpolate(
            "[lorem.words(3)]",
            &crate::engine::variables::types::ArgMap::default(),
        );

        assert_eq!(resolved.split_whitespace().count(), 3);
    }

    #[test]
    fn test_resolve_mock_email_interpolation() {
        let resolved = crate::engine::variables::interpolate::interpolate(
            "[mock.email]",
            &crate::engine::variables::types::ArgMap::default(),
        );

        assert!(resolved.contains('@'));
    }

    #[test]
    fn test_resolve_mock_password_interpolation() {
        let resolved = crate::engine::variables::interpolate::interpolate(
            "[mock.password(12)]",
            &crate::engine::variables::types::ArgMap::default(),
        );

        assert_eq!(resolved.chars().count(), 12);
    }

    #[test]
    fn test_finalize_cursor_positioning() {
        let res = finalize("hello [cursor] world", None);
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
        let res = finalize("name[key(tab)]email", None);
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
        let res = finalize("first[delay(200ms)]second[delay(100)]third", None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("first".to_string()),
                ExpansionStep::Delay(200),
                ExpansionStep::Text("second".to_string()),
                ExpansionStep::Delay(100),
                ExpansionStep::Text("third".to_string()),
            ]
        );
    }

    #[test]
    fn test_finalize_inline_run_splits_progressive_steps() {
        let res = finalize("Wait for it... [run.bash(echo Done!)]", None);

        assert_eq!(
            res.steps[0],
            ExpansionStep::Text("Wait for it... ".to_string())
        );
        match &res.steps[1] {
            ExpansionStep::InlineRun(metadata) => {
                assert_eq!(
                    crate::engine::shell::decompress(&metadata.compressed_content).unwrap(),
                    "echo Done!"
                );
            }
            other => panic!("expected InlineRun step, got {other:?}"),
        }
    }

    #[test]
    fn test_finalize_silent_inline_run_uses_silent_metadata() {
        let res = finalize("start[run.silent.bash(echo background)]end", None);

        assert_eq!(res.steps.len(), 3);
        match &res.steps[1] {
            ExpansionStep::InlineRun(metadata) => {
                assert_eq!(
                    metadata.behavior,
                    crate::engine::shell::ScriptBehavior::Silent
                );
            }
            other => panic!("expected InlineRun step, got {other:?}"),
        }
    }

    #[test]
    fn test_finalize_missing_run_file_emits_error_text() {
        let res = finalize("[run.bash.file(C:\\definitely\\missing.sh)]", None);

        assert_eq!(
            res.steps,
            vec![ExpansionStep::Text(
                "[Error: path to script not found!]".to_string()
            )]
        );
    }

    #[test]
    fn test_finalize_cursor_suppressed_when_key_directives_present() {
        let res = finalize("name[cursor][key(tab)]email", None);
        // [cursor] should be kept as literal text, not processed as a directive.
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("name[cursor]".to_string()),
                ExpansionStep::KeyPress("tab".to_string()),
                ExpansionStep::Text("email".to_string()),
            ]
        );
    }

    #[test]
    fn test_finalize_multiple_key_directives() {
        let res = finalize("a[key(tab)]b[key(enter)]c", None);
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
        let res = finalize("[key(TAB)]", None);
        assert_eq!(res.steps, vec![ExpansionStep::KeyPress("tab".to_string())]);
    }

    #[test]
    fn test_contains_key_or_delay_directives() {
        assert!(contains_key_or_delay_directives("hello [key(tab)] world"));
        assert!(contains_key_or_delay_directives("test [delay(100ms)]"));
        assert!(!contains_key_or_delay_directives("hello [cursor] world"));
        assert!(!contains_key_or_delay_directives("just plain text"));
        // Escaped should not count.
        assert!(!contains_key_or_delay_directives(r#"\[key(tab)\]"#));
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
    fn test_finalize_escaped_brackets_in_key_mode() {
        let res = finalize(r#"\[literal\][key(tab)]after"#, None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("[literal]".to_string()),
                ExpansionStep::KeyPress("tab".to_string()),
                ExpansionStep::Text("after".to_string()),
            ]
        );
    }

    #[test]
    fn test_finalize_modifier_combo_key() {
        let res = finalize("[key(ctrl+a)]", None);
        assert_eq!(
            res.steps,
            vec![ExpansionStep::KeyPress("ctrl+a".to_string())]
        );
    }

    #[test]
    fn test_finalize_multi_modifier_combo_case_normalized() {
        let res = finalize("[key(Ctrl+Shift+End)]", None);
        assert_eq!(
            res.steps,
            vec![ExpansionStep::KeyPress("ctrl+shift+end".to_string())]
        );
    }

    #[test]
    fn test_finalize_combo_between_text_segments() {
        let res = finalize("Name[key(tab)]Address[key(shift+tab)]Back", None);
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
        let res = finalize("[key(mod)][key(super)][key(ctrl)]", None);
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
        validate_output("[cursor] [cursor]", Some("multi"));
        validate_output("[key(tab)] [cursor]", Some("conflict"));
        validate_output("[cursor=invalid]", Some("default"));
        validate_output("[lorem.words([num=5])]", Some("nested"));
        validate_output(r#"\[cursor\] [cursor]"#, Some("escaped"));
        validate_output("[clipboard=invalid]", None);
    }

    #[test]
    fn test_split_key_default_respects_nested_placeholders() {
        assert_eq!(
            split_key_default("lorem.words([num=5])"),
            ("lorem.words([num=5])", None)
        );
        assert_eq!(
            split_key_default("cursor=invalid"),
            ("cursor", Some("invalid"))
        );
    }

    mod compatibility_finalize_tests {
        use super::*;

        #[test]
        fn delay_directives_also_suppress_cursor_positioning() {
            let res = finalize("start[cursor][delay(25ms)]end", None);

            assert_eq!(
                res.steps,
                vec![
                    ExpansionStep::Text("start[cursor]".to_string()),
                    ExpansionStep::Delay(25),
                    ExpansionStep::Text("end".to_string()),
                ]
            );
        }

        #[test]
        fn escaped_cursor_literal_stays_literal_when_key_directives_exist() {
            let res = finalize(r#"\[cursor\][key(tab)]after"#, None);

            assert_eq!(
                res.steps,
                vec![
                    ExpansionStep::Text("[cursor]".to_string()),
                    ExpansionStep::KeyPress("tab".to_string()),
                    ExpansionStep::Text("after".to_string()),
                ]
            );
        }

        #[test]
        fn first_cursor_location_wins_when_multiple_cursors_exist() {
            let res = finalize("[cursor]alpha[cursor]beta", None);

            assert_eq!(res.steps[0], ExpansionStep::Text("alphabeta".to_string()));
            assert_eq!(res.steps.len(), "alphabeta".chars().count() + 1);
            assert!(
                res.steps[1..]
                    .iter()
                    .all(|step| matches!(step, ExpansionStep::KeyPress(key) if key == "left"))
            );
        }

        #[test]
        fn key_and_delay_directives_preserve_current_execution_order() {
            let res = finalize("a[key(tab)]b[delay(10ms)]c[key(enter)]", None);

            assert_eq!(
                res.steps,
                vec![
                    ExpansionStep::Text("a".to_string()),
                    ExpansionStep::KeyPress("tab".to_string()),
                    ExpansionStep::Text("b".to_string()),
                    ExpansionStep::Delay(10),
                    ExpansionStep::Text("c".to_string()),
                    ExpansionStep::KeyPress("enter".to_string()),
                ]
            );
        }
    }
}
