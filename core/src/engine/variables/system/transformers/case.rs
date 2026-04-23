use heck::*;
use rand::Rng;

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
        "mockingcase" | "spongebobcase" => Some(mocking_case(content)),
        "leet" | "leetspeak" => Some(leet_speak(content)),
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
    use rand::{SeedableRng, rngs::StdRng};

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
        assert_eq!(
            apply("leet", "Elite Salsa Lot"),
            Some("31173 54154 107".to_string())
        );
    }

    #[test]
    fn test_mocking_case_preserves_text_shape() {
        let mut rng = StdRng::seed_from_u64(7);
        let mocked = mocking_case_with_rng("Hello, World!", &mut rng);

        assert_eq!(mocked.chars().count(), "Hello, World!".chars().count());
        assert_eq!(mocked.to_lowercase(), "hello, world!");
        assert_eq!(mocked.chars().nth(5), Some(','));
    }
}
