use regex::Regex;

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
    let spoken_punct = [
        (r"(?i)\bnew paragraph\b", "\n\n"),
        (r"(?i)\bnew line\b", "\n"),
        (r"(?i)\bnewline\b", "\n"),
        (r"(?i)\bperiod\b", "."),
        (r"(?i)\bfull stop\b", "."),
        (r"(?i)\bcomma\b", ","),
        (r"(?i)\bquestion mark\b", "?"),
        (r"(?i)\bexclamation point\b", "!"),
        (r"(?i)\bexclamation mark\b", "!"),
        (r"(?i)\bcolon\b", ":"),
        (r"(?i)\bsemicolon\b", ";"),
        (r"(?i)\bopen quote\b", "\""),
        (r"(?i)\bclose quote\b", "\""),
    ];

    for (pattern, replacement) in spoken_punct {
        if let Ok(re) = Regex::new(pattern) {
            text = re.replace_all(&text, replacement).to_string();
        }
    }

    // 2. Remove filler words
    let fillers = [r"(?i)\bum\b", r"(?i)\buh\b", r"(?i)\bah\b"];
    for pattern in fillers {
        if let Ok(re) = Regex::new(pattern) {
            text = re.replace_all(&text, "").to_string();
        }
    }

    // 3. Fix spacing before and after punctuation
    let punct_space_fixes = [
        (r"\s+([,\.\?!;:])", "$1"),
        (r"([,\.\?!;:])([^\s\d\n,\.\?!;:])", "$1 $2"),
        (r"[ \t]*\n[ \t]*", "\n"),
        (r"[ \t]{2,}", " "),
    ];
    for (pattern, replacement) in punct_space_fixes {
        if let Ok(re) = Regex::new(pattern) {
            text = re.replace_all(&text, replacement).to_string();
        }
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
    let i_fixes = [
        (r"\bi\b", "I"),
        (r"\bi'm\b", "I'm"),
        (r"\bi've\b", "I've"),
        (r"\bi'll\b", "I'll"),
        (r"\bi'd\b", "I'd"),
    ];
    for (pattern, replacement) in i_fixes {
        if let Ok(re) = Regex::new(pattern) {
            result = re.replace_all(&result, replacement).to_string();
        }
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
