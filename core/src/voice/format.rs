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

    // 3. Fix spacing before and after punctuation
    for (re, replacement) in PUNCT_SPACE_FIXES.iter() {
        text = re.replace_all(&text, *replacement).to_string();
    }

    // 4. Capitalize start of sentences
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

    // 5. Capitalize personal pronoun "I" and contractions
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
}
