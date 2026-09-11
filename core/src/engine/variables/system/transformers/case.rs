use heck::*;

use super::strip_argument_quotes;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if transformer != "case" || args.len() != 1 {
        return None;
    }

    match strip_argument_quotes(args[0]).to_lowercase().as_str() {
        "upper" => Some(content.to_uppercase()),
        "lower" => Some(content.to_lowercase()),
        "snake" => Some(preserve_whitespace(content, |s| s.to_snake_case())),
        "kebab" => Some(preserve_whitespace(content, |s| s.to_kebab_case())),
        "pascal" => Some(preserve_whitespace(content, |s| s.to_upper_camel_case())),
        "camel" => Some(preserve_whitespace(content, |s| s.to_lower_camel_case())),
        "title" => Some(title_case(content)),
        "sentence" => Some(sentence_case(content)),
        "slug" => Some(slug(content)),
        _ => None,
    }
}

fn preserve_whitespace<F: Fn(&str) -> String>(content: &str, transform: F) -> String {
    let first_alphanumeric = content.char_indices().find(|(_, c)| c.is_alphanumeric());
    let last_alphanumeric = content.char_indices().rfind(|(_, c)| c.is_alphanumeric());

    let (Some((leading_len, _)), Some((last_idx, last_char))) =
        (first_alphanumeric, last_alphanumeric)
    else {
        return content.to_string();
    };

    let trailing_start = last_idx + last_char.len_utf8();
    let trimmed = &content[leading_len..trailing_start];
    let transformed = transform(trimmed);

    let mut out =
        String::with_capacity(leading_len + transformed.len() + (content.len() - trailing_start));
    out.push_str(&content[..leading_len]);
    out.push_str(&transformed);
    out.push_str(&content[trailing_start..]);
    out
}

fn sentence_case(content: &str) -> String {
    let mut chars = content.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    let mut out = String::new();
    out.extend(first.to_uppercase());
    out.extend(chars);
    out
}

fn title_case(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut new_word = true;

    for ch in content.chars() {
        if ch.is_whitespace() {
            new_word = true;
            out.push(ch);
        } else if new_word {
            out.extend(ch.to_uppercase());
            new_word = false;
        } else {
            out.push(ch);
        }
    }

    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_transformers() {
        assert_eq!(
            apply("case", &["upper"], "hello"),
            Some("HELLO".to_string())
        );
        assert_eq!(
            apply("case", &["lower"], "HELLO"),
            Some("hello".to_string())
        );
        assert_eq!(
            apply("case", &["snake"], "HelloWorld"),
            Some("hello_world".to_string())
        );
        assert_eq!(
            apply("case", &["kebab"], "HelloWorld"),
            Some("hello-world".to_string())
        );
        assert_eq!(
            apply("case", &["pascal"], "hello_world"),
            Some("HelloWorld".to_string())
        );
        assert_eq!(
            apply("case", &["camel"], "hello_world"),
            Some("helloWorld".to_string())
        );
        assert_eq!(
            apply("case", &["title"], "hello_world"),
            Some("Hello_world".to_string())
        );
        assert_eq!(
            apply("case", &["sentence"], "hello world"),
            Some("Hello world".to_string())
        );
        assert_eq!(
            apply("case", &["slug"], "My Family Vacation 2026! 🌴"),
            Some("my-family-vacation-2026".to_string())
        );
    }

    #[test]
    fn test_case_wrong_transformer_name_returns_none() {
        assert_eq!(apply("upper", &[], "hello"), None);
        assert_eq!(apply("case", &[], "hello"), None);
        assert_eq!(apply("case", &["invalid"], "hello"), None);
    }
}
