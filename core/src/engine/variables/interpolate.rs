use super::registry::{split_system_tag, strip_global_transformers};
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

    while ptr < bytes.len() {
        match bytes[ptr] {
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
        let (mut key, default_value) = split_key_default(inner);

        // Strip transformer suffixes to find the base user key
        while let Some((sub, _)) = system::split_modifier(key) {
            key = sub;
        }

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

pub fn interpolate(template: &str, args: &ArgMap) -> String {
    interpolate_with_depth(template, args, 0)
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
            let (key, default_value) = split_key_default(inner);

            let resolved = if let Some(user) = user_resolutions.get(key) {
                user.clone()
            } else if let Some(sys) = resolve_system_placeholder(key, default_value, args, depth) {
                sys
            } else if let Some((sub, suffix)) = system::split_modifier(key)
                && (is_valid_user_reference(
                    strip_global_transformers(sub),
                    default_value,
                    args,
                    usize::MAX,
                ) || system::strip_quotes(sub).is_some()
                    || sub.chars().all(|c| c.is_ascii_digit()))
            {
                // Flattened resolution (e.g. msg.upper)
                if let Some(res) =
                    resolve_modified(sub, suffix, default_value, args, &user_resolutions, depth)
                {
                    res
                } else {
                    format!("{SENTINEL_OPEN}{inner}{SENTINEL_CLOSE}")
                }
            } else if system::is_directive(key) {
                // Directive stays for finalization phase, use sentinel to avoid re-processing
                format!("{SENTINEL_OPEN}{key}{SENTINEL_CLOSE}")
            } else {
                // Unknown tag, keep as is
                format!("{SENTINEL_OPEN}{inner}{SENTINEL_CLOSE}") // Sentinel to mark as "touched"
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

fn resolve_modified(
    sub: &str,
    suffix: &str,
    default_value: Option<&str>,
    args: &ArgMap,
    user_resolutions: &std::collections::HashMap<&str, String>,
    depth: usize,
) -> Option<String> {
    let content = if let Some(res) = system::resolve(sub) {
        res
    } else if let Some(user) = user_resolutions.get(sub) {
        user.clone()
    } else if let Some((s2, p2)) = system::split_modifier(sub) {
        resolve_modified(s2, p2, default_value, args, user_resolutions, depth)?
    } else if let Some(user) = {
        let mut pos_idx_dummy = usize::MAX;
        resolve_user_placeholder(sub, default_value, args, depth, &mut pos_idx_dummy)
    } {
        user
    } else if let Some(unquoted) = system::strip_quotes(sub) {
        unquoted.to_string()
    } else {
        // Fallback: literal
        sub.to_string()
    };

    // If the content is an unresolved sentinel, we don't transform it yet.
    // This allows the transformer to stay intact: [\x01msg\x02.upper]
    if content.starts_with(SENTINEL_OPEN) && content.ends_with(SENTINEL_CLOSE) {
        return None;
    }

    system::format::apply(suffix, &content)
}

fn resolve_system_placeholder(
    key: &str,
    default_value: Option<&str>,
    args: &ArgMap,
    depth: usize,
) -> Option<String> {
    if let Some(resolved) = system::resolve(key) {
        return Some(resolved);
    }

    if system::is_directive(strip_global_transformers(key)) {
        return None;
    }

    let default_value = default_value?;
    let fallback = resolve_default_value(default_value, args, depth);
    apply_transformers(key, fallback)
}

fn apply_transformers(key: &str, content: String) -> Option<String> {
    if let Some((sub, suffix)) = system::split_modifier(key) {
        let content = apply_transformers(sub, content)?;
        system::format::apply(suffix, &content)
    } else {
        Some(content)
    }
}

fn find_innermost_tag(s: &str) -> Option<(usize, usize)> {
    scan_tag_bounds(s)
        .into_iter()
        .next()
        .map(|tag| (tag.start, tag.end))
}

fn finalize_interpolation(mut s: String) -> String {
    // 1. Remove sentinel markers
    s = s.replace(SENTINEL_OPEN, "[").replace(SENTINEL_CLOSE, "]");

    // 2. Resolve escapes: \[, \], \\
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut ptr = 0;

    while ptr < bytes.len() {
        if bytes[ptr] == b'\\' && ptr + 1 < bytes.len() {
            let next = bytes[ptr + 1];
            if next == TAG_OPEN || next == TAG_CLOSE || next == b'\\' {
                // Specialized \[cursor\] handling for finalizer
                if s[ptr..].starts_with(r#"\[cursor\]"#) {
                    result.push_str(r#"\[cursor\]"#);
                    ptr += 10;
                    continue;
                }
                result.push(next as char);
                ptr += 2;
                continue;
            }
        }

        let c = s[ptr..].chars().next().unwrap();
        result.push(c);
        ptr += c.len_utf8();
    }
    result
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
        let args = ArgMap::default();
        let tpl = r#"const x = \[ "key": "[value=123]" \]; // literal \\ path"#;
        assert_eq!(
            interpolate(tpl, &args),
            r#"const x = [ "key": "123" ]; // literal \ path"#
        );
    }

    #[test]
    fn test_interpolate_system_variables() {
        let mut args = ArgMap::default();
        args.named.insert("msg".to_string(), "hello".to_string());

        system::clipboard::set_mock_clipboard(Some("clip_content".to_string()));

        let tpl = "[msg] [cursor] [time.now] [clipboard]";
        let res = interpolate(tpl, &args);

        assert!(res.contains("hello [cursor] "));
        assert!(res.contains("clip_content"));
        assert!(!res.contains("[time.now]"));
        assert!(!res.contains("[clipboard]"));

        system::clipboard::set_mock_clipboard(None);
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
        let tpl = "[[val.lower].upper]";
        // Pass 1: [val.lower] -> mixedcase
        // Pass 2: [mixedcase.upper] remains literal
        assert_eq!(interpolate(tpl, &args), "[mixedcase.upper]");
    }

    #[test]
    fn test_interpolate_nested_user() {
        let mut args = ArgMap::default();
        args.named.insert("name".to_string(), "john".to_string());
        let tpl = "[[name].upper]";
        // Under strict validation, unquoted tags that are not variables are left as-is.
        // [name] resolves to john, resulting in [john.upper].
        // john is not a variable, so [john.upper] remains literal.
        assert_eq!(interpolate(tpl, &args), "[john.upper]");
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
    fn test_interpolate_nested_default_before_outer_modifier() {
        let args = ArgMap::default();

        unsafe { std::env::remove_var("TAURINE_NESTED_USER") };

        assert_eq!(
            interpolate(
                "[user.title=[env.TAURINE_NESTED_USER.title=stranger]]",
                &args
            ),
            "Stranger"
        );
    }

    #[test]
    fn test_interpolate_balanced_with_escapes() {
        let args = ArgMap::default();
        // Use quotes to ensure it's treated as a literal and not an unresolved placeholder
        let tpl = r#"['a\[b\]c'.upper]"#;
        assert_eq!(interpolate(tpl, &args), "A[B]C");
    }

    #[test]
    fn test_interpolate_flattened_system() {
        let args = ArgMap::default();
        // time.now.upper should resolve to the current time in uppercase
        let res = interpolate("[time.now.upper]", &args);
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
        // name.upper should resolve to JOHN
        assert_eq!(interpolate("[name.upper]", &args), "JOHN");
    }

    #[test]
    fn test_interpolate_quoted_literal() {
        let args = ArgMap::default();
        assert_eq!(interpolate("['hello world'.upper]", &args), "HELLO WORLD");
        assert_eq!(interpolate("[\"hello world\".upper]", &args), "HELLO WORLD");
    }

    #[test]
    fn test_interpolate_deep_flattened() {
        let mut args = ArgMap::default();
        args.named
            .insert("val".to_string(), "MixedCase".to_string());
        assert_eq!(interpolate("[val.lower.upper]", &args), "MIXEDCASE");
    }

    #[test]
    fn test_extract_placeholders_suffixed() {
        let text = "Hello [name.upper] and [email.lower=DEFAULT@EMAIL.COM]";
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
    fn test_interpolate_urlencode_nested() {
        let mut args = ArgMap::default();
        args.named
            .insert("query".to_string(), "hello world!".to_string());

        let tpl = "https://google.com/search?q=[[query].urlencode]";
        // Under strict validation, unquoted tags resulting from interpolation like [hello world!.urlencode]
        // are left as literal if they don't match a variable.
        assert_eq!(
            interpolate(tpl, &args),
            "https://google.com/search?q=[hello world!.urlencode]"
        );
    }

    #[test]
    fn test_interpolate_google_search_clipboard() {
        let args = ArgMap::default();
        system::clipboard::set_mock_clipboard(Some("customer error msg".to_string()));

        let tpl = "https://google.com/search?q=[[clipboard].urlencode]";
        assert_eq!(
            interpolate(tpl, &args),
            "https://google.com/search?q=[customer error msg.urlencode]"
        );

        system::clipboard::set_mock_clipboard(None);
    }

    #[test]
    fn test_interpolate_urlencode_repro() {
        let mut args = ArgMap::default();
        args.positional.push("banana".to_string());

        let tpl = "https://google.com/search?q=[[0].urlencode]";
        assert_eq!(
            interpolate(tpl, &args),
            "https://google.com/search?q=[banana.urlencode]"
        );
    }

    #[test]
    fn test_interpolate_urlencode_flat() {
        let mut args = ArgMap::default();
        args.positional.push("apple".to_string());

        let tpl = "https://google.com/search?q=[0.urlencode]";
        assert_eq!(interpolate(tpl, &args), "https://google.com/search?q=apple");
    }

    #[test]
    fn test_interpolate_unknown_transformed_tag_remains_literal() {
        let args = ArgMap::default();
        assert_eq!(interpolate("[foo.upper]", &args), "[foo.upper]");
        assert_eq!(
            interpolate("[time.india.upper]", &args),
            "[time.india.upper]"
        );
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
}
