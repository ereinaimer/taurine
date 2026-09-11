use super::strip_argument_quotes;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    match transformer {
        "wrap" if args.len() == 1 => match strip_argument_quotes(args[0]) {
            "doublequote" => Some(format!("\"{content}\"")),
            "singlequote" => Some(format!("'{content}'")),
            "backtick" => Some(format!("`{content}`")),
            _ => None,
        },
        "unwrap" if args.len() == 1 && strip_argument_quotes(args[0]) == "quotes" => {
            Some(unwrap_quotes(content))
        }
        _ => None,
    }
}

fn unwrap_quotes(content: &str) -> String {
    super::super::strip_quotes(content)
        .unwrap_or(content)
        .to_string()
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
        assert_eq!(
            apply("unwrap", &["quotes"], "\"hello\""),
            Some("hello".to_string())
        );
        assert_eq!(
            apply("unwrap", &["quotes"], "hello"),
            Some("hello".to_string())
        );
    }
}
