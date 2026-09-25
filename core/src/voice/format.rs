use regex::Regex;
use std::sync::LazyLock;

static SPOKEN_PUNCT: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (Regex::new(r"(?i)\bnew paragraph\b").unwrap(), "\n\n"),
        (Regex::new(r"(?i)\bnew line\b").unwrap(), "\n"),
        (Regex::new(r"(?i)\bnewline\b").unwrap(), "\n"),
        (Regex::new(r"(?i)\bperiod\b").unwrap(), "."),
        (Regex::new(r"(?i)\bfull stop\b").unwrap(), "."),
        (Regex::new(r"(?i)\bcomma\b").unwrap(), ","),
        (Regex::new(r"(?i)\bquestion mark\b").unwrap(), "?"),
        (Regex::new(r"(?i)\bexclamation point\b").unwrap(), "!"),
        (Regex::new(r"(?i)\bexclamation mark\b").unwrap(), "!"),
        (Regex::new(r"(?i)\bcolon\b").unwrap(), ":"),
        (Regex::new(r"(?i)\bsemicolon\b").unwrap(), ";"),
        (Regex::new(r"(?i)\bopen quote\b").unwrap(), "\""),
        (Regex::new(r"(?i)\bclose quote\b").unwrap(), "\""),
    ]
});

static FILLERS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)\bum\b").unwrap(),
        Regex::new(r"(?i)\buh\b").unwrap(),
        Regex::new(r"(?i)\bah\b").unwrap(),
    ]
});

static PUNCT_SPACE_FIXES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (Regex::new(r"\s+([,\.\?!;:])").unwrap(), "$1"),
        (
            Regex::new(r"([,\.\?!;:])([^\s\d\n,\.\?!;:])").unwrap(),
            "$1 $2",
        ),
        (Regex::new(r"[ \t]*\n[ \t]*").unwrap(), "\n"),
        (Regex::new(r"[ \t]{2,}").unwrap(), " "),
    ]
});

/// Words whose immediate doubling is kept as intentional emphasis
/// ("very very", "no no") rather than collapsed as stutter.
static EMPHASIS_ALLOWLIST: &[&str] = &[
    "very", "really", "so", "quite", "pretty", "highly", "deeply", "truly", "no", "nope", "nah",
    "yes", "yeah", "yep", "yup", "ha", "haha", "hello", "bye", "please",
];

/// Comparison key for one token: lowercase alphanumeric core, no punctuation.
fn word_key(token: &str) -> String {
    token
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// True when a token ends a sentence (repeats across it are never collapsed).
fn ends_sentence(token: &str) -> bool {
    matches!(token.chars().last(), Some('.' | '?' | '!'))
}

/// Collapse one newline-free segment: adjacent repeated phrases (longest first).
fn collapse_segment(segment: &str) -> String {
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    if tokens.len() < 2 {
        return segment.to_string();
    }
    let keys: Vec<String> = tokens.iter().map(|t| word_key(t)).collect();

    // Phase 1: immediately repeated phrases, n = 4..1.
    let mut kept: Vec<usize> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let mut width = 0;
        let mut run_end = 0;
        for n in (1..=4).rev() {
            if i + 2 * n > tokens.len() || keys[i..i + n].iter().any(|k| k.is_empty()) {
                continue;
            }
            if keys[i..i + n] != keys[i + n..i + 2 * n] {
                continue;
            }
            if n == 1 {
                if EMPHASIS_ALLOWLIST.contains(&keys[i].as_str()) || ends_sentence(tokens[i]) {
                    continue;
                }
                // Skip the whole run so triples collapse to one ("the the the" -> "the").
                run_end = i + 2;
                while run_end < tokens.len()
                    && keys[run_end] == keys[i]
                    && !ends_sentence(tokens[run_end - 1])
                {
                    run_end += 1;
                }
                break;
            }
            if tokens[i..i + n].iter().any(|t| ends_sentence(t)) {
                continue;
            }
            width = n;
            break;
        }
        if run_end > 0 {
            kept.push(i);
            i = run_end;
        } else if width > 0 {
            kept.extend(i..i + width);
            i += 2 * width;
        } else {
            kept.push(i);
            i += 1;
        }
    }

    // Phase 2: false-start repair ("how are [how are these] how are you"):
    // a 2-gram repeating within 3 tokens drops the earlier fragment.
    // honey: window of 3 covers close restarts only; widen if dictation logs show longer repairs.
    let mut j = 0;
    while j + 1 < kept.len() {
        let mut dropped = false;
        for k in (j + 1)..=(j + 3).min(kept.len().saturating_sub(2)) {
            if keys[kept[j]].is_empty() || keys[kept[j + 1]].is_empty() {
                break;
            }
            // Emphasis doubles never trigger a repair ("no no no" stays).
            if keys[kept[j]] == keys[kept[j + 1]]
                && EMPHASIS_ALLOWLIST.contains(&keys[kept[j]].as_str())
            {
                break;
            }
            if keys[kept[j]] == keys[kept[k]]
                && keys[kept[j + 1]] == keys[kept[k + 1]]
                && !kept[j..k].iter().any(|&t| ends_sentence(tokens[t]))
            {
                kept.drain(j..k);
                dropped = true;
                break;
            }
        }
        if !dropped {
            j += 1;
        }
    }

    kept.iter()
        .map(|&t| tokens[t])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Collapse hesitation repeats and false starts, one line at a time so
/// paragraph breaks from spoken punctuation are never merged.
fn collapse_repeated_phrases(text: &str) -> String {
    text.split('\n')
        .map(collapse_segment)
        .collect::<Vec<_>>()
        .join("\n")
}

static I_FIXES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (Regex::new(r"\bi\b").unwrap(), "I"),
        (Regex::new(r"\bi'm\b").unwrap(), "I'm"),
        (Regex::new(r"\bi've\b").unwrap(), "I've"),
        (Regex::new(r"\bi'll\b").unwrap(), "I'll"),
        (Regex::new(r"\bi'd\b").unwrap(), "I'd"),
    ]
});

