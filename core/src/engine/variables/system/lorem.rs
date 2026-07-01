use fake::Fake;
use fake::faker::lorem::en::{Paragraphs, Sentences, Words};

const DEFAULT_WORD_COUNT: usize = 15;
const DEFAULT_SENTENCE_COUNT: usize = 1;
const DEFAULT_PARAGRAPH_COUNT: usize = 1;

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
    let end = invocation.count.checked_add(1)?;

    match invocation.variant {
        LoremVariant::Paragraph => Some(
            Paragraphs(invocation.count..end)
                .fake::<Vec<String>>()
                .join("\n\n"),
        ),
        LoremVariant::Word => Some(Words(invocation.count..end).fake::<Vec<String>>().join(" ")),
        LoremVariant::Sentence => Some(
            Sentences(invocation.count..end)
                .fake::<Vec<String>>()
                .join(" "),
        ),
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
    let trimmed = args.trim();
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
