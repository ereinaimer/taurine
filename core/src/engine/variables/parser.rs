use super::types::ArgMap;

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
    let _ = raw; // placeholder
    ArgMap::default()
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
}
