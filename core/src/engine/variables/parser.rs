use super::types::ArgMap;

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

pub(crate) fn tokenize_csv(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }

    let mut tokens = Vec::new();
    let mut current_token = String::new();
    let mut in_quote = false;

    for c in raw.chars() {
        if c == '"' {
            in_quote = !in_quote;
            current_token.push(c);
        } else if c == ',' && !in_quote {
            tokens.push(current_token.clone());
            current_token.clear();
        } else {
            current_token.push(c);
        }
    }

    tokens.push(current_token);
    tokens
}

pub fn parse_args(raw: &str) -> ArgMap {
    let mut map = ArgMap::default();

    if raw.trim().is_empty() {
        return map;
    }

    let raw_stripped = strip_quotes(raw);
    let tokens = tokenize_csv(raw_stripped);

    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            map.named.insert(
                strip_quotes(key).to_string(),
                strip_quotes(value).to_string(),
            );
        } else {
            map.positional.push(strip_quotes(&token).to_string());
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_csv() {
        assert_eq!(
            tokenize_csv(r#"foo,bar,"baz,qux""#),
            vec!["foo", "bar", "\"baz,qux\""]
        );
        assert_eq!(tokenize_csv("ereinaimer"), vec!["ereinaimer"]);
        assert_eq!(tokenize_csv(""), Vec::<String>::new());
    }

    #[test]
    fn test_parse_args_positional() {
        let map = parse_args("ereinaimer,taurine");
        assert_eq!(map.positional, vec!["ereinaimer", "taurine"]);
        assert!(map.named.is_empty());
    }

    #[test]
    fn test_parse_args_named() {
        let map = parse_args("username=ereinaimer,repo=taurine");
        assert_eq!(map.named.get("username").unwrap(), "ereinaimer");
        assert_eq!(map.named.get("repo").unwrap(), "taurine");
        assert!(map.positional.is_empty());
    }

    #[test]
    fn test_parse_args_quoted_entire() {
        let map = parse_args(r#""username=ereinaimer,repo=taurine""#);
        assert_eq!(map.named.get("username").unwrap(), "ereinaimer");
        assert_eq!(map.named.get("repo").unwrap(), "taurine");
        assert!(map.positional.is_empty());
    }

    #[test]
    fn test_parse_args_quoted_values() {
        let map = parse_args(r#"name="John Doe",repo=taurine"#);
        assert_eq!(map.named.get("name").unwrap(), "John Doe");
        assert_eq!(map.named.get("repo").unwrap(), "taurine");
        assert!(map.positional.is_empty());
    }

    #[test]
    fn test_parse_args_mixed() {
        let map = parse_args(r#"first,"second arg",key="val",another=123"#);
        assert_eq!(map.positional, vec!["first", "second arg"]);
        assert_eq!(map.named.get("key").unwrap(), "val");
        assert_eq!(map.named.get("another").unwrap(), "123");
    }
}
