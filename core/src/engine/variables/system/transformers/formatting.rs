use super::strip_argument_quotes;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    match transformer {
        "wrap" if args.len() == 1 => match strip_argument_quotes(args[0]) {
            "doublequote" => Some(format!("\"{content}\"")),
            "singlequote" => Some(format!("'{content}'")),
            "backtick" => Some(format!("`{content}`")),
            _ => None,
        },
        "unwrap" if args.len() == 1 => match strip_argument_quotes(args[0]) {
            "doublequote" => Some(unwrap_quoted(content, '"')),
            "singlequote" => Some(unwrap_quoted(content, '\'')),
            "backtick" => Some(unwrap_quoted(content, '`')),
            _ => None,
        },
        _ => None,
    }
}

fn unwrap_quoted(content: &str, quote: char) -> String {
    let len = content.len();
    if len >= 2 && content.starts_with(quote) && content.ends_with(quote) {
        content[quote.len_utf8()..len - quote.len_utf8()].to_string()
    } else {
        content.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_transformers() {
        assert_eq!(
            apply("wrap", &["doublequote"], "hello"),
            Some("\"hello\"".to_string())
        );
        assert_eq!(
            apply("wrap", &["singlequote"], "hello"),
            Some("'hello'".to_string())
        );
        assert_eq!(
            apply("wrap", &["backtick"], "hello"),
            Some("`hello`".to_string())
        );
    }

    #[test]
    fn test_unwrap_transformer() {
        for (kind, quoted) in [
            ("doublequote", "\"hello\""),
            ("singlequote", "'hello'"),
            ("backtick", "`hello`"),
        ] {
            assert_eq!(
                apply("unwrap", &[kind], quoted),
                Some("hello".to_string()),
                "unwrap({kind})"
            );
            assert_eq!(
                apply("unwrap", &[kind], "hello"),
                Some("hello".to_string()),
                "unwrap({kind}) wrong-kind no-op"
            );
        }
        assert_eq!(apply("unwrap", &["quotes"], "\"hello\""), None);
        assert_eq!(apply("unwrap", &[], "\"hello\""), None);
    }

    #[test]
    fn test_wrap_unwrap_round_trip() {
        for (kind, wrapped) in [
            ("doublequote", "\"hello\""),
            ("singlequote", "'hello'"),
            ("backtick", "`hello`"),
        ] {
            let once = apply("wrap", &[kind], "hello").unwrap();
            assert_eq!(once, wrapped);
            assert_eq!(apply("unwrap", &[kind], &once).unwrap(), "hello");
        }
    }
}
