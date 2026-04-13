use heck::*;

pub const TRANSFORMERS: &[&str] = &[
    "upper",
    "uppercase",
    "lower",
    "lowercase",
    "snake",
    "snakecase",
    "kebab",
    "kebabcase",
    "pascal",
    "pascalcase",
    "camel",
    "camelcase",
    "title",
    "titlecase",
    "shoutysnake",
    "shoutysnakecase",
    "shoutykebab",
    "shoutykebabcase",
    "train",
    "traincase",
];

pub fn resolve(key: &str) -> Option<String> {
    if let Some((prefix, sub_key)) = key.split_once('.') {
        return apply(prefix, sub_key);
    }
    None
}

pub fn apply(transformer: &str, content: &str) -> Option<String> {
    match transformer {
        "upper" | "uppercase" => Some(content.to_uppercase()),
        "lower" | "lowercase" => Some(content.to_lowercase()),
        "snake" | "snakecase" => Some(content.to_snake_case()),
        "kebab" | "kebabcase" => Some(content.to_kebab_case()),
        "pascal" | "pascalcase" => Some(content.to_upper_camel_case()),
        "camel" | "camelcase" => Some(content.to_lower_camel_case()),
        "title" | "titlecase" => Some(content.to_title_case()),
        "shoutysnake" | "shoutysnakecase" => Some(content.to_shouty_snake_case()),
        "shoutykebab" | "shoutykebabcase" => Some(content.to_shouty_kebab_case()),
        "train" | "traincase" => Some(content.to_train_case()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_resolve() {
        assert_eq!(resolve("upper.hello"), Some("HELLO".to_string()));
        assert_eq!(resolve("uppercase.hello"), Some("HELLO".to_string()));
        assert_eq!(resolve("lower.HELLO"), Some("hello".to_string()));
        assert_eq!(resolve("lowercase.HELLO"), Some("hello".to_string()));
        assert_eq!(resolve("snake.HelloWorld"), Some("hello_world".to_string()));
        assert_eq!(
            resolve("snakecase.HelloWorld"),
            Some("hello_world".to_string())
        );
        assert_eq!(resolve("kebab.HelloWorld"), Some("hello-world".to_string()));
        assert_eq!(
            resolve("kebabcase.HelloWorld"),
            Some("hello-world".to_string())
        );
        assert_eq!(
            resolve("pascal.hello_world"),
            Some("HelloWorld".to_string())
        );
        assert_eq!(
            resolve("pascalcase.hello_world"),
            Some("HelloWorld".to_string())
        );
        assert_eq!(resolve("camel.hello_world"), Some("helloWorld".to_string()));
        assert_eq!(
            resolve("camelcase.hello_world"),
            Some("helloWorld".to_string())
        );
        assert_eq!(
            resolve("title.hello_world"),
            Some("Hello World".to_string())
        );
        assert_eq!(
            resolve("titlecase.hello_world"),
            Some("Hello World".to_string())
        );
        assert_eq!(
            resolve("shoutysnake.hello_world"),
            Some("HELLO_WORLD".to_string())
        );
        assert_eq!(
            resolve("shoutysnakecase.hello_world"),
            Some("HELLO_WORLD".to_string())
        );
        assert_eq!(
            resolve("shoutykebab.hello_world"),
            Some("HELLO-WORLD".to_string())
        );
        assert_eq!(
            resolve("shoutykebabcase.hello_world"),
            Some("HELLO-WORLD".to_string())
        );
        assert_eq!(
            resolve("train.hello_world"),
            Some("Hello-World".to_string())
        );
        assert_eq!(
            resolve("traincase.hello_world"),
            Some("Hello-World".to_string())
        );
    }
}
