use heck::*;

pub const TRANSFORMERS: &[&str] = &[
    "upper",
    "lower",
    "snake",
    "kebab",
    "pascal",
    "camel",
    "title",
    "shoutysnake",
    "shoutykebab",
    "train",
];

pub fn resolve(key: &str) -> Option<String> {
    if let Some((prefix, sub_key)) = key.split_once('.') {
        return apply(prefix, sub_key);
    }
    None
}

pub fn apply(transformer: &str, content: &str) -> Option<String> {
    match transformer {
        "upper" => Some(content.to_uppercase()),
        "lower" => Some(content.to_lowercase()),
        "snake" => Some(content.to_snake_case()),
        "kebab" => Some(content.to_kebab_case()),
        "pascal" => Some(content.to_upper_camel_case()),
        "camel" => Some(content.to_lower_camel_case()),
        "title" => Some(content.to_title_case()),
        "shoutysnake" => Some(content.to_shouty_snake_case()),
        "shoutykebab" => Some(content.to_shouty_kebab_case()),
        "train" => Some(content.to_train_case()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_resolve() {
        assert_eq!(resolve("upper.hello"), Some("HELLO".to_string()));
        assert_eq!(resolve("lower.HELLO"), Some("hello".to_string()));
        assert_eq!(resolve("snake.HelloWorld"), Some("hello_world".to_string()));
        assert_eq!(resolve("kebab.HelloWorld"), Some("hello-world".to_string()));
        assert_eq!(
            resolve("pascal.hello_world"),
            Some("HelloWorld".to_string())
        );
        assert_eq!(resolve("camel.hello_world"), Some("helloWorld".to_string()));
        assert_eq!(
            resolve("title.hello_world"),
            Some("Hello World".to_string())
        );
        assert_eq!(
            resolve("shoutysnake.hello_world"),
            Some("HELLO_WORLD".to_string())
        );
        assert_eq!(
            resolve("shoutykebab.hello_world"),
            Some("HELLO-WORLD".to_string())
        );
        assert_eq!(
            resolve("train.hello_world"),
            Some("Hello-World".to_string())
        );
    }
}
