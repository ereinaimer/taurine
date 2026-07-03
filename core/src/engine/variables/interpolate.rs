use super::registry::split_system_tag;
use super::system;
use super::types::ArgMap;

use indexmap::IndexMap;

const TAG_OPEN: u8 = b'[';
const TAG_CLOSE: u8 = b']';
const SENTINEL_OPEN: char = '\x01';
const SENTINEL_CLOSE: char = '\x02';
const MAX_INTERPOLATION_DEPTH: usize = 32;
const MAX_ITERATIONS: usize = 128;

#[derive(Debug, PartialEq)]
pub(crate) struct Placeholder<'a> {
    pub key: &'a str,
    pub default_value: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TagBounds {
    start: usize,
    end: usize,
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

fn scan_tag_bounds(template: &str) -> Vec<TagBounds> {
    let bytes = template.as_bytes();
    let mut stack = Vec::new();
    let mut tags = Vec::new();
    let mut ptr = 0;
    let mut quote = None;

    while ptr < bytes.len() {
        if let Some(active_quote) = quote {
            if bytes[ptr] == active_quote && !is_escaped(bytes, ptr) {
                quote = None;
            }
            ptr += 1;
            continue;
        }

        match bytes[ptr] {
            b'\'' | b'"' if !stack.is_empty() && !is_escaped(bytes, ptr) => {
                quote = Some(bytes[ptr])
            }
            TAG_OPEN if !is_escaped(bytes, ptr) => stack.push(ptr),
            TAG_CLOSE if !is_escaped(bytes, ptr) => {
                if let Some(start) = stack.pop() {
                    tags.push(TagBounds { start, end: ptr });
                }
            }
            _ => {}
        }
        ptr += 1;
    }

    tags
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

        if !system::is_reserved(key)
            && !placeholders.contains_key(key)
            && system::strip_quotes(key).is_none()
            && !key.contains('[')
            && !key.contains(']')
        {
            placeholders.insert(key, Placeholder { key, default_value });
        }
    }

    placeholders
}

fn is_valid_user_reference(
    key: &str,
    default_value: Option<&str>,
    args: &ArgMap,
    pos_idx: usize,
) -> bool {
    if let Some((root, modifier)) = split_system_tag(key)
        && super::registry::validate_system_tag(root, modifier).is_ok()
    {
        return false;
    }

    key.parse::<usize>().is_ok()
        || args.named.contains_key(key)
        || pos_idx < args.positional.len()
        || (default_value.is_some()
            && system::strip_quotes(key).is_none()
            && !system::is_reserved(key)
            && !key.contains('[')
            && !key.contains(']'))
}

fn resolve_user_placeholder(
    key: &str,
    default_value: Option<&str>,
    args: &ArgMap,
    depth: usize,
    pos_idx: &mut usize,
) -> Option<String> {
    if !is_valid_user_reference(key, default_value, args, *pos_idx) {
        return None;
    }

    if let Ok(index) = key.parse::<usize>() {
        args.positional
            .get(index)
            .cloned()
            .or_else(|| default_value.map(|value| resolve_default_value(value, args, depth)))
    } else if let Some(value) = args.named.get(key) {
        Some(value.clone())
    } else if let Some(value) = args.positional.get(*pos_idx) {
        *pos_idx += 1;
        Some(value.clone())
    } else {
        default_value.map(|value| resolve_default_value(value, args, depth))
    }
}

fn resolve_default_value(default_value: &str, args: &ArgMap, depth: usize) -> String {
    if depth >= MAX_INTERPOLATION_DEPTH {
        default_value.to_string()
    } else {
        interpolate_with_depth(default_value, args, depth + 1)
    }
}

