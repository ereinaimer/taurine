use rand::RngExt;

const DEFAULT_WORD_COUNT: usize = 15;
const DEFAULT_SENTENCE_COUNT: usize = 1;
const DEFAULT_PARAGRAPH_COUNT: usize = 1;
pub(crate) const MAX_LOREM_COUNT: usize = 100_000;

const LOREM_WORDS: &[&str] = &[
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
    "ut",
    "enim",
    "ad",
    "minim",
    "veniam",
    "quis",
    "nostrud",
    "exercitation",
    "ullamco",
    "laboris",
    "nisi",
    "ut",
    "aliquip",
    "ex",
    "ea",
    "commodo",
    "consequat",
    "duis",
    "aute",
    "irure",
    "dolor",
    "in",
    "reprehenderit",
    "in",
    "voluptate",
    "velit",
    "esse",
    "cillum",
    "dolore",
    "eu",
    "fugiat",
    "nulla",
    "pariatur",
    "excepteur",
    "sint",
    "occaecat",
    "cupidatat",
    "non",
    "proident",
    "sunt",
    "in",
    "culpa",
    "qui",
    "officia",
    "deserunt",
    "mollit",
    "anim",
    "id",
    "est",
    "laborum",
];

fn pick_words(count: usize) -> Vec<String> {
    let mut rng = rand::rng();
    (0..count)
        .map(|_| LOREM_WORDS[rng.random_range(0..LOREM_WORDS.len())].to_string())
        .collect()
}

fn pick_sentences(count: usize) -> Vec<String> {
    let mut rng = rand::rng();
    (0..count)
        .map(|_| {
            let sentence_len = rng.random_range(5..=15);
            let mut words: Vec<String> = (0..sentence_len)
                .map(|_| LOREM_WORDS[rng.random_range(0..LOREM_WORDS.len())].to_string())
                .collect();
            if !words.is_empty() {
                let first = words[0]
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default()
                    + &words[0][1..];
                words[0] = first;
                words.push(".".to_string());
            }
            words.join(" ")
        })
        .collect()
}

fn pick_paragraphs(count: usize) -> Vec<String> {
    let mut rng = rand::rng();
    (0..count)
        .map(|_| {
            let sentence_count = rng.random_range(3..=8);
            let sentences: Vec<String> = (0..sentence_count)
                .map(|_| {
                    let sentence_len = rng.random_range(5..=15);
                    let mut words: Vec<String> = (0..sentence_len)
                        .map(|_| LOREM_WORDS[rng.random_range(0..LOREM_WORDS.len())].to_string())
                        .collect();
                    if !words.is_empty() {
                        let first = words[0]
                            .chars()
                            .next()
                            .map(|c| c.to_uppercase().to_string())
                            .unwrap_or_default()
                            + &words[0][1..];
                        words[0] = first;
                        words.push(".".to_string());
                    }
                    words.join(" ")
                })
                .collect();
            sentences.join(" ")
        })
        .collect()
}

/// Resolves the unified `lorem(...)` system variable.
///
/// `raw` is the argument list inside `lorem(...)` (`""` when bare),
/// bound as `(type=paragraphs, count)`; a numeric single arg detects count.
pub fn resolve(raw: &str) -> Option<String> {
    let spec = crate::engine::variables::registry::param_spec("lorem")?;
    let bound = crate::engine::variables::parser::bind_call("lorem", raw, &spec).ok()?;
    if bound.positional.len() > spec.params.len() {
        return None;
    }
    let has_named = crate::engine::variables::parser::has_named_args(&bound, &spec);
    let (kind, count_str) = if has_named {
        let kind = bound
            .named
            .get("type")
            .map(String::as_str)
            .unwrap_or("paragraphs");
        let count = bound.named.get("count").map(String::as_str).unwrap_or("");
        (kind.to_string(), count.to_string())
    } else if bound.positional.is_empty() {
        ("paragraphs".to_string(), String::new())
    } else if bound.positional.len() == 1 && bound.positional[0].parse::<usize>().is_ok() {
        ("paragraphs".to_string(), bound.positional[0].clone())
    } else if bound.positional.len() == 1 {
        (bound.positional[0].clone(), String::new())
    } else {
        (bound.positional[0].clone(), bound.positional[1].clone())
    };
    let default = match kind.as_str() {
        "words" => DEFAULT_WORD_COUNT,
        "sentences" => DEFAULT_SENTENCE_COUNT,
        "paragraphs" => DEFAULT_PARAGRAPH_COUNT,
        _ => return None,
    };
    let count = if count_str.trim().is_empty() {
        default
    } else {
        count_str.trim().parse::<usize>().ok()?
    };
    let count = count.max(1);
    if count > MAX_LOREM_COUNT {
        return None;
    }

    match kind.as_str() {
        "words" => Some(pick_words(count).join(" ")),
        "sentences" => Some(pick_sentences(count).join(" ")),
        "paragraphs" => Some(pick_paragraphs(count).join("\n\n")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentence_count(text: &str) -> usize {
        text.split_terminator('.')
            .filter(|part| !part.trim().is_empty())
            .count()
    }

    #[test]
    fn lorem_unified() {
        assert!(!resolve("").unwrap().is_empty()); // bare = paragraphs
        assert_eq!(resolve("3").unwrap().split("\n\n").count(), 3); // numeric → count
        assert_eq!(resolve("words, 5").unwrap().split_whitespace().count(), 5);
        assert_eq!(
            resolve("type=words, count=5")
                .unwrap()
                .split_whitespace()
                .count(),
            5
        );
        let sentences = resolve("sentences, 2").unwrap();
        assert_eq!(sentence_count(&sentences), 2);
        assert_eq!(resolve("nope"), None);
        assert_eq!(resolve("words, nope"), None);
    }

    #[test]
    fn lorem_count_cap() {
        assert!(resolve("words, 100000").is_some());
        assert_eq!(resolve("words, 100001"), None);
        assert_eq!(resolve("paragraphs, 100001"), None);
    }
}
