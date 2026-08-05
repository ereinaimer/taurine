use rand::RngExt;

const DEFAULT_WORD_COUNT: usize = 15;
const DEFAULT_SENTENCE_COUNT: usize = 1;
const DEFAULT_PARAGRAPH_COUNT: usize = 1;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoremInvocation {
    pub variant: LoremVariant,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoremVariant {
    Word,
    Sentence,
    Paragraph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoremParseError {
    InvalidRoot,
    InvalidVariant,
    MissingParentheses,
    UnbalancedParentheses,
    InvalidCount,
    InvalidTrailingSyntax,
}

pub(crate) fn parse_invocation(key: &str) -> Result<LoremInvocation, LoremParseError> {
    let rest = key
        .strip_prefix("lorem")
        .ok_or(LoremParseError::InvalidRoot)?;

    if rest.is_empty() {
        return Err(LoremParseError::InvalidVariant);
    }

    let modifier = rest
        .strip_prefix('.')
        .ok_or(LoremParseError::InvalidRoot)?
        .trim();

    let paren_idx = modifier
        .find('(')
        .ok_or(LoremParseError::MissingParentheses)?;
    let variant = modifier[..paren_idx].trim();
    let (args, trailing) = scan_parenthesized(&modifier[paren_idx..])?;
    if !trailing.trim().is_empty() {
        return Err(LoremParseError::InvalidTrailingSyntax);
    }

    let count = parse_count_arg(args)?;

    match variant {
        "word" => Ok(LoremInvocation {
            variant: LoremVariant::Word,
            count: count.unwrap_or(DEFAULT_WORD_COUNT),
        }),
        "sentence" => Ok(LoremInvocation {
            variant: LoremVariant::Sentence,
            count: count.unwrap_or(DEFAULT_SENTENCE_COUNT),
        }),
        "paragraph" => Ok(LoremInvocation {
            variant: LoremVariant::Paragraph,
            count: count.unwrap_or(DEFAULT_PARAGRAPH_COUNT),
        }),
        _ => Err(LoremParseError::InvalidVariant),
    }
}

pub fn resolve(key: &str) -> Option<String> {
    let invocation = parse_invocation(key).ok()?;
    let count = invocation.count.max(1);

    match invocation.variant {
        LoremVariant::Word => Some(pick_words(count).join(" ")),
        LoremVariant::Sentence => Some(pick_sentences(count).join(" ")),
        LoremVariant::Paragraph => Some(pick_paragraphs(count).join("\n\n")),
    }
}

fn scan_parenthesized(input: &str) -> Result<(&str, &str), LoremParseError> {
    if !input.starts_with('(') {
        return Err(LoremParseError::MissingParentheses);
    }

    let mut depth = 0usize;
    let mut start = None;

    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => {
                if depth == 0 {
                    start = Some(idx + ch.len_utf8());
                }
                depth += 1;
            }
            ')' => {
                if depth == 0 {
                    return Err(LoremParseError::UnbalancedParentheses);
                }
                depth -= 1;
                if depth == 0 {
                    let start = start.ok_or(LoremParseError::MissingParentheses)?;
                    return Ok((input[start..idx].trim(), &input[idx + 1..]));
                }
            }
            _ => {}
        }
    }

    Err(LoremParseError::UnbalancedParentheses)
}

fn parse_count_arg(args: &str) -> Result<Option<usize>, LoremParseError> {
    let trimmed = crate::engine::variables::system::strip_argument_quotes(args);
    if trimmed.is_empty() {
        return Ok(None);
    }

    if trimmed.contains(',') {
        return Err(LoremParseError::InvalidCount);
    }

    trimmed
        .parse::<usize>()
        .map(Some)
        .map_err(|_| LoremParseError::InvalidCount)
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
    fn rejects_bare_lorem_tag() {
        assert_eq!(resolve("lorem"), None);
    }

    #[test]
    fn resolves_words_with_exact_counts() {
        assert_eq!(
            resolve("lorem.word(1)").unwrap().split_whitespace().count(),
            1
        );
        assert_eq!(
            resolve("lorem.word(5)").unwrap().split_whitespace().count(),
            5
        );
        assert_eq!(
            resolve("lorem.word()").unwrap().split_whitespace().count(),
            DEFAULT_WORD_COUNT
        );
    }

    #[test]
    fn resolves_sentences_and_paragraphs_output_formatting() {
        let sentences = resolve("lorem.sentence(2)").unwrap();
        let paragraphs = resolve("lorem.paragraph(2)").unwrap();

        assert_eq!(sentence_count(&sentences), 2);
        assert!(!sentences.contains("\n\n"));
        assert_eq!(paragraphs.split("\n\n").count(), 2);
    }

    #[test]
    fn rejects_invalid_input_for_fallback() {
        assert_eq!(resolve("lorem.word"), None);
        assert_eq!(resolve("lorem.word(nope)"), None);
        assert_eq!(resolve("lorem.sentence(1, 2)"), None);
        assert_eq!(resolve("lorem.paragraph(1).upper"), None);
    }
}