fn split_key_default(inner: &str) -> (&str, Option<&str>) {
    let inner = trim_slice(inner);
    let bytes = inner.as_bytes();
    let mut depth = 0;
    let mut paren_depth = 0;
    let mut ptr = 0;
    let mut quote = None;
    while ptr < bytes.len() {
        if let Some(active_quote) = quote {
            if bytes[ptr] == active_quote && !is_escaped(bytes, ptr) {
                quote = None;
            }
        } else if (bytes[ptr] == b'\'' || bytes[ptr] == b'"') && !is_escaped(bytes, ptr) {
            quote = Some(bytes[ptr]);
        } else if bytes[ptr] == TAG_OPEN && !is_escaped(bytes, ptr) {
            depth += 1;
        } else if bytes[ptr] == TAG_CLOSE && !is_escaped(bytes, ptr) {
            depth -= 1;
        } else if bytes[ptr] == b'(' && !is_escaped(bytes, ptr) {
            paren_depth += 1;
        } else if bytes[ptr] == b')' && !is_escaped(bytes, ptr) {
            paren_depth -= 1;
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

pub fn interpolate(template: &str, args: &ArgMap) -> String {
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

    let mut pos_idx = 0;

    for (key, placeholder) in placeholders.iter() {
        if let Some(resolved) =
            resolve_user_placeholder(key, placeholder.default_value, args, depth, &mut pos_idx)
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

            let base_resolved = if let Some(user) = user_resolutions.get(key) {
                Some(user.clone())
            } else if key.starts_with("use(") && key.ends_with(')') {
                Some(resolve_use_placeholder(key, args, depth))
            } else if let Some(sys) = resolve_system_placeholder(key) {
                Some(sys)
            } else if is_valid_user_reference(key, default_value, args, usize::MAX) {
                let mut pos_idx_dummy = usize::MAX;
                resolve_user_placeholder(key, default_value, args, depth, &mut pos_idx_dummy)
            } else if let Some(unquoted) = system::strip_quotes(key) {
                Some(unquoted.to_string())
            } else if key.chars().all(|c| c.is_ascii_digit()) {
                let mut pos_idx_dummy = usize::MAX;
                resolve_user_placeholder(key, default_value, args, depth, &mut pos_idx_dummy)
            } else if !transformers.is_empty() && (key.contains(' ') || key.contains(SENTINEL_OPEN))
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
            } else if system::is_directive(key) && transformers.is_empty() {
                format!("{SENTINEL_OPEN}{key}{SENTINEL_CLOSE}")
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

    let conn = match rusqlite::Connection::open(crate::paths::get_db_path()) {
        Ok(c) => c,
        Err(e) => return format!("[Error: Database error: {}]", e),
    };

    let action = match crate::db::crud::automations::get_action_by_trigger(&conn, &trigger_name) {
        Ok(Some(act)) => act,
        Ok(None) => return format!("[Error: Snippet '{}' does not exist]", trigger_name),
        Err(e) => return format!("[Error: Database query error: {}]", e),
    };

    if action.action_type != "text" {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_placeholders() {
        let text = "https://github.com/[username=ereinaimer]/[repo]";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 2);
        assert_eq!(p.get("username").unwrap().default_value, Some("ereinaimer"));
        assert_eq!(p.get("repo").unwrap().default_value, None);
    }

    #[test]
    fn test_extract_placeholders_deduplicate() {
        let text = "a [foo] b [foo=bar] c [foo]";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 1);
        // Should keep the first appearance
        assert_eq!(p.get("foo").unwrap().default_value, None);
    }

    #[test]
    fn test_extract_placeholders_ignore_system() {
        let text = "Hello [cursor] at [time.now]. My name is [name]";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 1);
        assert!(p.contains_key("name"));
        assert!(!p.contains_key("cursor"));
        assert!(!p.contains_key("time.now"));
    }

    #[test]
    fn test_extract_placeholders_escapes() {
        let text = r#"function \[ return "[msg]"; \]"#;
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 1);
        assert!(p.contains_key("msg"));
    }

    #[test]
    fn test_extract_placeholders_trims_inner_whitespace() {
        let text = "Hello [  name  ] and [ title = Captain ]";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 2);
        assert!(p.contains_key("name"));
        assert_eq!(p.get("title").unwrap().default_value, Some("Captain"));
    }

    #[test]
    fn test_interpolate_positional() {
        let mut args = ArgMap::default();
        args.positional.push("ereinaimer".to_string());
        args.positional.push("taurine".to_string());

        let tpl = "https://github.com/[0]/[1]";
        assert_eq!(
            interpolate(tpl, &args),
            "https://github.com/ereinaimer/taurine"
        );
    }

    #[test]
    fn test_interpolate_named() {
        let mut args = ArgMap::default();
        args.named.insert("repo".to_string(), "taurine".to_string());
        args.positional.push("ereinaimer".to_string());

        let tpl = "https://github.com/[0]/[repo]";
        assert_eq!(
            interpolate(tpl, &args),
            "https://github.com/ereinaimer/taurine"
        );
    }

    #[test]
    fn test_interpolate_defaults() {
        let args = ArgMap::default();
        let tpl = "https://github.com/[username=ereinaimer]/[repo=taurine]";
        assert_eq!(
            interpolate(tpl, &args),
            "https://github.com/ereinaimer/taurine"
        );
    }

    #[test]
    fn test_interpolate_empty_default() {
        let args = ArgMap::default();
        let tpl = "git commit -m \"fix: [msg=]\"";
        assert_eq!(interpolate(tpl, &args), "git commit -m \"fix: \"");
    }

    #[test]
    fn test_interpolate_missing_args() {
        let args = ArgMap::default();
        let tpl = "https://github.com/[username]/[repo]";
        assert_eq!(
            interpolate(tpl, &args),
            "https://github.com/[username]/[repo]"
        );
    }

    #[test]
    fn test_interpolate_escapes() {
        let text = r#"const x = \[ "key": "123" \]; // literal \\ path"#;
        let args = ArgMap::default();
        let result = interpolate(text, &args);
        // Escapes are now resolved by system::finalize in split_into_steps
        assert_eq!(
            result,
            r#"const x = \[ "key": "123" \]; // literal \\ path"#
        );
    }

    #[test]
    fn test_interpolate_system_variables() {
        let mut args = ArgMap::default();
        args.named.insert("msg".to_string(), "hello".to_string());

        system::clip::set_mock_clip(Some("clip_content".to_string()));

        let tpl = "[msg] [cursor] [time.now] [clip]";
        let res = interpolate(tpl, &args);

        assert!(res.contains("hello [cursor] "));
        assert!(res.contains("clip_content"));
        assert!(!res.contains("[time.now]"));
        assert!(!res.contains("[clip]"));

        system::clip::set_mock_clip(None);
    }

    #[test]
    fn test_interpolate_system_cursor_collision() {
        let args = ArgMap::default();
        let tpl = "Hello [cursor=invalid] world";
        assert_eq!(interpolate(tpl, &args), "Hello [cursor] world");
    }

    #[test]
    fn test_extract_cursor_offset() {
        use super::super::types::ExpansionStep;

        let res = system::finalize("hello [cursor] world", None);
        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("hello  world".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
            ]
        );

        let res2 = system::finalize("hello [cursor] world [cursor]", None);
        assert_eq!(
            res2.steps,
            vec![
                ExpansionStep::Text("hello  world ".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
            ]
        );

        let res3 = system::finalize(r#"Hello \[cursor\]"#, None);
        assert_eq!(
            res3.steps,
            vec![ExpansionStep::Text("Hello [cursor]".to_string())]
        );
    }

    #[test]
    fn test_interpolate_repeated() {
        let mut args = ArgMap::default();
        args.positional.push("foo".to_string());
        let tpl = "https://[0].github.io/[0]";
        assert_eq!(interpolate(tpl, &args), "https://foo.github.io/foo");
    }

    #[test]
    fn test_interpolate_nested_system() {
        let mut args = ArgMap::default();
        args.named
            .insert("val".to_string(), "MixedCase".to_string());
        let tpl = "[[val | lower] | upper]";
        // Pass 1: [val | lower] -> mixedcase
        // Pass 2: [mixedcase | upper] remains literal because mixedcase is not a variable
        assert_eq!(interpolate(tpl, &args), "[mixedcase | upper]");
    }

    #[test]
    fn test_interpolate_nested_user() {
        let mut args = ArgMap::default();
        args.named.insert("name".to_string(), "john".to_string());
        let tpl = "[[name] | upper]";
        // Under strict validation, unquoted tags that are not variables are left as-is.
        // [name] resolves to john, resulting in [john | upper].
        // john is not a variable, so [john | upper] remains literal.
        assert_eq!(interpolate(tpl, &args), "[john | upper]");
    }

    #[test]
    fn test_interpolate_nested_default() {
        let args = ArgMap::default();
        // Template: [outer=[inner=fallback]]
        // inner resolves to fallback, then outer resolves to fallback
        let tpl = "[outer=[inner=fallback]]";
        assert_eq!(interpolate(tpl, &args), "fallback");
    }

    #[test]
    fn test_interpolate_nested_variable_default() {
        let mut args = ArgMap::default();
        args.named
            .insert("default".to_string(), "friend".to_string());

        assert_eq!(interpolate("[name=[default]]", &args), "friend");
    }

    #[test]
    fn test_interpolate_modified_default_prefers_positional_arg() {
        let mut args = ArgMap::default();
        args.positional.push("aimer".to_string());

        assert_eq!(interpolate("[name=erein | title]", &args), "Aimer");
        assert_eq!(
            interpolate("[name=erein | title]", &ArgMap::default()),
            "Erein"
        );
    }

    #[test]
    fn test_interpolate_balanced_with_escapes() {
        let text = r#"A\[B\]C"#;
        let args = ArgMap::default();
        let result = interpolate(text, &args);
        assert_eq!(result, r#"A\[B\]C"#);
    }

    #[test]
    fn test_interpolate_flattened_system() {
        let args = ArgMap::default();
        // time.now | upper should resolve to the current time in uppercase
        let res = interpolate("[time.now | upper]", &args);
        // We check if it resolved to SOMETHING that isn't the literal string or empty
        assert!(!res.is_empty());
        assert!(!res.contains("time.now"));
        // Check if it's uppercase
        assert_eq!(res, res.to_uppercase());
    }

    #[test]
    fn test_interpolate_flattened_user() {
        let mut args = ArgMap::default();
        args.named.insert("name".to_string(), "john".to_string());
        // name | upper should resolve to JOHN
        assert_eq!(interpolate("[name | upper]", &args), "JOHN");
    }

    #[test]
    fn test_interpolate_quoted_literal() {
        let args = ArgMap::default();
        assert_eq!(interpolate("['hello world' | upper]", &args), "HELLO WORLD");
        assert_eq!(
            interpolate("[\"hello world\" | upper]", &args),
            "HELLO WORLD"
        );
    }

    #[test]
    fn test_interpolate_deep_flattened() {
        let mut args = ArgMap::default();
        args.named
            .insert("val".to_string(), "MixedCase".to_string());
        assert_eq!(interpolate("[val | lower | upper]", &args), "MIXEDCASE");
    }

    #[test]
    fn test_extract_placeholders_suffixed() {
        let text = "Hello [name | upper] and [email=DEFAULT@EMAIL.COM | lower]";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 2);
        assert!(p.contains_key("name"));
        assert!(p.contains_key("email"));
        assert_eq!(
            p.get("email").unwrap().default_value,
            Some("DEFAULT@EMAIL.COM")
        );
    }

    #[test]
    fn test_extract_placeholders_parameterized_transformers() {
        let text = "Hello [name | truncate(3)] and [email=DEFAULT | replace(\"@\", \"+\")]";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 2);
        assert!(p.contains_key("name"));
        assert!(p.contains_key("email"));
        assert_eq!(p.get("email").unwrap().default_value, Some("DEFAULT"));
    }

    #[test]
    fn test_interpolate_unknown_transformed_tag_remains_literal() {
        let args = ArgMap::default();
        assert_eq!(interpolate("[foo | upper]", &args), "[foo | upper]");
    }

    #[test]
    fn test_interpolate_parameterized_transformers_for_user_values() {
        let mut args = ArgMap::default();
        args.named.insert("name".to_string(), "john".to_string());

        assert_eq!(interpolate("[name | truncate(2)]", &args), "jo");
        assert_eq!(
            interpolate("[name | replace(\"o\", \"0\") | upper]", &args),
            "J0HN"
        );
    }

    #[test]
    fn test_interpolate_parameterized_transformers_for_system_values() {
        let args = ArgMap::default();
        system::clip::set_mock_clip(Some("alpha,beta".to_string()));

        assert_eq!(interpolate("[clip | truncate(5)]", &args), "alpha");
        assert_eq!(
            interpolate("[clip | replace(\",\", \";\")]", &args),
            "alpha;beta"
        );

        system::clip::set_mock_clip(None);
    }

    #[test]
    fn test_interpolate_clipboard_history_function_syntax() {
        let args = ArgMap::default();
        system::clip::set_mock_clip_history(vec!["current".to_string(), "previous".to_string()]);

        assert_eq!(interpolate("[clip]", &args), "current");
        assert_eq!(interpolate("[clip(0)]", &args), "current");
        assert_eq!(interpolate("[clip(1) | upper]", &args), "PREVIOUS");
        assert_eq!(interpolate("[clip(2)]", &args), "");

        system::clip::set_mock_clip(None);
    }

    #[test]
    fn test_interpolate_replace_handles_literal_commas() {
        let args = ArgMap::default();
        assert_eq!(
            interpolate(r#"['a,b,c' | replace(",", ";")]"#, &args),
            "a;b;c"
        );
    }

    #[test]
    fn test_interpolate_regexreplace_handles_commas_in_quoted_args() {
        let args = ArgMap::default();
        assert_eq!(
            interpolate(
                r#"['a,B,c,D' | regexreplace("([a-z]),([A-Z])", "$1 $2")]"#,
                &args
            ),
            "a B,c D"
        );
    }

    #[test]
    fn test_interpolate_substring_is_utf8_safe() {
        let args = ArgMap::default();
        assert_eq!(interpolate(r#"['aßç' | substring(1, 3)]"#, &args), "ßç");
    }

    #[test]
    fn test_interpolate_json_array_literal_remains_untouched() {
        let args = ArgMap::default();
        assert_eq!(
            interpolate("payload = [1, 2, 3]", &args),
            "payload = [1, 2, 3]"
        );
    }

    #[test]
    fn test_interpolate_user_variable_mapping_to_positional() {
        let mut args = ArgMap::default();
        args.positional.push("monkeytype.com".to_string());

        let tpl = "Start-Process https://[url=google.com]";
        assert_eq!(
            interpolate(tpl, &args),
            "Start-Process https://monkeytype.com"
        );

        // Test fallback when argument is omitted
        let args_empty = ArgMap::default();
        assert_eq!(
            interpolate(tpl, &args_empty),
            "Start-Process https://google.com"
        );
    }

    #[test]
    fn test_interpolate_user_variable_without_default() {
        let mut args = ArgMap::default();
        args.positional.push("myarg".to_string());

        let tpl = "Hello [var]";
        assert_eq!(interpolate(tpl, &args), "Hello myarg");

        // If no arg is passed and no default, it remains as literal
        let args_empty = ArgMap::default();
        assert_eq!(interpolate(tpl, &args_empty), "Hello [var]");
    }

    mod compatibility_interpolation_tests {
        use super::*;

        #[test]
        fn directives_are_preserved_for_finalize_phase() {
            let args = ArgMap::default();

            assert_eq!(
                interpolate("before [cursor] [key(tab)] [delay(25ms)] after", &args),
                "before [cursor] [key(tab)] [delay(25ms)] after"
            );
        }

        #[test]
        fn named_placeholders_do_not_consume_sequential_positional_fallback() {
            let mut args = ArgMap::default();
            args.named
                .insert("name".to_string(), "ereinaimer".to_string());
            args.positional.push("taurine".to_string());

            assert_eq!(
                interpolate("[name] / [repo]", &args),
                "ereinaimer / taurine"
            );
        }

        #[test]
        fn empty_positional_values_beat_defaults() {
            let mut args = ArgMap::default();
            args.positional.push(String::new());

            assert_eq!(interpolate("numeric=[0=fallback]", &args), "numeric=");
            assert_eq!(
                interpolate("sequential=[value=fallback]", &args),
                "sequential="
            );
        }

        #[test]
        fn escaped_cursor_literal_and_backslashes_survive_interpolation() {
            let text = r#"Hello \[cursor\] and \\ path"#;
            let args = ArgMap::default();
            let result = super::interpolate(text, &args);
            // The interpolate step leaves escapes alone, and finalize/split_into_steps processes them.
            assert_eq!(result, r#"Hello \[cursor\] and \\ path"#);
        }

        #[test]
        fn nested_transformer_forms_stay_literal_while_flat_form_resolves() {
            let mut args = ArgMap::default();
            args.positional.push("banana".to_string());

            assert_eq!(
                interpolate("nested=[[0] | url.encode]", &args),
                "nested=[banana | url.encode]"
            );
            assert_eq!(interpolate("flat=[0 | url.encode]", &args), "flat=banana");
        }

        #[test]
        fn detects_and_extracts_ai_markers() {
            assert!(!contains_ai_markers("normal text"));

            let marked = "\x03input_text\x1Fprompt_text\x04";
            assert!(contains_ai_markers(marked));

            let extracted = extract_ai_markers(marked);
            assert_eq!(extracted.len(), 1);
            assert_eq!(extracted[0].0, "input_text");
            assert_eq!(extracted[0].1, "prompt_text");

            let multiple = "hello \x03in1\x1Fp1\x04 and \x03in2\x1Fp2\x04 end";
            let multi_extracted = extract_ai_markers(multiple);
            assert_eq!(multi_extracted.len(), 2);
            assert_eq!(multi_extracted[0].0, "in1");
            assert_eq!(multi_extracted[0].1, "p1");
            assert_eq!(multi_extracted[1].0, "in2");
            assert_eq!(multi_extracted[1].1, "p2");
        }

        #[test]
        fn global_pipeline_strips_quotes_and_preserves_spaces() {
            let args = ArgMap::default();
            // Test 1.1: Global pipeline quote stripping
            assert_eq!(
                interpolate("\"hello world \" | title | repeat(2)", &args),
                "Hello World Hello World "
            );
            assert_eq!(interpolate("'hello world ' | upper", &args), "HELLO WORLD ");
        }

        #[test]
        fn nested_directive_evaluates_to_literal_when_quoted() {
            let args = ArgMap::default();
            // Test 2.2: Quoted directive evaluates to literal without escaping brackets
            // This allows the downstream macro parser (injector) to still see it and execute it.
            // If the user wants to prevent execution, they must escape the brackets explicitly.
            assert_eq!(
                interpolate("['[key(enter)]' | repeat(3)]", &args),
                "[key(enter)][key(enter)][key(enter)]"
            );
        }

        #[test]
        fn test_gcp_manual_case() {
            let mut args = ArgMap::default();
            args.positional.push("cli".to_string());
            args.positional
                .push("add support for custom pipelines".to_string());
            let tpl = "git commit -m \"feat([0=core]): [1=update codebase | sentence]\"[key(enter)][delay(500ms)]git push origin main[key(enter)]";
            assert_eq!(
                interpolate(tpl, &args),
                "git commit -m \"feat(cli): Add support for custom pipelines\"[key(enter)][delay(500ms)]git push origin main[key(enter)]"
            );
        }

        #[test]
        fn test_tblrow_manual_case() {
            let mut args = ArgMap::default();
            args.positional.push("101".to_string());
            args.positional.push("john doe".to_string());
            args.positional.push("active".to_string());
            let tpl = "| [0=ID] | [1=Name | title] | [2=Status | upper] |[key(enter)]| ['--- | ' | repeat(3)][key(enter)]";
            assert_eq!(
                interpolate(tpl, &args),
                "| 101 | John Doe | ACTIVE |[key(enter)]| --- | --- | --- | [key(enter)]"
            );
        }

        #[test]
        fn test_docsnippet_manual_case() {
            let args = ArgMap::default();
            let tpl = r#"\'\[key(enter)\]\' directive | title | repeat(2)"#;
            assert_eq!(
                interpolate(tpl, &args),
                r#"\'\[key(enter)\]\' Directive\'\[key(enter)\]\' Directive"#
            );
        }

        #[test]
        fn test_mockreq_manual_case() {
            let mut args = ArgMap::default();
            args.positional.push("password_reset".to_string());
            // Mock the env var and uuid in system resolve via a mock or just test the structure
            // Since we can't easily mock UUID without a lock, we can use a known value.
            // Wait, UUID changes. We'll skip [uuid] and [date.iso] for exact match and just test env
            // actually we can test the interpolation of `[env(TAURINE_TEST_USER=admin)]`.
            let tpl = r#"{"user": "[env(TAURINE_TEST_USER=admin) | lower]", "action": "[0=login | upper]"}"#;
            assert_eq!(
                interpolate(tpl, &args),
                r#"{"user": "admin", "action": "PASSWORD_RESET"}"#
            );
        }

        #[test]
        fn test_aisummary_manual_case() {
            let args = ArgMap::default();
            let tpl = "### SUMMARY OF COPIED TEXT ([date.short]):[key(enter)][clip | ai(summarize this in 3 concise bullet points) | trim]";
            system::clip::set_mock_clip(Some("Long article text".to_string()));
            let result = interpolate(tpl, &args);
            // date.short will be the actual date, so we just check the AI marker structure
            assert!(result.starts_with("### SUMMARY OF COPIED TEXT ("));
            assert!(result.contains("):[key(enter)]\x03Long article text\x1Fsummarize this in 3 concise bullet points\x04"));
            // trim is applied to the AI marker?
            // the pipeline handles `clipboard | ai(...) | trim` by adding \x03 and \x04.
            // Wait, the test checks if it generates correct markers.
            system::clip::set_mock_clip(None);
        }

        #[test]
        fn test_testchain_manual_case() {
            let args = ArgMap::default();
            let tpl = "'hello_world-demo_test' | replace('_', ' ') | replace('-', ' ') | title | repeat(2)";
            assert_eq!(
                interpolate(tpl, &args),
                "Hello World Demo TestHello World Demo Test"
            );
        }
    }
}
