use super::strip_argument_quotes;
use regex::Regex;
use tracing::warn;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    match transformer {
        // Counts
        "count" if args.len() == 1 => match strip_argument_quotes(args[0]) {
            "chars" => Some(content.chars().count().to_string()),
            "words" => Some(content.split_whitespace().count().to_string()),
            _ => None,
        },

        // Basic text ops
        "truncate" if args.len() == 1 => truncate(content, args[0]),
        "repeat" if args.len() == 1 => repeat(content, args[0]),

        // replace(old, new) — literal replacement
        "replace" if args.len() == 2 => Some(replace(content, args[0], args[1])),
        // replace(regex, pattern, replacement) — regex replacement
        "replace" if args.len() == 3 && strip_argument_quotes(args[0]) == "regex" => {
            Some(regex_replace(content, args[1], args[2]))
        }

        "slice" if args.len() == 2 => slice(content, args[0], args[1]),

        // Filters
        "filter" if args.len() == 1 => match strip_argument_quotes(args[0]) {
            "digits" => Some(only_digits(content)),
            "alphanumeric" => Some(only_alphanumeric(content)),
            _ => None,
        },

        // Stripping
        "strip" if args.len() == 1 => match strip_argument_quotes(args[0]) {
            "whitespace" => Some(strip_whitespace(content)),
            "emoji" => Some(strip_emoji(content)),
            _ => None,
        },

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
        warn!(
            transformer = "repeat",
            bytes = content.len().saturating_mul(count),
            "transformer output exceeded maximum character limit"
        );
        return None;
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
                transformer = "replace",
                pattern,
                %error,
                "invalid regex pattern"
            );
            content.to_string()
        }
    }
}

fn slice(content: &str, start: &str, end: &str) -> Option<String> {
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

fn strip_whitespace(content: &str) -> String {
    content.trim().to_string()
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
    fn test_count_transformers() {
        assert_eq!(apply("count", &["chars"], "naive"), Some("5".to_string()));
        assert_eq!(
            apply("count", &["words"], "hello world   foo"),
            Some("3".to_string())
        );
        assert_eq!(apply("count", &["words"], "   "), Some("0".to_string()));
        assert_eq!(apply("count", &["words"], ""), Some("0".to_string()));
    }

    #[test]
    fn test_text_transformers() {
        assert_eq!(
            apply("strip", &["whitespace"], "  hi  "),
            Some("hi".to_string())
        );
        assert_eq!(
            apply("truncate", &["4"], "abcdef"),
            Some("abcd".to_string())
        );
        assert_eq!(apply("truncate", &["2"], "aßc"), Some("aß".to_string()));
        assert_eq!(apply("repeat", &["3"], "hi"), Some("hihihi".to_string()));
        assert_eq!(apply("repeat", &["0"], "hi"), Some("".to_string()));
        assert_eq!(apply("repeat", &["150"], "a"), Some("a".repeat(100)));
        assert_eq!(apply("repeat", &["100"], &"x".repeat(3000)), None);
        assert_eq!(
            apply("replace", &["\"a\"", "\"o\""], "banana"),
            Some("bonono".to_string())
        );
    }

    #[test]
    fn test_slice_transformer() {
        assert_eq!(apply("slice", &["1", "3"], "aßc"), Some("ßc".to_string()));
        assert_eq!(
            apply("slice", &["2", "99"], "naïve"),
            Some("ïve".to_string())
        );
        assert_eq!(apply("slice", &["4", "2"], "hello"), Some(String::new()));
    }

    #[test]
    fn test_filter_transformers() {
        assert_eq!(
            apply("filter", &["digits"], "ID: A-10-9"),
            Some("109".to_string())
        );
        assert_eq!(
            apply("filter", &["alphanumeric"], "a b-c_1!"),
            Some("abc1".to_string())
        );
    }

    #[test]
    fn test_strip_transformers() {
        assert_eq!(
            apply("strip", &["whitespace"], "  a b c  "),
            Some("a b c".to_string())
        );
        assert_eq!(
            apply(
                "strip",
                &["emoji"],
                "Huge update today! 🚀🔥 structural changes are coming... 🛠️"
            ),
            Some("Huge update today!  structural changes are coming... ".to_string())
        );
    }
}
