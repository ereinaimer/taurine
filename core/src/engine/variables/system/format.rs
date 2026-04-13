use heck::*;

pub fn resolve(key: &str) -> Option<String> {
    if let Some(val) = key.strip_prefix("upper.") {
        return Some(val.to_uppercase());
    }
    if let Some(val) = key.strip_prefix("lower.") {
        return Some(val.to_lowercase());
    }
    if let Some(val) = key.strip_prefix("snake.") {
        return Some(val.to_snake_case());
    }
    if let Some(val) = key.strip_prefix("kebab.") {
        return Some(val.to_kebab_case());
    }
    if let Some(val) = key.strip_prefix("pascal.") {
        return Some(val.to_upper_camel_case());
    }
    if let Some(val) = key.strip_prefix("camel.") {
        return Some(val.to_lower_camel_case());
    }
    if let Some(val) = key.strip_prefix("title.") {
        return Some(val.to_title_case());
    }
    if let Some(val) = key.strip_prefix("shoutysnake.") {
        return Some(val.to_shouty_snake_case());
    }
    if let Some(val) = key.strip_prefix("shoutykebab.") {
        return Some(val.to_shouty_kebab_case());
    }
    if let Some(val) = key.strip_prefix("train.") {
        return Some(val.to_train_case());
    }
    None
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
