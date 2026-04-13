use super::system;
use super::types::ArgMap;

use indexmap::IndexMap;

#[derive(Debug, PartialEq)]
pub(crate) struct Placeholder<'a> {
    pub key: &'a str,
    pub default_value: Option<&'a str>,
}

pub(crate) fn extract_placeholders<'a>(template: &'a str) -> IndexMap<&'a str, Placeholder<'a>> {
    let mut placeholders = IndexMap::new();
    let bytes = template.as_bytes();
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
            // Find the MATCHING closing brace by counting depth
            let start = ptr + 1;
            let mut end = start;
            let mut depth = 1;

            while end < bytes.len() {
                if bytes[end] == b'\\'
                    && end + 1 < bytes.len()
                    && (bytes[end + 1] == b'{' || bytes[end + 1] == b'}')
                {
                    end += 2;
                    continue;
                }
                if bytes[end] == b'{' {
                    depth += 1;
                } else if bytes[end] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                end += 1;
            }

            if depth == 0 {
                let inner = &template[start..end];
                let (mut key, default_value) = split_key_default(inner);

                // Strip transformer prefixes to find the base user key
                while let Some((_, sub)) = system::split_transformer(key) {
                    key = sub;
                }

                if !system::is_reserved(key)
                    && !placeholders.contains_key(key)
                    && system::strip_quotes(key).is_none()
                {
                    placeholders.insert(key, Placeholder { key, default_value });
                }

                // Even if reserved, its children might contain placeholders
                // So we continue scanning from start
                ptr = start;
                continue;
            }
        }
        ptr += 1;
    }

    placeholders
}

fn split_key_default(inner: &str) -> (&str, Option<&str>) {
    let bytes = inner.as_bytes();
    let mut depth = 0;
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
            depth += 1;
        } else if bytes[ptr] == b'}' {
            depth -= 1;
        } else if bytes[ptr] == b'=' && depth == 0 {
            return (&inner[..ptr], Some(&inner[ptr + 1..]));
        }
        ptr += 1;
    }
    (inner, None)
}

