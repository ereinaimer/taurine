use super::types::ArgMap;

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 {
        let first = s.chars().next().unwrap();
        let last = s.chars().last().unwrap();
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return &s[first.len_utf8()..s.len() - last.len_utf8()];
        }
    }
    s
}

pub fn tokenize(raw: &str, delimiter: char) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }

    let mut tokens = Vec::new();
    let mut current_token = String::new();
    let mut active_quote: Option<char> = None;

    for c in raw.chars() {
        if (c == '"' || c == '\'') && (active_quote.is_none() || active_quote == Some(c)) {
            if active_quote.is_some() {
                active_quote = None;
            } else {
                active_quote = Some(c);
            }
            current_token.push(c);
        } else if c == delimiter && active_quote.is_none() {
            tokens.push(current_token.clone());
            current_token.clear();
        } else {
            current_token.push(c);
        }
    }

    tokens.push(current_token);
    tokens
}

pub fn parse_tokens(tokens: &[String]) -> ArgMap {
    let mut map = ArgMap::default();

    for token in tokens {
        let token = token.trim();
        if token.is_empty() {
            map.positional.push(String::new());
            continue;
        }

        let token = strip_quotes(token);

        if let Some((key, value)) = token.split_once('=') {
            map.named.insert(
                strip_quotes(key).to_string(),
                strip_quotes(value).to_string(),
            );
        } else {
            map.positional.push(token.to_string());
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        assert_eq!(
            tokenize(r#"foo:bar:"baz:qux""#, ':'),
            vec!["foo", "bar", "\"baz:qux\""]
        );
        assert_eq!(tokenize("ereinaimer", ':'), vec!["ereinaimer"]);
        assert_eq!(tokenize("", ':'), Vec::<String>::new());
    }

    #[test]
    fn test_parse_tokens_positional() {
        let tokens = vec!["ereinaimer".to_string(), "taurine".to_string()];
        let map = parse_tokens(&tokens);
        assert_eq!(map.positional, vec!["ereinaimer", "taurine"]);
        assert!(map.named.is_empty());
    }

    #[test]
    fn test_parse_tokens_named() {
        let tokens = vec![
            "username=ereinaimer".to_string(),
            "repo=taurine".to_string(),
        ];
        let map = parse_tokens(&tokens);
        assert_eq!(map.named.get("username").unwrap(), "ereinaimer");
        assert_eq!(map.named.get("repo").unwrap(), "taurine");
        assert!(map.positional.is_empty());
    }

    #[test]
    fn test_parse_tokens_quoted_values() {
        let tokens = vec!["name=\"John Doe\"".to_string(), "repo=taurine".to_string()];
        let map = parse_tokens(&tokens);
        assert_eq!(map.named.get("name").unwrap(), "John Doe");
        assert_eq!(map.named.get("repo").unwrap(), "taurine");
        assert!(map.positional.is_empty());
    }

    #[test]
    fn test_parse_tokens_mixed() {
        let tokens = vec![
            "first".to_string(),
            "\"second arg\"".to_string(),
            "key=\"val\"".to_string(),
            "another=123".to_string(),
        ];
        let map = parse_tokens(&tokens);
        assert_eq!(map.positional, vec!["first", "second arg"]);
        assert_eq!(map.named.get("key").unwrap(), "val");
        assert_eq!(map.named.get("another").unwrap(), "123");
    }

    #[test]
    fn test_parse_tokens_single_quoted() {
        let tokens = vec![
            "name='Neil Armstrong'".to_string(),
            "repo=taurine".to_string(),
        ];
        let map = parse_tokens(&tokens);
        assert_eq!(map.named.get("name").unwrap(), "Neil Armstrong");
        assert_eq!(map.named.get("repo").unwrap(), "taurine");
        assert!(map.positional.is_empty());
    }

    mod compatibility_parser_tests {
        use super::*;

        #[test]
        fn tokenize_keeps_colons_inside_single_and_double_quotes() {
            assert_eq!(
                tokenize(r#"alpha:'beta:gamma':"delta:epsilon":zeta"#, ':'),
                vec!["alpha", "'beta:gamma'", "\"delta:epsilon\"", "zeta"]
            );
        }

        #[test]
        fn parse_tokens_preserves_empty_arguments() {
            let tokens = tokenize("alpha::beta:", ':');
            let map = parse_tokens(&tokens);

            assert_eq!(map.positional, vec!["alpha", "", "beta", ""]);
            assert!(map.named.is_empty());
        }

        #[test]
        fn parse_tokens_uses_first_equals_for_named_values() {
            let tokens = vec![
                "query=foo=bar=baz".to_string(),
                "formula=\"x=1:y=2\"".to_string(),
            ];
            let map = parse_tokens(&tokens);

            assert_eq!(map.named.get("query").unwrap(), "foo=bar=baz");
            assert_eq!(map.named.get("formula").unwrap(), "x=1:y=2");
        }

        #[test]
        fn parse_tokens_strips_outer_quotes_from_whole_named_pair() {
            let tokens = vec![
                "\"name=Neil Armstrong\"".to_string(),
                "'repo=taurine'".to_string(),
            ];
            let map = parse_tokens(&tokens);

            assert_eq!(map.named.get("name").unwrap(), "Neil Armstrong");
            assert_eq!(map.named.get("repo").unwrap(), "taurine");
        }

        #[test]
        fn parse_tokens_preserves_spaces_inside_quotes_for_hybrid_arguments() {
            let tokens = vec!["\"bye \"".to_string(), "4".to_string()];
            let map = parse_tokens(&tokens);
            assert_eq!(map.positional[0], "bye ");
            assert_eq!(map.positional[1], "4");
        }
    }
}
