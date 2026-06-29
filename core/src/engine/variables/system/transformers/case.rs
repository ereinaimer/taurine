use heck::*;
use rand::{Rng, RngExt};

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
        "shoutysnake" => Some(preserve_whitespace(content, |s| s.to_shouty_snake_case())),
        "shoutykebab" => Some(preserve_whitespace(content, |s| s.to_shouty_kebab_case())),
        "train" => Some(preserve_whitespace(content, |s| s.to_train_case())),
        "mocking" => Some(mocking_case(content)),
        "leet" => Some(leet_speak(content)),
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

fn mocking_case(content: &str) -> String {
    let mut rng = rand::rng();
    mocking_case_with_rng(content, &mut rng)
}

fn mocking_case_with_rng<R: Rng + ?Sized>(content: &str, rng: &mut R) -> String {
    let mut output = String::with_capacity(content.len());

    for ch in content.chars() {
        if ch.is_alphabetic() {
            if rng.random::<bool>() {
                output.extend(ch.to_uppercase());
            } else {
                output.extend(ch.to_lowercase());
            }
        } else {
            output.push(ch);
        }
    }

    output
}

fn leet_speak(content: &str) -> String {
    content
        .chars()
        .map(|ch| match ch.to_ascii_lowercase() {
            'a' => '4',
            'e' => '3',
            'i' | 'l' => '1',
            'o' => '0',
            's' => '5',
            't' => '7',
            _ => ch,
        })
        .collect()
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
        assert_eq!(
            apply("shoutysnake", "hello_world"),
            Some("HELLO_WORLD".to_string())
        );
        assert_eq!(
            apply("shoutykebab", "hello_world"),
            Some("HELLO-WORLD".to_string())
        );
        assert_eq!(
            apply("train", "hello_world"),
            Some("Hello-World".to_string())
        );
        assert_eq!(
            apply("leet", "Elite Salsa Lot"),
            Some("31173 54154 107".to_string())
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
        assert_eq!(apply("shoutysnakecase", "hello"), None);
        assert_eq!(apply("shoutykebabcase", "hello"), None);
        assert_eq!(apply("traincase", "hello"), None);
        assert_eq!(apply("mockingcase", "hello"), None);
        assert_eq!(apply("spongebobcase", "hello"), None);
        assert_eq!(apply("leetspeak", "hello"), None);
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
