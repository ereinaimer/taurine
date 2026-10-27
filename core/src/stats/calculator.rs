/// Logic for calculating stats based on expansion events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExpansionStats {
    pub words_count: usize,
    pub time_saved_ms: i64,
}

/// Counts words in `text` using standard whitespace splitting as a baseline,
/// with a character-length fallback formula for long unspaced tokens
/// (such as URLs, email addresses, or file paths) using the standard
/// typing ratio of 5 characters per word.
pub fn count_words(text: &str) -> usize {
    let mut total_words = 0;
    for token in text.split_whitespace() {
        let char_count = token.chars().count();
        if char_count == 0 {
            continue;
        }
        // Baseline is 1 word per whitespace token.
        // For long unspaced tokens (e.g. emails, URLs, file paths), fall back
        // to standard typing ratio of 5 characters per word.
        let token_words = (char_count / 5).max(1);
        total_words += token_words;
    }
    total_words
}

/// Estimates word count from character count when raw text is not preserved.
/// Uses the standard typing ratio of 5 characters per word.
pub fn fallback_words_from_chars(char_count: usize) -> usize {
    if char_count == 0 {
        0
    } else {
        (char_count / 5).max(1)
    }
}

/// Calculates time saved using a five-characters-per-word typing model.
///
/// Time saved is computed as `(words * 60_000) / WPM`, capped at 5 minutes (300,000 ms).
pub fn calculate_time_saved_ms(words: usize, wpm: u32) -> i64 {
    if words == 0 {
        return 0;
    }

    let effective_wpm = u64::from(crate::settings::Settings::sanitize_wpm(wpm));
    if effective_wpm == 0 {
        return 0;
    }

    let words_u64 = words as u64;
    let time_saved_ms = ((words_u64.saturating_mul(60_000)) / effective_wpm) as i64;
    time_saved_ms.min(300_000)
}

pub fn calculate_expansion_stats(words_count: usize, wpm: u32) -> ExpansionStats {
    let time_saved_ms = calculate_time_saved_ms(words_count, wpm);

    ExpansionStats {
        words_count,
        time_saved_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_words_empty_and_whitespace() {
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("   \t\n\r  "), 0);
    }

    #[test]
    fn test_count_words_standard_text() {
        assert_eq!(count_words("hello"), 1);
        assert_eq!(count_words("hello world"), 2);
        assert_eq!(
            count_words("The quick brown fox jumps over the lazy dog"),
            9
        );
    }

    #[test]
    fn test_count_words_unspaced_tokens() {
        // Standard typing ratio: 5 characters per word.
        // Email: 20 chars -> 20 / 5 = 4 words
        assert_eq!(count_words("user@example.com"), 3); // 16 chars -> 16 / 5 = 3
        // URL: 45 chars -> 45 / 5 = 9 words
        assert_eq!(
            count_words("https://taurine.dev/docs/getting-started/path"),
            9
        );
        // Mixed text with unspaced link
        let mixed = "Check out https://github.com/taurine/taurine-repo for more details";
        // "Check" (1) + "out" (1) + URL (39 chars -> 7) + "for" (1) + "more" (1) + "details" (1) = 12
        assert_eq!(count_words(mixed), 12);
    }

    #[test]
    fn test_fallback_words_from_chars() {
        assert_eq!(fallback_words_from_chars(0), 0);
        assert_eq!(fallback_words_from_chars(3), 1);
        assert_eq!(fallback_words_from_chars(5), 1);
        assert_eq!(fallback_words_from_chars(25), 5);
    }

    #[test]
    fn time_saved_uses_wpm() {
        let fast = calculate_time_saved_ms(60, 60);
        let slow = calculate_time_saved_ms(60, 30);
        assert!(slow > fast);
        assert_eq!(fast, 60_000);
        assert_eq!(slow, 120_000);
    }

    #[test]
    fn time_saved_caps_at_five_minutes() {
        // At 60 WPM, 10,000 words would take massive time, but should cap at 5 mins (300,000 ms)
        assert_eq!(calculate_time_saved_ms(10_000, 60), 300_000);
    }

    #[test]
    fn zero_words_have_zero_time_saved() {
        assert_eq!(calculate_time_saved_ms(0, 60), 0);
    }

    #[test]
    fn zero_wpm_falls_back_to_default() {
        assert_eq!(
            calculate_time_saved_ms(60, 0),
            calculate_time_saved_ms(60, crate::settings::Settings::default_wpm())
        );
    }
}
