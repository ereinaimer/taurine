pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if !args.is_empty() {
        return None;
    }

    match transformer {
        "quote" | "doublequote" => Some(format!("\"{content}\"")),
        "singlequote" => Some(format!("'{content}'")),
        "unquote" => Some(unquote(content)),
        _ => None,
    }
}

fn unquote(content: &str) -> String {
    super::super::strip_quotes(content)
        .unwrap_or(content)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formatting_transformers() {
        assert_eq!(apply("quote", &[], "hello"), Some("\"hello\"".to_string()));
        assert_eq!(
            apply("doublequote", &[], "hello"),
            Some("\"hello\"".to_string())
        );
        assert_eq!(
            apply("singlequote", &[], "hello"),
            Some("'hello'".to_string())
        );
        assert_eq!(
            apply("unquote", &[], "\"hello\""),
            Some("hello".to_string())
        );
        assert_eq!(apply("unquote", &[], "hello"), Some("hello".to_string()));
    }
}
