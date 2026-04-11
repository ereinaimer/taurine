use super::system;
use super::types::ArgMap;

use indexmap::IndexMap;

#[derive(Debug, PartialEq)]
pub(crate) struct Placeholder<'a> {
    pub key: &'a str,
    pub default_value: Option<&'a str>,
}

pub(crate) fn extract_placeholders<'a>(
    template: &'a str,
    trigger: Option<&str>,
) -> IndexMap<&'a str, Placeholder<'a>> {
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
            let start = ptr + 1;
            let mut end = start;
            let mut found_close = false;

            while end < bytes.len() {
                if bytes[end] == b'}' {
                    found_close = true;
                    break;
                }
                end += 1;
            }

            if found_close {
                let inner = &template[start..end];
                let (key, default_value) = if let Some((k, v)) = inner.split_once('=') {
                    (k, Some(v))
                } else {
                    (inner, None)
                };

                if system::is_reserved(key) && default_value.is_some() {
                    let trigger_ctx = trigger
                        .map(|t| format!(" for trigger '{}'", t))
                        .unwrap_or_default();
                    tracing::warn!(
                        "System variable {{{}}} cannot have a default value. The default assignment will be ignored{}.",
                        key,
                        trigger_ctx
                    );
                }

                if !system::is_reserved(key) && !placeholders.contains_key(key) {
                    placeholders.insert(key, Placeholder { key, default_value });
                }

                ptr = end;
            }
        }
        ptr += 1;
    }

    placeholders
}

