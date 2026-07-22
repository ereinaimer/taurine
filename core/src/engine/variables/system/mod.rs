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
pub mod mock;
pub mod net;
pub mod random;
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
pub fn is_reserved(key: &str) -> bool {
    key == "cursor"
        || key == "uuid"
        || clip::is_clip_key(key)
        || key == "lorem"
        || key == "mock"
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
        || key.starts_with("mock.")
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
    if key.starts_with("mock.") {
        return mock::resolve(key);
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
    let mut paren_depth = 0usize;
    let mut ptr = 0usize;

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

        let c = segment[ptr..].chars().next().unwrap();
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
mod tests {
    use super::*;
    use crate::engine::variables::types::ExpansionStep;

    #[test]
    fn test_is_reserved() {
        assert!(is_reserved("cursor"));
        assert!(is_reserved("uuid"));
        assert!(is_reserved("clip"));
        assert!(is_reserved("clip(1)"));
        assert!(is_reserved("clip.truncate(5)"));
        assert!(is_reserved("clip(2).upper"));
        assert!(is_reserved("uuid.v4"));
        assert!(is_reserved("time"));
        assert!(is_reserved("time.utc"));
        assert!(is_reserved("net.localip"));
        assert!(is_reserved("net.localip"));
        assert!(is_reserved("exec.bash(echo hi)"));
        assert!(is_reserved("random.int(1, 9)"));
        assert!(is_reserved("lorem"));
        assert!(is_reserved("lorem.word(3)"));
        assert!(is_reserved("mock"));
        assert!(is_reserved("mock.email"));
        assert!(is_reserved("mock.company"));

        // These are valid user variables and should not be reserved
        assert!(!is_reserved("username"));
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
        assert!(!is_directive("time.utc"));
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
    fn test_resolve_lorem_word_interpolation_count() {
        let resolved = crate::engine::variables::interpolate::interpolate(
            "[lorem.word(3)]",
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
        let res = finalize("Wait for it... [exec.bash(echo Done!)]", None);

        assert_eq!(
            res.steps[0],
            ExpansionStep::Text("Wait for it... ".to_string())
        );
        match &res.steps[1] {
            ExpansionStep::InlineRun(metadata, transformers) => {
                assert_eq!(
                    crate::engine::shell::decompress(&metadata.compressed_content).unwrap(),
                    "echo Done!"
                );
                assert!(transformers.is_empty());
            }
            other => panic!("expected InlineRun step, got {other:?}"),
        }
    }

    #[test]
    fn test_finalize_silent_inline_run_uses_silent_metadata() {
        let res = finalize("start[exec.silent.bash(echo background)]end", None);

        assert_eq!(res.steps.len(), 3);
        match &res.steps[1] {
            ExpansionStep::InlineRun(metadata, transformers) => {
                assert_eq!(
                    metadata.behavior,
                    crate::engine::shell::ScriptBehavior::Silent
                );
                assert!(transformers.is_empty());
            }
            other => panic!("expected InlineRun step, got {other:?}"),
        }
    }

    #[test]
    fn test_finalize_inline_run_with_transformers() {
        let res = finalize("[exec.bash(echo done) | upper | trim]", None);
        assert_eq!(res.steps.len(), 1);
        match &res.steps[0] {
            ExpansionStep::InlineRun(metadata, transformers) => {
                assert_eq!(
                    crate::engine::shell::decompress(&metadata.compressed_content).unwrap(),
                    "echo done"
                );
                assert_eq!(transformers, &vec!["upper".to_string(), "trim".to_string()]);
            }
            other => panic!("expected InlineRun step, got {other:?}"),
        }
    }

    #[test]
    fn test_finalize_missing_run_file_emits_error_text() {
        let res = finalize("[exec.bash.file(C:\\definitely\\missing.sh)]", None);

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
    fn test_finalize_key_alias_strips_quotes() {
        let res = finalize("[key(\"tab\")]", None);
        assert_eq!(res.steps, vec![ExpansionStep::KeyPress("tab".to_string())]);
        let res2 = finalize("[key('enter')]", None);
        assert_eq!(
            res2.steps,
            vec![ExpansionStep::KeyPress("enter".to_string())]
        );
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
        assert_eq!(parse_delay_ms("ms"), None);
        // New test cases for seconds:
        assert_eq!(parse_delay_ms("1s"), Some(1000));
        assert_eq!(parse_delay_ms("1.5s"), Some(1500));
        assert_eq!(parse_delay_ms("0.5s"), Some(500));
        assert_eq!(parse_delay_ms("0s"), Some(0));
        assert_eq!(parse_delay_ms("60s"), Some(60000));
    }

    #[test]
    fn test_append_unescaped_segment_control_chars() {
        let mut out = String::new();
        append_unescaped_segment("hello\\nworld\\tgoodbye\\r!", &mut out);
        assert_eq!(out, "hello\\nworld\\tgoodbye\\r!");
    }

    #[test]
    fn test_finalize_with_control_char_escapes() {
        let res = finalize("first\\nsecond\\tthird", None);
        assert_eq!(
            res.steps,
            vec![ExpansionStep::Text("first\\nsecond\\tthird".to_string())]
        );
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
        validate_output("valid", None).unwrap();
        validate_output("[cursor] [cursor]", Some("multi")).unwrap();
        validate_output("[key(tab)] [cursor]", Some("conflict")).unwrap();
        validate_output("[cursor=invalid]", Some("default")).unwrap();
        validate_output("[lorem.word([num=5])]", Some("nested")).unwrap();
        validate_output(r#"\[cursor\] [cursor]"#, Some("escaped")).unwrap();
        validate_output("[clip=invalid]", None).unwrap();
    }

    #[test]
    fn test_validate_output_rejects_empty() {
        assert!(validate_output("", None).is_err());
        assert!(validate_output("   ", None).is_err());
        assert!(validate_output("\t\n", None).is_err());
    }

    #[test]
    fn test_split_key_default_respects_nested_placeholders() {
        assert_eq!(
            split_key_default("lorem.word([num=5])"),
            ("lorem.word([num=5])", None)
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

        #[test]
        fn escaped_directives_are_unescaped_to_text() {
            let res = finalize(r#"\[key(enter)\]"#, None);
            assert_eq!(
                res.steps,
                vec![ExpansionStep::Text("[key(enter)]".to_string())]
            );
        }

        #[test]
        fn escaped_quotes_are_unescaped() {
            let res = finalize(r#"echo \'hello\' | grep \"hello\""#, None);
            assert_eq!(
                res.steps,
                vec![ExpansionStep::Text(
                    r#"echo 'hello' | grep "hello""#.to_string()
                )]
            );
        }

        fn evaluate_template(
            text: &str,
            args: Option<&crate::engine::variables::types::ArgMap>,
        ) -> FinalExpansion {
            let interpolated = crate::engine::variables::interpolate::interpolate(
                text,
                args.unwrap_or(&crate::engine::variables::types::ArgMap::default()),
            );
            finalize(&interpolated, None)
        }

        #[test]
        fn test_template_syntax_spec_compliance_evaluation() {
            let _guard = crate::testing::TEST_LOCK.lock().unwrap();
            let (_dir, _conn) = crate::testing::open_test_db();
            unsafe {
                std::env::set_var(
                    "TAURINE_DB_PATH",
                    _dir.path().join("test_taurine.db").to_str().unwrap(),
                );
            }

            // Test Case 1: testvars
            {
                let mut args = crate::engine::variables::types::ArgMap::default();
                args.positional.push("Bob".to_string());
                args.positional.push("New York".to_string());
                args.named.insert("role".to_string(), "Manager".to_string());
                let res = evaluate_template(
                    "Hello [0=friend]! You live in [1='San Francisco'] and work as [role='Software Engineer'].",
                    Some(&args),
                );
                assert_eq!(
                    res.steps,
                    vec![ExpansionStep::Text(
                        "Hello Bob! You live in New York and work as Manager.".to_string()
                    )]
                );
            }

            // Test Case 2: testescape
            {
                let mut args = crate::engine::variables::types::ArgMap::default();
                args.positional.push("custom".to_string());
                let res = evaluate_template(
                    "Escaped brackets: \\[0=ignored\\] | Literal pipe: [0='default value' \\| upper] | Parsed pipe: [0='hello' | upper]",
                    Some(&args),
                );
                assert_eq!(res.steps, vec![ExpansionStep::Text("Escaped brackets: [0=ignored] | Literal pipe: custom | Parsed pipe: CUSTOM".to_string())]);

                let res_no_args = evaluate_template(
                    "Escaped brackets: \\[0=ignored\\] | Literal pipe: [0='default value' \\| upper] | Parsed pipe: [0='hello' | upper]",
                    None,
                );
                assert_eq!(res_no_args.steps, vec![ExpansionStep::Text("Escaped brackets: [0=ignored] | Literal pipe: 'default value' | upper | Parsed pipe: 'DEFAULT VALUE' | UPPER".to_string())]);
            }

            // Test Case 3: testdatetime
            {
                let res = evaluate_template(
                    "Local: [date] [time] | UTC +1w: [date.utc.calc(+1w).format('Today is' dddd, MMMM D, YYYY)] | UTC Time -2h: [time.utc.calc(-2h).format(hh:mm A)] | Cased AM/PM: [time.format(A) | lower]",
                    None,
                );
                assert_eq!(res.steps.len(), 1);
                if let ExpansionStep::Text(ref text) = res.steps[0] {
                    assert!(text.contains("Local: "));
                    assert!(text.contains("UTC +1w: Today is "));
                    assert!(text.contains("UTC Time -2h: "));
                    assert!(text.contains("Cased AM/PM: "));
                } else {
                    panic!("Expected Text step");
                }
            }

            // Test Case 4: testenv
            {
                unsafe {
                    std::env::set_var("USERNAME", "aimer");
                    std::env::set_var("USERPROFILE", "c:\\users\\aimer");
                }
                let res = evaluate_template(
                    "User (Title Case): [env(USERNAME) | title] | Home Path (Lowercase): [env(USERPROFILE) | lower]",
                    None,
                );
                assert_eq!(
                    res.steps,
                    vec![ExpansionStep::Text(
                        "User (Title Case): Aimer | Home Path (Lowercase): c:\\users\\aimer"
                            .to_string()
                    )]
                );
            }

            // Test Case 5: testfile
            {
                if let Some(home) = directories::UserDirs::new().map(|d| d.home_dir().to_path_buf())
                {
                    let path = home.join("taurine_test.txt");
                    std::fs::write(&path, "line one\nline two\nline three").ok();
                    let res = evaluate_template(
                        "Full Content: [file.read(~/taurine_test.txt) | trim] | Line 2: [file.read_line(~/taurine_test.txt, 2) | upper] | Lines 1-3: [file.read_line(~/taurine_test.txt, 1, 3)]",
                        None,
                    );
                    std::fs::remove_file(&path).ok();

                    assert_eq!(res.steps.len(), 1);
                    if let ExpansionStep::Text(ref text) = res.steps[0] {
                        let normalized = text.replace("\r\n", "\n");
                        assert_eq!(
                            normalized,
                            "Full Content: line one\nline two\nline three | Line 2: LINE TWO | Lines 1-3: line one\nline two\nline three"
                        );
                    } else {
                        panic!("Expected Text step");
                    }
                }
            }

            // Test Case 6: testclip
            {
                super::clip::set_mock_clip_history(vec![
                    "  apple pie  ".to_string(),
                    "banana".to_string(),
                ]);
                let res = evaluate_template(
                    "Latest (Slugified): [clip | slug] | Second: [clip(0) | trim] | Third (Upper): [clip(1) | upper] | Empty index: [clip(2) | squote]",
                    None,
                );
                super::clip::set_mock_clip(None);

                assert_eq!(
                    res.steps,
                    vec![ExpansionStep::Text("Latest (Slugified): apple-pie | Second: apple pie | Third (Upper): BANANA | Empty index: ''".to_string())]
                );
            }

            // Test Case 7: testexec
            {
                let res = evaluate_template(
                    "Cwd Path: [exec.powershell((Get-Location).Path) | trim] | Cmd Command: [exec.cmd(echo hello from cmd) | upper] | Silent Task: [exec.silent.powershell(echo 'background task')]",
                    None,
                );
                assert_eq!(res.steps.len(), 6);
                assert_eq!(res.steps[0], ExpansionStep::Text("Cwd Path: ".to_string()));
                if let ExpansionStep::InlineRun(ref m, ref t) = res.steps[1] {
                    assert_eq!(t, &vec!["trim".to_string()]);
                    assert_eq!(m.behavior, crate::engine::shell::ScriptBehavior::Inline);
                } else {
                    panic!("Expected InlineRun");
                }
                assert_eq!(
                    res.steps[2],
                    ExpansionStep::Text(" | Cmd Command: ".to_string())
                );
                if let ExpansionStep::InlineRun(ref m, ref t) = res.steps[3] {
                    assert_eq!(t, &vec!["upper".to_string()]);
                    assert_eq!(m.behavior, crate::engine::shell::ScriptBehavior::Inline);
                } else {
                    panic!("Expected InlineRun");
                }
                assert_eq!(
                    res.steps[4],
                    ExpansionStep::Text(" | Silent Task: ".to_string())
                );
                if let ExpansionStep::InlineRun(ref m, ref t) = res.steps[5] {
                    assert!(t.is_empty());
                    assert_eq!(m.behavior, crate::engine::shell::ScriptBehavior::Silent);
                } else {
                    panic!("Expected InlineRun");
                }
            }

            // Test Case 8: testhttp
            {
                let res = evaluate_template(
                    "Status: [http.status(https://httpbin.org/status/200)] | UA: [http.get(https://httpbin.org/headers) | json.get('headers.User-Agent') | truncate(15)]",
                    None,
                );
                assert_eq!(res.steps.len(), 1);
                if let ExpansionStep::Text(ref text) = res.steps[0] {
                    assert_eq!(
                        text,
                        "Status: \x03\x1Fsys:http.status(https://httpbin.org/status/200)\x04 | UA: \x03\x1Fsys:http.get(https://httpbin.org/headers) | json.get('headers.User-Agent') | truncate(15)\x04"
                    );
                } else {
                    panic!("Expected Text step");
                }
            }

            // Test Case 9: testrandom
            {
                let res = evaluate_template(
                    "Int (10-50): [random.int(10, 50)] | Pass (12): [random.pass(12)] | Choice: [random.choice(apple, banana, cherry) | title] | Lorem (Dynamic Count): [lorem.word([random.int(2, 4)]) | kebab]",
                    None,
                );
                assert_eq!(res.steps.len(), 1);
                if let ExpansionStep::Text(ref text) = res.steps[0] {
                    assert!(text.contains("Int (10-50): "));
                    assert!(text.contains(" | Pass (12): "));
                    assert!(text.contains(" | Choice: "));
                    assert!(text.contains(" | Lorem (Dynamic Count): "));
                } else {
                    panic!("Expected Text step");
                }
            }

            // Test Case 10: testmock
            {
                let res = evaluate_template(
                    "Name: [mock.name | upper] | Email: [mock.email] | Address: [mock.address | title] | Job Title: [mock.job_title | kebab]",
                    None,
                );
                assert_eq!(res.steps.len(), 1);
                if let ExpansionStep::Text(ref text) = res.steps[0] {
                    assert!(text.contains("Name: "));
                    assert!(text.contains(" | Email: "));
                    assert!(text.contains(" | Address: "));
                    assert!(text.contains(" | Job Title: "));
                } else {
                    panic!("Expected Text step");
                }
            }

            // Test Case 11: testnested
            {
                let conn = rusqlite::Connection::open(crate::paths::get_db_path()).unwrap();
                conn.execute(
                    "INSERT OR REPLACE INTO triggers (id, trigger, output, action_type, target_os, name, tags, is_deleted, created_at, updated_at)
                     VALUES ('test_inner_id', 'testinner', 'Hello from the inner snippet!', 'text', 'all', 'testinner', '[]', 0, 1719878400, 1719878400)",
                    []
                ).unwrap();

                let res =
                    evaluate_template("Output: [use('testinner') | upper] | Date: [date]", None);

                conn.execute("DELETE FROM triggers WHERE id = 'test_inner_id'", [])
                    .ok();

                assert_eq!(res.steps.len(), 1);
                if let ExpansionStep::Text(ref text) = res.steps[0] {
                    println!("ACTUAL TEXT: {}", text);
                    assert!(text.contains("Output: HELLO FROM THE INNER SNIPPET! | Date: "));
                } else {
                    panic!("Expected Text step");
                }
            }

            // Test Case 12: testcombo
            {
                let res = evaluate_template(
                    "User [name='Developer'] checked [url='httpbin.org/json'] at [time.utc.format(HH:mm)] UTC. Title of JSON: [http.get([url]) | json.get('slideshow.title') | upper]",
                    None,
                );
                assert_eq!(res.steps.len(), 1);
                if let ExpansionStep::Text(ref text) = res.steps[0] {
                    assert!(text.contains("User Developer checked httpbin.org/json at "));
                    assert!(text.contains(" UTC. Title of JSON: \x03\x1Fsys:http.get(httpbin.org/json) | json.get('slideshow.title') | upper\x04"));
                } else {
                    panic!("Expected Text step");
                }
            }

            // Test Case 13: testkeys
            {
                let mut args = crate::engine::variables::types::ArgMap::default();
                args.positional.push("Jane".to_string());
                args.positional.push("Smith".to_string());
                args.positional.push("Admin".to_string());
                let res = evaluate_template(
                    "[0=first][key(tab)][delay(100ms)][1=second][key(tab)][delay(50)][2=third][key(enter)]",
                    Some(&args),
                );
                assert_eq!(
                    res.steps,
                    vec![
                        ExpansionStep::Text("Jane".to_string()),
                        ExpansionStep::KeyPress("tab".to_string()),
                        ExpansionStep::Delay(100),
                        ExpansionStep::Text("Smith".to_string()),
                        ExpansionStep::KeyPress("tab".to_string()),
                        ExpansionStep::Delay(50),
                        ExpansionStep::Text("Admin".to_string()),
                        ExpansionStep::KeyPress("enter".to_string()),
                    ]
                );
            }
        }
    }
}