pub fn interpolate(template: &str, args: &ArgMap) -> String {
    let placeholders = extract_placeholders(template);
    let mut user_resolutions = std::collections::HashMap::new();
    let mut pos_cursor = 0;

    for (key, placeholder) in placeholders.iter() {
        let resolved = if let Some(val) = args.named.get(*key) {
            val.clone()
        } else if pos_cursor < args.positional.len() {
            let val = args.positional[pos_cursor].clone();
            pos_cursor += 1;
            val
        } else if let Some(def) = placeholder.default_value {
            def.to_string()
        } else {
            format!("\x01{}\x02", key)
        };
        user_resolutions.insert(*key, resolved);
    }

    let mut output = template.to_string();
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 128;

    while iterations < MAX_ITERATIONS {
        if let Some((start, end)) = find_innermost_tag(&output) {
            let inner = &output[start + 1..end];
            let (key, _) = split_key_default(inner);
            let resolved = if let Some(sys) = system::resolve(key) {
                sys
            } else if let Some(user) = user_resolutions.get(key) {
                user.clone()
            } else if let Some((prefix, sub)) = system::split_transformer(key) {
                // Flattened resolution (e.g. upper.msg)
                if let Some(res) = resolve_prefixed(prefix, sub, &user_resolutions) {
                    res
                } else {
                    format!("\x01{}\x02", inner)
                }
            } else if system::is_directive(key) {
                // Directive stays for finalization phase, use sentinel to avoid re-processing
                format!("\x01{}\x02", key)
            } else {
                // Unknown tag, keep as is
                format!("\x01{}\x02", inner) // Sentinel to mark as "touched"
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

fn resolve_prefixed(
    prefix: &str,
    sub: &str,
    user_resolutions: &std::collections::HashMap<&str, String>,
) -> Option<String> {
    let content = if let Some(res) = system::resolve(sub) {
        res
    } else if let Some(user) = user_resolutions.get(sub) {
        user.clone()
    } else if let Some((p2, s2)) = system::split_transformer(sub) {
        resolve_prefixed(p2, s2, user_resolutions)?
    } else if let Some(unquoted) = system::strip_quotes(sub) {
        unquoted.to_string()
    } else {
        // Fallback: literal
        sub.to_string()
    };

    // If the content is an unresolved sentinel, we don't transform it yet.
    // This allows the transformer to stay intact: {upper.\x01msg\x02}
    if content.starts_with('\x01') && content.ends_with('\x02') {
        return None;
    }

    system::format::apply(prefix, &content)
}

fn find_innermost_tag(s: &str) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'}' {
            // Check if escaped
            if i > 0 && bytes[i - 1] == b'\\' {
                continue;
            }

            // Look backwards for the first '{'
            for j in (0..i).rev() {
                if bytes[j] == b'{' {
                    if j > 0 && bytes[j - 1] == b'\\' {
                        continue;
                    }
                    return Some((j, i));
                }
            }
        }
    }
    None
}

fn finalize_interpolation(mut s: String) -> String {
    // 1. Remove sentinel markers
    s = s.replace('\x01', "{").replace('\x02', "}");

    // 2. Resolve escapes: \{, \}, \\
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut ptr = 0;

    while ptr < bytes.len() {
        if bytes[ptr] == b'\\' && ptr + 1 < bytes.len() {
            let next = bytes[ptr + 1];
            if next == b'{' || next == b'}' || next == b'\\' {
                // Specialized \{cursor\} handling for finalizer
                if s[ptr..].starts_with(r#"\{cursor\}"#) {
                    result.push_str(r#"\{cursor\}"#);
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
        let text = "https://github.com/{username=ereinaimer}/{repo}";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 2);
        assert_eq!(p.get("username").unwrap().default_value, Some("ereinaimer"));
        assert_eq!(p.get("repo").unwrap().default_value, None);
    }

    #[test]
    fn test_extract_placeholders_deduplicate() {
        let text = "a {foo} b {foo=bar} c {foo}";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 1);
        // Should keep the first appearance
        assert_eq!(p.get("foo").unwrap().default_value, None);
    }

    #[test]
    fn test_extract_placeholders_ignore_system() {
        let text = "Hello {cursor} at {time.now}. My name is {name}";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 1);
        assert!(p.contains_key("name"));
        assert!(!p.contains_key("cursor"));
        assert!(!p.contains_key("time.now"));
    }

    #[test]
    fn test_extract_placeholders_escapes() {
        let text = r#"function \{ return "{msg}"; \}"#;
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 1);
        assert!(p.contains_key("msg"));
    }

    #[test]
    fn test_interpolate_positional() {
        let mut args = ArgMap::default();
        args.positional.push("ereinaimer".to_string());
        args.positional.push("taurine".to_string());

        let tpl = "https://github.com/{username}/{repo}";
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

        let tpl = "https://github.com/{username}/{repo}";
        assert_eq!(
            interpolate(tpl, &args),
            "https://github.com/ereinaimer/taurine"
        );
    }

    #[test]
    fn test_interpolate_defaults() {
        let args = ArgMap::default();
        let tpl = "https://github.com/{username=ereinaimer}/{repo=taurine}";
        assert_eq!(
            interpolate(tpl, &args),
            "https://github.com/ereinaimer/taurine"
        );
    }

    #[test]
    fn test_interpolate_empty_default() {
        let args = ArgMap::default();
        let tpl = "git commit -m \"fix: {msg=}\"";
        assert_eq!(interpolate(tpl, &args), "git commit -m \"fix: \"");
    }

    #[test]
    fn test_interpolate_missing_args() {
        let args = ArgMap::default();
        let tpl = "https://github.com/{username}/{repo}";
        assert_eq!(
            interpolate(tpl, &args),
            "https://github.com/{username}/{repo}"
        );
    }

    #[test]
    fn test_interpolate_escapes() {
        let args = ArgMap::default();
        let tpl = r#"const x = \{ "key": "{value=123}" \}; // literal \\ path"#;
        assert_eq!(
            interpolate(tpl, &args),
            r#"const x = { "key": "123" }; // literal \ path"#
        );
    }

    #[test]
    fn test_interpolate_system_variables() {
        let mut args = ArgMap::default();
        args.named.insert("msg".to_string(), "hello".to_string());

        system::clipboard::set_mock_clipboard(Some("clip_content".to_string()));

        let tpl = "{msg} {cursor} {time.now} {clipboard}";
        let res = interpolate(tpl, &args);

        assert!(res.contains("hello {cursor} "));
        assert!(res.contains("clip_content"));
        assert!(!res.contains("{time.now}"));
        assert!(!res.contains("{clipboard}"));

        system::clipboard::set_mock_clipboard(None);
    }

    #[test]
    fn test_interpolate_system_cursor_collision() {
        let args = ArgMap::default();
        let tpl = "Hello {cursor=invalid} world";
        assert_eq!(interpolate(tpl, &args), "Hello {cursor} world");
    }

    #[test]
    fn test_extract_cursor_offset() {
        use super::super::types::ExpansionStep;

        let res = system::finalize("hello {cursor} world", None);
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

        let res2 = system::finalize("hello {cursor} world {cursor}", None);
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

        let res3 = system::finalize(r#"Hello \{cursor\}"#, None);
        assert_eq!(
            res3.steps,
            vec![ExpansionStep::Text("Hello {cursor}".to_string())]
        );
    }

    #[test]
    fn test_interpolate_repeated() {
        let mut args = ArgMap::default();
        args.positional.push("foo".to_string());
        let tpl = "https://{username}.github.io/{username}";
        assert_eq!(interpolate(tpl, &args), "https://foo.github.io/foo");
    }

    #[test]
    fn test_interpolate_nested_system() {
        let mut args = ArgMap::default();
        args.named
            .insert("val".to_string(), "MixedCase".to_string());
        let tpl = "{upper.{lower.val}}";
        assert_eq!(interpolate(tpl, &args), "MIXEDCASE");
    }

    #[test]
    fn test_interpolate_nested_user() {
        let mut args = ArgMap::default();
        args.named.insert("name".to_string(), "john".to_string());
        let tpl = "{upper.{name}}";
        assert_eq!(interpolate(tpl, &args), "JOHN");
    }

    #[test]
    fn test_interpolate_nested_default() {
        let args = ArgMap::default();
        // Template: {outer={inner=fallback}}
        // inner resolves to fallback, then outer resolves to fallback
        let tpl = "{outer={inner=fallback}}";
        assert_eq!(interpolate(tpl, &args), "fallback");
    }

    #[test]
    fn test_interpolate_balanced_with_escapes() {
        let args = ArgMap::default();
        // Use quotes to ensure it's treated as a literal and not an unresolved placeholder
        let tpl = r#"{upper.'a\{b\}c'}"#;
        assert_eq!(interpolate(tpl, &args), "A{B}C");
    }

    #[test]
    fn test_interpolate_flattened_system() {
        let args = ArgMap::default();
        // upper.time.now should resolve to the current time in uppercase
        let res = interpolate("{upper.time.now}", &args);
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
        // upper.name should resolve to JOHN
        assert_eq!(interpolate("{upper.name}", &args), "JOHN");
    }

    #[test]
    fn test_interpolate_quoted_literal() {
        let args = ArgMap::default();
        assert_eq!(interpolate("{upper.'hello world'}", &args), "HELLO WORLD");
        assert_eq!(interpolate("{upper.\"hello world\"}", &args), "HELLO WORLD");
    }

    #[test]
    fn test_interpolate_deep_flattened() {
        let mut args = ArgMap::default();
        args.named
            .insert("val".to_string(), "MixedCase".to_string());
        assert_eq!(interpolate("{upper.lower.val}", &args), "MIXEDCASE");
    }

    #[test]
    fn test_extract_placeholders_prefixed() {
        let text = "Hello {upper.name} and {lower.email=DEFAULT@EMAIL.COM}";
        let p = extract_placeholders(text);
        assert_eq!(p.len(), 2);
        assert!(p.contains_key("name"));
        assert!(p.contains_key("email"));
        assert_eq!(
            p.get("email").unwrap().default_value,
            Some("DEFAULT@EMAIL.COM")
        );
    }
}