pub fn interpolate(template: &str, args: &ArgMap, trigger: Option<&str>) -> String {
    let placeholders = extract_placeholders(template, trigger);
    let mut resolutions = std::collections::HashMap::new();
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
            format!("{{{}}}", key)
        };
        resolutions.insert(*key, resolved);
    }

    let mut output = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut ptr = 0;
    let mut last_pushed = 0;

    while ptr < bytes.len() {
        if bytes[ptr] == b'\\' && ptr + 1 < bytes.len() {
            let next = bytes[ptr + 1];
            if next == b'{' || next == b'}' || next == b'\\' {
                if template[ptr..].starts_with(r#"\{cursor\}"#) {
                    output.push_str(&template[last_pushed..ptr + 9]);
                    ptr += 9;
                    last_pushed = ptr;
                    continue;
                }
                output.push_str(&template[last_pushed..ptr]);
                output.push(next as char);
                ptr += 2;
                last_pushed = ptr;
                continue;
            }
        }

        if bytes[ptr] == b'{' {
            let start = ptr + 1;
            let mut end = start;
            let mut found_close = false;
            while end < bytes.len() {
                if bytes[end] == b'}' {
                    found_close = true;
                    break;
                }
                end += 1;
            }

            if found_close {
                let inner = &template[start..end];
                let key = inner.split_once('=').map(|(k, _)| k).unwrap_or(inner);

                if let Some(resolved) = system::resolve(key) {
                    output.push_str(&template[last_pushed..ptr]);
                    output.push_str(&resolved);
                    ptr = end + 1;
                    last_pushed = ptr;
                    continue;
                } else if let Some(resolved) = resolutions.get(key) {
                    output.push_str(&template[last_pushed..ptr]);
                    output.push_str(resolved);
                    ptr = end + 1;
                    last_pushed = ptr;
                    continue;
                } else if system::is_directive(key) {
                    output.push_str(&template[last_pushed..ptr]);
                    output.push_str(&format!("{{{}}}", key));
                    ptr = end + 1;
                    last_pushed = ptr;
                    continue;
                }
            }
        }

        ptr += 1;
    }

    if last_pushed < template.len() {
        output.push_str(&template[last_pushed..template.len()]);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_placeholders() {
        let text = "https://github.com/{username=ereinaimer}/{repo}";
        let p = extract_placeholders(text, None);
        assert_eq!(p.len(), 2);
        assert_eq!(p.get("username").unwrap().default_value, Some("ereinaimer"));
        assert_eq!(p.get("repo").unwrap().default_value, None);
    }

    #[test]
    fn test_extract_placeholders_deduplicate() {
        let text = "a {foo} b {foo=bar} c {foo}";
        let p = extract_placeholders(text, None);
        assert_eq!(p.len(), 1);
        // Should keep the first appearance
        assert_eq!(p.get("foo").unwrap().default_value, None);
    }

    #[test]
    fn test_extract_placeholders_ignore_system() {
        let text = "Hello {cursor} at {time.now}. My name is {name}";
        let p = extract_placeholders(text, None);
        assert_eq!(p.len(), 1);
        assert!(p.contains_key("name"));
        assert!(!p.contains_key("cursor"));
        assert!(!p.contains_key("time.now"));
    }

    #[test]
    fn test_extract_placeholders_escapes() {
        let text = r#"function \{ return "{msg}"; \}"#;
        let p = extract_placeholders(text, None);
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
            interpolate(tpl, &args, None),
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
            interpolate(tpl, &args, None),
            "https://github.com/ereinaimer/taurine"
        );
    }

    #[test]
    fn test_interpolate_defaults() {
        let args = ArgMap::default();
        let tpl = "https://github.com/{username=ereinaimer}/{repo=taurine}";
        assert_eq!(
            interpolate(tpl, &args, None),
            "https://github.com/ereinaimer/taurine"
        );
    }

    #[test]
    fn test_interpolate_empty_default() {
        let args = ArgMap::default();
        let tpl = "git commit -m \"fix: {msg=}\"";
        assert_eq!(interpolate(tpl, &args, None), "git commit -m \"fix: \"");
    }

    #[test]
    fn test_interpolate_missing_args() {
        let args = ArgMap::default();
        let tpl = "https://github.com/{username}/{repo}";
        assert_eq!(
            interpolate(tpl, &args, None),
            "https://github.com/{username}/{repo}"
        );
    }

    #[test]
    fn test_interpolate_escapes() {
        let args = ArgMap::default();
        let tpl = r#"const x = \{ "key": "{value=123}" \}; // literal \\ path"#;
        assert_eq!(
            interpolate(tpl, &args, None),
            r#"const x = { "key": "123" }; // literal \ path"#
        );
    }

    #[test]
    fn test_interpolate_system_variables() {
        let mut args = ArgMap::default();
        args.named.insert("msg".to_string(), "hello".to_string());
        let tpl = "{msg} {cursor} {time.now}";
        let res = interpolate(tpl, &args, None);
        assert!(res.starts_with("hello {cursor} "));
        assert!(!res.contains("{time.now}"));
        assert!(res.contains(':'));
    }

    #[test]
    fn test_interpolate_system_cursor_collision() {
        let args = ArgMap::default();
        let tpl = "Hello {cursor=invalid} world";
        assert_eq!(interpolate(tpl, &args, None), "Hello {cursor} world");
    }

    #[test]
    fn test_extract_cursor_offset() {
        let res = system::finalize("hello {cursor} world", None);
        assert_eq!(res.text, "hello  world");
        assert_eq!(res.left_arrow_count, 6);

        let res2 = system::finalize("hello {cursor} world {cursor}", None);
        assert_eq!(res2.text, "hello  world ");
        assert_eq!(res2.left_arrow_count, 7);

        let res3 = system::finalize(r#"Hello \{cursor\}"#, None);
        assert_eq!(res3.text, "Hello {cursor}");
        assert_eq!(res3.left_arrow_count, 0);
    }

    // Removed duplicate tests

    #[test]
    fn test_interpolate_repeated() {
        let mut args = ArgMap::default();
        args.positional.push("foo".to_string());
        let tpl = "https://{username}.github.io/{username}";
        assert_eq!(interpolate(tpl, &args, None), "https://foo.github.io/foo");
    }
}
