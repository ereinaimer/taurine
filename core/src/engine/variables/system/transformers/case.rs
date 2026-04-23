use heck::*;

pub fn apply(transformer: &str, content: &str) -> Option<String> {
    match transformer {
        "upper" | "uppercase" => Some(content.to_uppercase()),
        "lower" | "lowercase" => Some(content.to_lowercase()),
        "snake" | "snakecase" => Some(content.to_snake_case()),
        "kebab" | "kebabcase" => Some(content.to_kebab_case()),
        "pascal" | "pascalcase" => Some(content.to_upper_camel_case()),
        "camel" | "camelcase" => Some(content.to_lower_camel_case()),
        "title" | "titlecase" => Some(content.to_title_case()),
        "sentencecase" => Some(sentence_case(content)),
        "shoutysnake" | "shoutysnakecase" => Some(content.to_shouty_snake_case()),
        "shoutykebab" | "shoutykebabcase" => Some(content.to_shouty_kebab_case()),
        "train" | "traincase" => Some(content.to_train_case()),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_transformers() {
        assert_eq!(apply("upper", "hello"), Some("HELLO".to_string()));
        assert_eq!(apply("lowercase", "HELLO"), Some("hello".to_string()));
        assert_eq!(
            apply("snake", "HelloWorld"),
            Some("hello_world".to_string())
        );
        assert_eq!(
            apply("kebabcase", "HelloWorld"),
            Some("hello-world".to_string())
        );
        assert_eq!(
            apply("pascal", "hello_world"),
            Some("HelloWorld".to_string())
        );
        assert_eq!(
            apply("camelcase", "hello_world"),
            Some("helloWorld".to_string())
        );
        assert_eq!(
            apply("title", "hello_world"),
            Some("Hello World".to_string())
        );
        assert_eq!(
            apply("sentencecase", "hello world"),
            Some("Hello world".to_string())
        );
        assert_eq!(
            apply("shoutysnake", "hello_world"),
            Some("HELLO_WORLD".to_string())
        );
        assert_eq!(
            apply("shoutykebabcase", "hello_world"),
            Some("HELLO-WORLD".to_string())
        );
        assert_eq!(
            apply("train", "hello_world"),
            Some("Hello-World".to_string())
        );
    }
}