/// Formats a raw speech recognition transcript into clean, human-readable text:
/// - Replaces spoken punctuation commands ("period", "comma", "question mark", etc.)
/// - Removes verbal disfluencies ("um", "uh", "ah")
/// - Collapses hesitation repeats and false starts ("how are how are" -> "how are")
/// - Normalizes punctuation spacing
/// - Applies sentence capitalization and personal pronoun capitalization
pub fn format_transcript(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut text = trimmed.to_string();

    // 1. Spoken punctuation replacements
    for (re, replacement) in SPOKEN_PUNCT.iter() {
        text = re.replace_all(&text, *replacement).to_string();
    }

    // 2. Remove filler words
    for re in FILLERS.iter() {
        text = re.replace_all(&text, "").to_string();
    }

    // 3. Collapse repeated phrases and false starts
    text = collapse_repeated_phrases(&text);

    // 4. Fix spacing before and after punctuation
    for (re, replacement) in PUNCT_SPACE_FIXES.iter() {
        text = re.replace_all(&text, *replacement).to_string();
    }

    // 5. Capitalize start of sentences
    let mut chars: Vec<char> = text.chars().collect();
    let mut capitalize_next = true;

    for c in &mut chars {
        if c.is_alphabetic() {
            if capitalize_next {
                *c = c.to_ascii_uppercase();
                capitalize_next = false;
            }
        } else if *c == '.' || *c == '?' || *c == '!' || *c == '\n' {
            capitalize_next = true;
        }
    }

    let mut result: String = chars.into_iter().collect();

    // 6. Capitalize personal pronoun "I" and contractions
    for (re, replacement) in I_FIXES.iter() {
        result = re.replace_all(&result, *replacement).to_string();
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_spoken_punctuation() {
        let input = "hello world period how are you question mark";
        let formatted = format_transcript(input);
        assert_eq!(formatted, "Hello world. How are you?");
    }

    #[test]
    fn test_format_fillers_and_pronouns() {
        let input = "um i think uh i'm going to win exclamation mark";
        let formatted = format_transcript(input);
        assert_eq!(formatted, "I think I'm going to win!");
    }

    #[test]
    fn test_format_newlines() {
        let input = "first line new line second line";
        let formatted = format_transcript(input);
        assert_eq!(formatted, "First line\nSecond line");
    }

    #[test]
    fn test_format_collapses_stutter_repeats() {
        assert_eq!(
            format_transcript("went to to the store"),
            "Went to the store"
        );
        assert_eq!(
            format_transcript("how are how are you today"),
            "How are you today"
        );
        assert_eq!(format_transcript("the the the dog ran"), "The dog ran");
    }

    #[test]
    fn test_format_keeps_emphasis_doubles() {
        assert_eq!(
            format_transcript("that was very very good"),
            "That was very very good"
        );
        assert_eq!(
            format_transcript("no no no do not go"),
            "No no no do not go"
        );
    }

    #[test]
    fn test_format_trims_false_start_repair() {
        assert_eq!(
            format_transcript("hello how are how are these how are you"),
            "Hello how are you"
        );
    }

    #[test]
    fn test_format_keeps_repeats_across_sentences() {
        assert_eq!(
            format_transcript("hello period hello period"),
            "Hello. Hello."
        );
    }
}
