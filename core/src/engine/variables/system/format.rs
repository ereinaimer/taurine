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
    "urlencode",
];

pub fn resolve(key: &str) -> Option<String> {
    if let Some((sub_key, suffix)) = key.rsplit_once('.') {
        return apply(suffix, sub_key);
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
        "urlencode" => Some(urlencode_string(content)),
        _ => None,
    }
}

fn urlencode_string(s: &str) -> String {
    let mut res = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                res.push(*b as char)
            }
            b' ' => res.push_str("%20"),
            _ => res.push_str(&format!("%{:02X}", b)),
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_resolve() {
        assert_eq!(resolve("hello.upper"), Some("HELLO".to_string()));
        assert_eq!(resolve("hello.uppercase"), Some("HELLO".to_string()));
        assert_eq!(resolve("HELLO.lower"), Some("hello".to_string()));
        assert_eq!(resolve("HELLO.lowercase"), Some("hello".to_string()));
        assert_eq!(resolve("HelloWorld.snake"), Some("hello_world".to_string()));
        assert_eq!(
            resolve("HelloWorld.snakecase"),
            Some("hello_world".to_string())
        );
        assert_eq!(resolve("HelloWorld.kebab"), Some("hello-world".to_string()));
        assert_eq!(
            resolve("HelloWorld.kebabcase"),
            Some("hello-world".to_string())
        );
        assert_eq!(
            resolve("hello_world.pascal"),
            Some("HelloWorld".to_string())
        );
        assert_eq!(
            resolve("hello_world.pascalcase"),
            Some("HelloWorld".to_string())
        );
        assert_eq!(resolve("hello_world.camel"), Some("helloWorld".to_string()));
        assert_eq!(
            resolve("hello_world.camelcase"),
            Some("helloWorld".to_string())
        );
        assert_eq!(
            resolve("hello_world.title"),
            Some("Hello World".to_string())
        );
        assert_eq!(
            resolve("hello_world.titlecase"),
            Some("Hello World".to_string())
        );
        assert_eq!(
            resolve("hello_world.shoutysnake"),
            Some("HELLO_WORLD".to_string())
        );
        assert_eq!(
            resolve("hello_world.shoutysnakecase"),
            Some("HELLO_WORLD".to_string())
        );
        assert_eq!(
            resolve("hello_world.shoutykebab"),
            Some("HELLO-WORLD".to_string())
        );
        assert_eq!(
            resolve("hello_world.shoutykebabcase"),
            Some("HELLO-WORLD".to_string())
        );
        assert_eq!(
            resolve("hello_world.train"),
            Some("Hello-World".to_string())
        );
        assert_eq!(
            resolve("hello_world.traincase"),
            Some("Hello-World".to_string())
        );
        assert_eq!(
            resolve("hello world!.urlencode"),
            Some("hello%20world%21".to_string())
        );
    }
}
