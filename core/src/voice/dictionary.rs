use strsim::normalized_damerau_levenshtein;

/// Smart personal dictionary for phonetic and fuzzy corrections of domain terms.
#[derive(Debug, Clone, Default)]
pub struct VoiceDictionary {
    terms: Vec<String>,
}

impl VoiceDictionary {
    /// Parse comma-separated dictionary terms (e.g. "Taurine, PostgreSQL, Kubernetes, API").
    pub fn from_csv(csv: &str) -> Self {
        let terms: Vec<String> = csv
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        Self { terms }
    }

    /// Check if dictionary has zero entries.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Apply dictionary corrections to input text.
    pub fn apply(&self, text: &str) -> String {
        if self.terms.is_empty() || text.is_empty() {
            return text.to_string();
        }

        let words: Vec<&str> = text.split_whitespace().collect();
        let mut corrected_words = Vec::with_capacity(words.len());

        for word in words {
            // Strip leading/trailing punctuation
            let leading_punct: String = word.chars().take_while(|c| !c.is_alphanumeric()).collect();
            let trailing_punct: String = word
                .chars()
                .rev()
                .take_while(|c| !c.is_alphanumeric())
                .collect::<String>()
                .chars()
                .rev()
                .collect();

            let core_end = word.len() - trailing_punct.len();
            if leading_punct.len() >= core_end {
                corrected_words.push(word.to_string());
                continue;
            }

            let core = &word[leading_punct.len()..core_end];
            let core_lower = core.to_lowercase();

            let mut best_match: Option<&str> = None;
            let mut best_score = 0.0;

            for term in &self.terms {
                let term_lower = term.to_lowercase();
                if core_lower == term_lower {
                    best_match = Some(term);
                    break;
                }

                let score = normalized_damerau_levenshtein(&core_lower, &term_lower);
                if score >= 0.82 && score > best_score {
                    best_score = score;
                    best_match = Some(term);
                }
            }

            if let Some(matched) = best_match {
                corrected_words.push(format!("{leading_punct}{matched}{trailing_punct}"));
            } else {
                corrected_words.push(word.to_string());
            }
        }

        corrected_words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_exact_and_fuzzy_matches() {
        let dict = VoiceDictionary::from_csv("Taurine, PostgreSQL, Kubernetes, API");

        // Exact case correction
        let res = dict.apply("welcome to taurine today");
        assert_eq!(res, "welcome to Taurine today");

        // Punctuation preservation
        let res_punct = dict.apply("use taurine, right now.");
        assert_eq!(res_punct, "use Taurine, right now.");

        // Fuzzy typo correction
        let res_fuzzy = dict.apply("deploy to kubernets cluster");
        assert_eq!(res_fuzzy, "deploy to Kubernetes cluster");

        // Non-matching words untouched
        let res_untouched = dict.apply("hello world");
        assert_eq!(res_untouched, "hello world");
    }
}
