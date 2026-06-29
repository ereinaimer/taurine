use heck::*;

pub fn apply(transformer: &str, content: &str) -> Option<String> {
    match transformer {
        "upper" => Some(content.to_uppercase()),
        "lower" => Some(content.to_lowercase()),
        "snake" => Some(preserve_whitespace(content, |s| s.to_snake_case())),
        "kebab" => Some(preserve_whitespace(content, |s| s.to_kebab_case())),
        "pascal" => Some(preserve_whitespace(content, |s| s.to_upper_camel_case())),
        "camel" => Some(preserve_whitespace(content, |s| s.to_lower_camel_case())),
        "title" => Some(title_case(content)),
        "sentence" => Some(sentence_case(content)),
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
            // Capitalize the first character (if it has uppercase) and clear the flag.
            out.extend(ch.to_uppercase());
            // It only counts as the start of a word if it's alphabetic. Punctuation doesn't toggle
            // new_word, but since we just capitalized it, we set new_word=false anyway.
            new_word = false;
        } else {
            out.push(ch);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_transformers() {
        assert_eq!(apply("upper", "hello"), Some("HELLO".to_string()));
        assert_eq!(apply("lower", "HELLO"), Some("hello".to_string()));
        assert_eq!(
            apply("snake", "HelloWorld"),
            Some("hello_world".to_string())
        );
        assert_eq!(
            apply("kebab", "HelloWorld"),
            Some("hello-world".to_string())
        );
        assert_eq!(
            apply("pascal", "hello_world"),
            Some("HelloWorld".to_string())
        );
        assert_eq!(
            apply("camel", "hello_world"),
            Some("helloWorld".to_string())
        );
        assert_eq!(
            apply("title", "hello_world"),
            Some("Hello_world".to_string())
        );
        assert_eq!(
            apply("sentence", "hello world"),
            Some("Hello world".to_string())
        );
    }

    #[test]
    fn test_pruned_casing_aliases_return_none() {
        assert_eq!(apply("uppercase", "hello"), None);
        assert_eq!(apply("lowercase", "hello"), None);
        assert_eq!(apply("snakecase", "hello"), None);
        assert_eq!(apply("kebabcase", "hello"), None);
        assert_eq!(apply("pascalcase", "hello"), None);
        assert_eq!(apply("camelcase", "hello"), None);
        assert_eq!(apply("titlecase", "hello"), None);
        assert_eq!(apply("sentencecase", "hello"), None);
    }

    #[test]
    fn test_case_transformers_preserve_affixes_and_escapes() {
        assert_eq!(
            apply("title", r#"\'hello world \'"#),
            Some(r#"\'hello World \'"#.to_string())
        );
        assert_eq!(
            apply("snake", r#"\'hello world \'"#),
            Some(r#"\'hello_world \'"#.to_string())
        );
        assert_eq!(
            apply("kebab", r#"\'hello world \'"#),
            Some(r#"\'hello-world \'"#.to_string())
        );
    }
}
