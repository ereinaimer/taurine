use super::types::ArgMap;
use indexmap::IndexMap;

#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub(crate) struct Placeholder<'a> {
    pub key: &'a str,
    pub default_value: Option<&'a str>,
}

#[allow(dead_code)]
pub(crate) fn extract_placeholders(template: &str) -> IndexMap<&str, Placeholder<'_>> {
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

                // Ignore system variables
                if inner != "cursor" && !inner.contains('.') {
                    let (key, default_value) = if let Some((k, v)) = inner.split_once('=') {
                        (k, Some(v))
                    } else {
                        (inner, None)
                    };

                    if !placeholders.contains_key(key) {
                        placeholders.insert(key, Placeholder { key, default_value });
                    }
                }
                ptr = end;
            }
        }
        ptr += 1;
    }

    placeholders
}

pub fn interpolate(template: &str, args: &ArgMap) -> String {
    let _ = args; // placeholder
    template.to_string()
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
}
