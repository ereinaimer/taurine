use super::strip_argument_quotes;
use regex::Regex;
use std::sync::LazyLock;
use tracing::warn;

static URL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s<>"']+"#).expect("valid URL regex"));
static EMAIL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").expect("valid email regex")
});

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    match transformer {
        "reverse" if args.is_empty() => Some(content.chars().rev().collect()),
        "length" if args.is_empty() => Some(content.chars().count().to_string()),
        "trim" if args.is_empty() => Some(content.trim().to_string()),
        "truncate" if args.len() == 1 => truncate(content, args[0]),
        "repeat" if args.len() == 1 => repeat(content, args[0]),
        "replace" if args.len() == 2 => Some(replace(content, args[0], args[1])),
        "remove" if args.len() == 1 => Some(remove(content, args[0])),
        "regexreplace" if args.len() == 2 => Some(regex_replace(content, args[0], args[1])),
        "substring" if args.len() == 2 => substring(content, args[0], args[1]),
        "extracturls" if args.is_empty() => Some(extract_urls(content)),
        "extractemails" if args.is_empty() => Some(extract_emails(content)),
        "onlydigits" if args.is_empty() => Some(only_digits(content)),
        "onlyalphanumeric" if args.is_empty() => Some(only_alphanumeric(content)),
        "stripall" if args.is_empty() => Some(strip_all(content)),
        _ => None,
    }
}

fn truncate(content: &str, arg: &str) -> Option<String> {
    let limit = strip_argument_quotes(arg).parse::<usize>().ok()?;
    Some(content.chars().take(limit).collect())
}

const MAX_REPEAT_BUFFER_BYTES: usize = 200_000;

fn repeat(content: &str, arg: &str) -> Option<String> {
    let raw_count = strip_argument_quotes(arg).parse::<usize>().ok()?;
    let count = raw_count.min(100);
    if content.len().saturating_mul(count) > MAX_REPEAT_BUFFER_BYTES {
        return Some("[Error: Transformer output exceeded maximum character limit]".to_string());
    }
    Some(content.repeat(count))
}

fn replace(content: &str, old: &str, new: &str) -> String {
    let old = strip_argument_quotes(old);
    let new = strip_argument_quotes(new);
    content.replace(old, new)
}

fn remove(content: &str, needle: &str) -> String {
    content.replace(strip_argument_quotes(needle), "")
}

fn regex_replace(content: &str, pattern: &str, replacement: &str) -> String {
    let pattern = strip_argument_quotes(pattern);
    let replacement = strip_argument_quotes(replacement);

    match Regex::new(pattern) {
        Ok(regex) => regex.replace_all(content, replacement).into_owned(),
        Err(error) => {
            warn!(
                transformer = "regexreplace",
                pattern,
                %error,
                "invalid regex pattern"
            );
            content.to_string()
        }
    }
}

fn substring(content: &str, start: &str, end: &str) -> Option<String> {
    let start = strip_argument_quotes(start).parse::<usize>().ok()?;
    let end = strip_argument_quotes(end).parse::<usize>().ok()?;
    let chars: Vec<_> = content.chars().collect();
    let len = chars.len();
    let start = start.min(len);
    let end = end.min(len);

    if start >= end {
        return Some(String::new());
    }

    Some(chars[start..end].iter().collect())
}

fn extract_urls(content: &str) -> String {
    URL_REGEX
        .find_iter(content)
        .map(|capture| trim_trailing_punctuation(capture.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_emails(content: &str) -> String {
    EMAIL_REGEX
        .find_iter(content)
        .map(|capture| capture.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn only_digits(content: &str) -> String {
    content.chars().filter(|ch| ch.is_ascii_digit()).collect()
}

fn only_alphanumeric(content: &str) -> String {
    content.chars().filter(|ch| ch.is_alphanumeric()).collect()
}

fn strip_all(content: &str) -> String {
    content.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn trim_trailing_punctuation(match_text: &str) -> String {
    match_text
        .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']'])
        .to_string()
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
        assert_eq!(apply("repeat", &["3"], "hi"), Some("hihihi".to_string()));
        assert_eq!(apply("repeat", &["0"], "hi"), Some("".to_string()));
        assert_eq!(apply("repeat", &["150"], "a"), Some("a".repeat(100)));
        assert_eq!(
            apply("repeat", &["100"], &"x".repeat(3000)),
            Some("[Error: Transformer output exceeded maximum character limit]".to_string())
        );
        assert_eq!(
            apply("replace", &["\"a\"", "\"o\""], "banana"),
            Some("bonono".to_string())
        );
        assert_eq!(
            apply("remove", &["\"na\""], "banana"),
            Some("ba".to_string())
        );
        assert_eq!(
            apply("onlydigits", &[], "ID: A-10-9"),
            Some("109".to_string())
        );
        assert_eq!(
            apply("onlyalphanumeric", &[], "a b-c_1!"),
            Some("abc1".to_string())
        );
        assert_eq!(
            apply("stripall", &[], " a \n b\tc "),
            Some("abc".to_string())
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

    #[test]
    fn test_regexreplace_handles_quoted_commas_and_groups() {
        assert_eq!(
            apply(
                "regexreplace",
                &["\"([a-z]),([A-Z])\"", "\"$1 $2\""],
                "a,B c,D"
            ),
            Some("a B c D".to_string())
        );
    }

    #[test]
    fn test_regexreplace_invalid_pattern_falls_back_to_original() {
        assert_eq!(
            apply("regexreplace", &["\"(\"", "\"x\""], "hello"),
            Some("hello".to_string())
        );
    }

    #[test]
    fn test_substring_is_char_aware() {
        assert_eq!(
            apply("substring", &["1", "3"], "aßc"),
            Some("ßc".to_string())
        );
        assert_eq!(
            apply("substring", &["2", "99"], "naïve"),
            Some("ïve".to_string())
        );
        assert_eq!(
            apply("substring", &["4", "2"], "hello"),
            Some(String::new())
        );
    }

    #[test]
    fn test_extractors_return_newline_separated_matches() {
        let text = "Links: https://example.com, https://openai.com/docs! Email aimer@example.com";
        assert_eq!(
            apply("extracturls", &[], text),
            Some("https://example.com\nhttps://openai.com/docs".to_string())
        );
        assert_eq!(
            apply("extractemails", &[], text),
            Some("aimer@example.com".to_string())
        );
    }
}
