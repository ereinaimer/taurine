use super::strip_argument_quotes;
use regex::Regex;
use tracing::warn;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    match transformer {
        "length" if args.is_empty() => Some(content.chars().count().to_string()),
        "wordcount" if args.is_empty() => Some(content.split_whitespace().count().to_string()),
        "trim" if args.is_empty() => Some(content.trim().to_string()),
        "truncate" if args.len() == 1 => truncate(content, args[0]),
        "repeat" if args.len() == 1 => repeat(content, args[0]),
        "replace" if args.len() == 2 => Some(replace(content, args[0], args[1])),
        "slug" if args.is_empty() => Some(slug(content)),
        "regexreplace" if args.len() == 2 => Some(regex_replace(content, args[0], args[1])),
        "substring" if args.len() == 2 => substring(content, args[0], args[1]),

        "onlydigit" if args.is_empty() => Some(only_digits(content)),
        "onlyalphanumeric" if args.is_empty() => Some(only_alphanumeric(content)),
        "stripall" if args.is_empty() => Some(strip_all(content)),
        "stripemoji" if args.is_empty() => Some(strip_emoji(content)),
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

fn only_digits(content: &str) -> String {
    content.chars().filter(|ch| ch.is_ascii_digit()).collect()
}

fn only_alphanumeric(content: &str) -> String {
    content.chars().filter(|ch| ch.is_alphanumeric()).collect()
}

fn strip_all(content: &str) -> String {
    content.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn slug(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut last_was_hyphen = false;

    for ch in content.chars() {
        if ch.is_alphanumeric() {
            for lowercase_ch in ch.to_lowercase() {
                result.push(lowercase_ch);
            }
            last_was_hyphen = false;
        } else if (ch.is_whitespace() || ch == '-' || ch == '_' || ch.is_ascii_punctuation())
            && !result.is_empty()
            && !last_was_hyphen
        {
            result.push('-');
            last_was_hyphen = true;
        }
    }

    if result.ends_with('-') {
        result.pop();
    }

    result
}

fn strip_emoji(content: &str) -> String {
    content.chars().filter(|&c| !is_emoji(c)).collect()
}

fn is_emoji(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        0x1F300..=0x1F5FF // Miscellaneous Symbols and Pictographs
        | 0x1F600..=0x1F64F // Emoticons
        | 0x1F680..=0x1F6FF // Transport and Map Symbols
        | 0x1F900..=0x1F9FF // Supplemental Symbols and Pictographs
        | 0x1FA70..=0x1FAFF // Symbols and Pictographs Extended-A
        | 0x2700..=0x27BF   // Dingbats
        | 0x1F1E6..=0x1F1FF // Regional Indicator Symbols (Flags)
        | 0x200D            // Zero Width Joiner
        | 0xFE0F            // Variation Selector-16
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_transformers() {
        assert_eq!(apply("length", &[], "naive"), Some("5".to_string()));
        assert_eq!(
            apply("wordcount", &[], "hello world   foo"),
            Some("3".to_string())
        );
        assert_eq!(apply("wordcount", &[], "   "), Some("0".to_string()));
        assert_eq!(apply("wordcount", &[], ""), Some("0".to_string()));
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
            apply("onlydigit", &[], "ID: A-10-9"),
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
        assert_eq!(
            apply("slug", &[], "My Family Vacation 2026! 🌴"),
            Some("my-family-vacation-2026".to_string())
        );
        assert_eq!(
            apply("slug", &[], "---hello_world---"),
            Some("hello-world".to_string())
        );
        assert_eq!(apply("slug", &[], "   "), Some(String::new()));
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
        assert_eq!(apply("slug", &[], "   "), Some(String::new()));
        assert_eq!(
            apply(
                "stripemoji",
                &[],
                "Huge update today! 🚀🔥 structural changes are coming... 🛠️"
            ),
            Some("Huge update today!  structural changes are coming... ".to_string())
        );
        assert_eq!(
            apply(
                "stripemoji",
                &[],
                "No emojis here, but some normal symbols: © and ® and ™ and &."
            ),
            Some("No emojis here, but some normal symbols: © and ® and ™ and &.".to_string())
        );
        assert_eq!(
            apply(
                "stripemoji",
                &[],
                "Checkmark emoji: ✔️, heart: ❤️, combined flag: 🇺🇸"
            ),
            Some("Checkmark emoji: , heart: , combined flag: ".to_string())
        );
        assert_eq!(apply("stripemoji", &[], "   "), Some("   ".to_string()));
        assert_eq!(
            apply("substring", &["4", "2"], "hello"),
            Some(String::new())
        );
    }
}
