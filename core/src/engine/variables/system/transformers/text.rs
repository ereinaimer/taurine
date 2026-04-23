use super::strip_argument_quotes;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    match transformer {
        "reverse" if args.is_empty() => Some(content.chars().rev().collect()),
        "length" if args.is_empty() => Some(content.chars().count().to_string()),
        "trim" if args.is_empty() => Some(content.trim().to_string()),
        "truncate" if args.len() == 1 => truncate(content, args[0]),
        "replace" if args.len() == 2 => Some(replace(content, args[0], args[1])),
        _ => None,
    }
}

fn truncate(content: &str, arg: &str) -> Option<String> {
    let limit = strip_argument_quotes(arg).parse::<usize>().ok()?;
    Some(content.chars().take(limit).collect())
}

fn replace(content: &str, old: &str, new: &str) -> String {
    let old = strip_argument_quotes(old);
    let new = strip_argument_quotes(new);
    content.replace(old, new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_transformers() {
        assert_eq!(
            apply("reverse", &[], "naive cafe"),
            Some("efac evian".to_string())
        );
        assert_eq!(apply("length", &[], "naive"), Some("5".to_string()));
        assert_eq!(apply("trim", &[], "  hi  "), Some("hi".to_string()));
        assert_eq!(
            apply("truncate", &["4"], "abcdef"),
            Some("abcd".to_string())
        );
        assert_eq!(apply("truncate", &["2"], "aßc"), Some("aß".to_string()));
        assert_eq!(
            apply("replace", &["\"a\"", "\"o\""], "banana"),
            Some("bonono".to_string())
        );
    }

    #[test]
    fn test_replace_with_quoted_arguments() {
        assert_eq!(
            apply("replace", &["\",\"", "\";\""], "a,b,c"),
            Some("a;b;c".to_string())
        );
        assert_eq!(
            apply("replace", &["\",\"", "\"\""], "a,b,c"),
            Some("abc".to_string())
        );
    }
}
