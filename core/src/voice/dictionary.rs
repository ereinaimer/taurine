use strsim::normalized_damerau_levenshtein;

fn is_number_word(w: &str) -> bool {
    matches!(
        w,
        "zero"
            | "one"
            | "two"
            | "three"
            | "four"
            | "five"
            | "six"
            | "seven"
            | "eight"
            | "nine"
            | "ten"
            | "eleven"
            | "twelve"
            | "thirteen"
            | "fourteen"
            | "fifteen"
            | "sixteen"
            | "seventeen"
            | "eighteen"
            | "nineteen"
            | "twenty"
            | "thirty"
            | "forty"
            | "fifty"
            | "sixty"
            | "seventy"
            | "eighty"
            | "ninety"
            | "hundred"
            | "thousand"
            | "million"
            | "billion"
            | "first"
            | "second"
            | "third"
            | "fourth"
            | "fifth"
            | "sixth"
            | "seventh"
            | "eighth"
            | "ninth"
            | "tenth"
    )
}

fn is_basic_stop_word(w: &str) -> bool {
    matches!(
        w,
        "a" | "about"
            | "above"
            | "across"
            | "after"
            | "again"
            | "against"
            | "all"
            | "almost"
            | "alone"
            | "along"
            | "already"
            | "also"
            | "although"
            | "always"
            | "am"
            | "among"
            | "an"
            | "and"
            | "another"
            | "any"
            | "anybody"
            | "anyone"
            | "anything"
            | "anywhere"
            | "are"
            | "area"
            | "arent"
            | "around"
            | "as"
            | "ask"
            | "asked"
            | "asking"
            | "asks"
            | "at"
            | "away"
            | "back"
            | "be"
            | "became"
            | "because"
            | "become"
            | "becomes"
            | "been"
            | "before"
            | "began"
            | "begin"
            | "behind"
            | "being"
            | "believe"
            | "below"
            | "best"
            | "better"
            | "between"
            | "big"
            | "both"
            | "bring"
            | "brings"
            | "brought"
            | "but"
            | "by"
            | "call"
            | "called"
            | "calling"
            | "calls"
            | "came"
            | "can"
            | "cannot"
            | "cant"
            | "case"
            | "cases"
            | "certain"
            | "change"
            | "changed"
            | "changes"
            | "clear"
            | "clearly"
            | "close"
            | "come"
            | "comes"
            | "coming"
            | "could"
            | "couldnt"
            | "day"
            | "days"
            | "did"
            | "didnt"
            | "differ"
            | "different"
            | "do"
            | "does"
            | "doesnt"
            | "doing"
            | "done"
            | "dont"
            | "down"
            | "draw"
            | "drawing"
            | "drawn"
            | "draws"
            | "dream"
            | "dreamed"
            | "dreaming"
            | "dreams"
            | "dreamt"
            | "drew"
            | "drink"
            | "drinking"
            | "drinks"
            | "drank"
            | "drive"
            | "driven"
            | "driver"
            | "drivers"
            | "drives"
            | "driving"
            | "drop"
            | "dropped"
            | "dropping"
            | "drops"
            | "drove"
            | "drunk"
            | "dry"
            | "dried"
            | "dries"
            | "drying"
            | "during"
            | "each"
            | "early"
            | "end"
            | "ended"
            | "enough"
            | "even"
            | "ever"
            | "every"
            | "everybody"
            | "everyone"
            | "everything"
            | "everywhere"
            | "fact"
            | "far"
            | "feel"
            | "few"
            | "find"
            | "first"
            | "five"
            | "for"
            | "form"
            | "four"
            | "from"
            | "full"
            | "further"
            | "gave"
            | "general"
            | "get"
            | "gets"
            | "getting"
            | "give"
            | "given"
            | "gives"
            | "giving"
            | "go"
            | "goes"
            | "going"
            | "gone"
            | "good"
            | "got"
            | "great"
            | "group"
            | "had"
            | "hadnt"
            | "has"
            | "hasnt"
            | "have"
            | "havent"
            | "having"
            | "he"
            | "help"
            | "helped"
            | "her"
            | "here"
            | "heres"
            | "hers"
            | "herself"
            | "hes"
            | "high"
            | "higher"
            | "highest"
            | "him"
            | "himself"
            | "his"
            | "hold"
            | "home"
            | "how"
            | "hows"
            | "however"
            | "i"
            | "id"
            | "if"
            | "ill"
            | "im"
            | "important"
            | "in"
            | "into"
            | "is"
            | "isnt"
            | "it"
            | "its"
            | "itself"
            | "ive"
            | "just"
            | "keep"
            | "keeps"
            | "kept"
            | "kind"
            | "knew"
            | "know"
            | "known"
            | "knows"
            | "large"
            | "last"
            | "late"
            | "later"
            | "least"
            | "leave"
            | "leaves"
            | "left"
            | "less"
            | "let"
            | "lets"
            | "like"
            | "likely"
            | "line"
            | "little"
            | "long"
            | "longer"
            | "look"
            | "looked"
            | "looking"
            | "looks"
            | "made"
            | "make"
            | "makes"
            | "making"
            | "man"
            | "many"
            | "may"
            | "maybe"
            | "me"
            | "mean"
            | "means"
            | "meant"
            | "men"
            | "might"
            | "more"
            | "most"
            | "mostly"
            | "move"
            | "moved"
            | "mr"
            | "mrs"
            | "much"
            | "must"
            | "my"
            | "myself"
            | "name"
            | "near"
            | "need"
            | "needed"
            | "needs"
            | "never"
            | "new"
            | "next"
            | "no"
            | "nobody"
            | "none"
            | "noone"
            | "nor"
            | "not"
            | "nothing"
            | "now"
            | "number"
            | "of"
            | "off"
            | "often"
            | "old"
            | "on"
            | "once"
            | "one"
            | "only"
            | "open"
            | "or"
            | "order"
            | "other"
            | "others"
            | "our"
            | "ours"
            | "ourselves"
            | "out"
            | "over"
            | "own"
            | "part"
            | "people"
            | "per"
            | "place"
            | "point"
            | "possible"
            | "present"
            | "problem"
            | "problems"
            | "put"
            | "puts"
            | "quite"
            | "rather"
            | "read"
            | "real"
            | "really"
            | "right"
            | "room"
            | "said"
            | "same"
            | "saw"
            | "say"
            | "saying"
            | "says"
            | "second"
            | "see"
            | "seem"
            | "seemed"
            | "seeming"
            | "seems"
            | "seen"
            | "sees"
            | "several"
            | "shall"
            | "she"
            | "shes"
            | "should"
            | "shouldnt"
            | "show"
            | "showed"
            | "shown"
            | "shows"
            | "side"
            | "since"
            | "six"
            | "small"
            | "so"
            | "solve"
            | "solved"
            | "some"
            | "somebody"
            | "someone"
            | "something"
            | "somewhere"
            | "state"
            | "states"
            | "still"
            | "such"
            | "sure"
            | "take"
            | "taken"
            | "takes"
            | "taking"
            | "tell"
            | "telling"
            | "tells"
            | "test"
            | "tested"
            | "testing"
            | "than"
            | "that"
            | "thats"
            | "the"
            | "their"
            | "theirs"
            | "them"
            | "themselves"
            | "then"
            | "there"
            | "theres"
            | "therefore"
            | "these"
            | "they"
            | "theyd"
            | "theyll"
            | "theyre"
            | "theyve"
            | "thing"
            | "things"
            | "think"
            | "thinking"
            | "thinks"
            | "this"
            | "those"
            | "though"
            | "thought"
            | "three"
            | "through"
            | "thus"
            | "time"
            | "to"
            | "today"
            | "together"
            | "told"
            | "too"
            | "took"
            | "toward"
            | "turn"
            | "turned"
            | "two"
            | "under"
            | "until"
            | "up"
            | "upon"
            | "us"
            | "use"
            | "used"
            | "uses"
            | "using"
            | "very"
            | "want"
            | "wanted"
            | "wants"
            | "was"
            | "wasnt"
            | "way"
            | "ways"
            | "we"
            | "well"
            | "went"
            | "were"
            | "werent"
            | "what"
            | "whats"
            | "when"
            | "where"
            | "wheres"
            | "whether"
            | "which"
            | "while"
            | "who"
            | "whole"
            | "whom"
            | "whose"
            | "why"
            | "will"
            | "with"
            | "within"
            | "without"
            | "won"
            | "wont"
            | "work"
            | "worked"
            | "working"
            | "works"
            | "would"
            | "wouldnt"
            | "year"
            | "years"
            | "yes"
            | "yet"
            | "you"
            | "youd"
            | "youll"
            | "young"
            | "your"
            | "youre"
            | "yours"
            | "yourself"
            | "yourselves"
            | "youve"
    )
}

/// Returns true if a lowercase ASCII word is a common English functional/stop word,
/// a number, or an authentic English word in Taurine's offline lexicon.
///
/// Phonetic sound-alike matching (Pass 3) must never hijack valid English
/// words or standard numbers (e.g. "this" -> "Tether", "thirty" -> "Taurine",
/// "train" -> "Taurine", "earn" -> "Erein").
pub fn is_common_word(word: &str) -> bool {
    let check = |w: &str| {
        is_number_word(w)
            || is_basic_stop_word(w)
            || crate::engine::dictionary::offline::is_offline_word(w)
    };
    if check(word) {
        return true;
    }
    if word.contains('\'') || word.contains('’') {
        let stripped: String = word.chars().filter(|c| *c != '\'' && *c != '’').collect();
        return check(&stripped);
    }
    false
}

/// Splits a word into `(leading_punct, core, trailing_punct)`.
/// Correctly and safely handles Unicode character boundaries.
fn split_word_punct(word: &str) -> (&str, &str, &str) {
    let mut first_alnum = None;
    let mut last_alnum_end = 0;

    for (idx, ch) in word.char_indices() {
        if ch.is_alphanumeric() {
            if first_alnum.is_none() {
                first_alnum = Some(idx);
            }
            last_alnum_end = idx + ch.len_utf8();
        }
    }

    match first_alnum {
        Some(start) => (
            &word[..start],
            &word[start..last_alnum_end],
            &word[last_alnum_end..],
        ),
        None => (word, "", ""),
    }
}

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

    /// Return all configured dictionary terms.
    pub fn terms(&self) -> &[String] {
        &self.terms
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

        // Single-word phonetic representations for Pass 3
        let term_phonetics: Vec<(&String, String, String)> = self
            .terms
            .iter()
            .filter(|t| {
                !t.contains(' ') && t.len() >= 3 && !t.chars().all(|c| c.is_ascii_uppercase())
            })
            .map(|t| {
                let (primary, alt) = crate::voice::phonetic::double_metaphone(&t.to_lowercase());
                (t, primary, alt)
            })
            .filter(|(_, p, _)| p.len() >= 2)
            .collect();

        // 2-word phonetic splits for syllable-split acoustic artifacts (e.g. "draw in" -> "Taurine")
        let mut term_splits = Vec::new();
        for term in &self.terms {
            if term.contains(' ') || term.len() < 4 || term.chars().all(|c| c.is_ascii_uppercase())
            {
                continue;
            }
            let term_lower = term.to_lowercase();
            // Test char boundary cuts between prefix and suffix
            for (idx, _) in term_lower.char_indices().skip(1) {
                if idx >= 2 && term_lower.len().saturating_sub(idx) >= 2 {
                    let prefix = &term_lower[..idx];
                    let suffix = &term_lower[idx..];
                    let (p_pre, _) = crate::voice::phonetic::double_metaphone(prefix);
                    let (p_suf, _) = crate::voice::phonetic::double_metaphone(suffix);
                    if p_pre.len() >= 2 && p_suf.len() >= 2 {
                        term_splits.push((term.as_str(), p_pre, p_suf));
                    }
                }
            }
        }

        let words: Vec<&str> = text.split_whitespace().collect();
        let mut corrected_words = Vec::with_capacity(words.len());

        let mut i = 0;
        while i < words.len() {
            let (lead1, core1, trail1) = split_word_punct(words[i]);

            // Attempt 2-word sliding window match if there is a consecutive word without internal punctuation
            if i + 1 < words.len() && trail1.is_empty() {
                let (lead2, core2, trail2) = split_word_punct(words[i + 1]);
                if lead2.is_empty() && !core1.is_empty() && !core2.is_empty() {
                    let core1_lower = core1.to_lowercase();
                    let core2_lower = core2.to_lowercase();
                    let combined_concat = format!("{core1_lower}{core2_lower}");
                    let combined_spaced = format!("{core1_lower} {core2_lower}");

                    let mut matched_2word: Option<&str> = None;

                    // 1. Exact match against multi-word or compound terms
                    for term in &self.terms {
                        let term_lower = term.to_lowercase();
                        if combined_concat == term_lower || combined_spaced == term_lower {
                            matched_2word = Some(term);
                            break;
                        }
                    }

                    // 2. High-confidence fuzzy match on concatenated compound
                    if matched_2word.is_none() && !is_common_word(&combined_concat) {
                        for term in &self.terms {
                            if !term.contains(' ') {
                                let term_lower = term.to_lowercase();
                                let score =
                                    normalized_damerau_levenshtein(&combined_concat, &term_lower);
                                if score >= 0.85 {
                                    matched_2word = Some(term);
                                    break;
                                }
                            }
                        }
                    }

                    // 3. Syllable-split phonetic matching (e.g. "draw in" -> "taur" + "ine" -> "Taurine")
                    if matched_2word.is_none() {
                        let (c1_phone, _) = crate::voice::phonetic::double_metaphone(&core1_lower);
                        let (c2_phone, _) = crate::voice::phonetic::double_metaphone(&core2_lower);
                        if c1_phone.len() >= 2 && c2_phone.len() >= 2 {
                            for (term, p_pre, p_suf) in &term_splits {
                                if c1_phone == *p_pre && c2_phone == *p_suf {
                                    matched_2word = Some(*term);
                                    break;
                                }
                            }
                        }
                    }

                    if let Some(matched) = matched_2word {
                        corrected_words.push(format!("{lead1}{matched}{trail2}"));
                        i += 2;
                        continue;
                    }
                }
            }

            // Single word processing
            if core1.is_empty() {
                corrected_words.push(words[i].to_string());
                i += 1;
                continue;
            }

            let core_lower = core1.to_lowercase();
            let mut best_match: Option<&str> = None;
            let mut best_score = 0.0;

            for term in &self.terms {
                let term_lower = term.to_lowercase();
                if core_lower == term_lower {
                    best_match = Some(term);
                    break;
                }

                let score = normalized_damerau_levenshtein(&core_lower, &term_lower);
                if score >= 0.85 && score > best_score && !is_common_word(&core_lower) {
                    best_score = score;
                    best_match = Some(term);
                }
            }

            // Pass 3: Strict phonetic sound-alike matching (Double Metaphone).
            // Industry standard: maps acoustic transcription artifacts (e.g. "Theoring", "Toring", "Darin", "Dorin", "towerrent")
            // to configured dictionary words ("Taurine") while completely protecting standard English words.
            if best_match.is_none() && !is_common_word(&core_lower) && core_lower.len() >= 3 {
                let (spoken_primary, spoken_alt) =
                    crate::voice::phonetic::double_metaphone(&core_lower);
                if spoken_primary.len() >= 2 {
                    let mut best_sim = -1.0;
                    for (term, term_primary, term_alt) in &term_phonetics {
                        let matches = *term_primary == spoken_primary
                            || (!term_alt.is_empty() && *term_alt == spoken_primary)
                            || (!spoken_alt.is_empty() && *term_primary == spoken_alt)
                            || (!term_alt.is_empty()
                                && !spoken_alt.is_empty()
                                && *term_alt == spoken_alt)
                            || spoken_primary.strip_suffix('T') == Some(term_primary.as_str())
                            || spoken_primary.strip_suffix('K') == Some(term_primary.as_str());
                        if matches {
                            let sim =
                                normalized_damerau_levenshtein(&core_lower, &term.to_lowercase());
                            if sim > best_sim {
                                best_sim = sim;
                                best_match = Some(term.as_str());
                            }
                        }
                    }
                }
            }

            if let Some(matched) = best_match {
                corrected_words.push(format!("{lead1}{matched}{trail1}"));
            } else {
                corrected_words.push(words[i].to_string());
            }

            i += 1;
        }

        corrected_words.join(" ")
    }
}

/// Combines active Voice Trigger phrases and personal dictionary terms into a
/// `/`-separated hotwords string for decoder biasing, capped at 30 items with
/// trigger phrases taking precedence.
pub fn build_hotwords_payload(
    dict: &VoiceDictionary,
    trigger_phrases: &[impl AsRef<str>],
) -> Option<String> {
    let mut collected: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Voice triggers first (highest priority)
    for trigger in trigger_phrases {
        let trimmed = trigger.as_ref().trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_lowercase()) {
            collected.push(trimmed.to_string());
            if collected.len() >= 30 {
                break;
            }
        }
    }

    // 2. Personal dictionary terms second
    if collected.len() < 30 {
        for term in dict.terms() {
            let trimmed = term.trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_lowercase()) {
                collected.push(trimmed.to_string());
                if collected.len() >= 30 {
                    break;
                }
            }
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(collected.join("/"))
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

    #[test]
    fn test_dictionary_unicode_punctuation() {
        let dict = VoiceDictionary::from_csv("Taurine");
        // Em-dash is 3 UTF-8 bytes: —
        let res = dict.apply("—taurine—");
        assert_eq!(res, "—Taurine—");

        // Smart quotes: “taurine”
        let res_quotes = dict.apply("“taurine”");
        assert_eq!(res_quotes, "“Taurine”");
    }

    #[test]
    fn test_dictionary_preserves_english_words_and_numbers() {
        let dict = VoiceDictionary::from_csv("Tether, Taurine, Kubernetes, API");

        // Real numbers and common English words must NEVER be mutated
        assert_eq!(dict.apply("thirty two"), "thirty two");
        assert_eq!(
            dict.apply("I'm testing whether this works"),
            "I'm testing whether this works"
        );
        assert_eq!(
            dict.apply("How can I solve this problem"),
            "How can I solve this problem"
        );
        assert_eq!(
            dict.apply("Is there anything we can do"),
            "Is there anything we can do"
        );
        assert_eq!(
            dict.apply("more accurate than one word"),
            "more accurate than one word"
        );

        // Domain terms with proper casing are applied accurately
        assert_eq!(
            dict.apply("taurine is a text expander"),
            "Taurine is a text expander"
        );
        assert_eq!(
            dict.apply("connect to kubernets via api"),
            "connect to Kubernetes via API"
        );
    }

    #[test]
    fn test_dictionary_phonetic_sound_alikes_and_lexicon_immunity() {
        let dict = VoiceDictionary::from_csv("Taurine");

        // Real-world user failure cases:
        // 1. Syllable split: "draw in" -> "Taurine"
        assert_eq!(
            dict.apply("Switch to the draw in folder."),
            "Switch to the Taurine folder."
        );
        // 2. Terminal -ing folding: "Theoring" -> "Taurine"
        assert_eq!(
            dict.apply("Theoring is the best fix matter in the world."),
            "Taurine is the best fix matter in the world."
        );
        // 3. Terminal -ing folding: "Toring" -> "Taurine"
        assert_eq!(
            dict.apply("Toring is the best text expandnder in the world."),
            "Taurine is the best text expandnder in the world."
        );
        // 4. Proper name vs common lexicon: "Darin" -> "Taurine"
        assert_eq!(
            dict.apply("Darin is the best extxt spender in the world."),
            "Taurine is the best extxt spender in the world."
        );
        // 5. Syllable split: "Tor in" -> "Taurine"
        assert_eq!(
            dict.apply("Switch to the Tor in folder"),
            "Switch to the Taurine folder"
        );
        // 6. Excrescent stop on non-word: "towerrent" -> "Taurine"
        assert_eq!(
            dict.apply("Switch to the towerrent folder"),
            "Switch to the Taurine folder"
        );

        // Additional transcription sound-alikes for "Taurine"
        assert_eq!(
            dict.apply("Dorin is the best text expander in the world"),
            "Taurine is the best text expander in the world"
        );
        assert_eq!(
            dict.apply("Dorren is the best text expander in the world"),
            "Taurine is the best text expander in the world"
        );
        assert_eq!(
            dict.apply("Torin is the best text expander in the world"),
            "Taurine is the best text expander in the world"
        );
        assert_eq!(
            dict.apply("Doreen is the best text expander in the world"),
            "Taurine is the best text expander in the world"
        );

        // Standard English words sharing phonetic sounds must NEVER be corrupted
        assert_eq!(dict.apply("I took a train today"), "I took a train today");
        assert_eq!(dict.apply("drain the water"), "drain the water");
        assert_eq!(dict.apply("the page was torn"), "the page was torn");
        assert_eq!(dict.apply("a thorn in the side"), "a thorn in the side");
        assert_eq!(dict.apply("rough terrain ahead"), "rough terrain ahead");
        assert_eq!(dict.apply("thirty two"), "thirty two");
        assert_eq!(dict.apply("run torrent now"), "run torrent now");
        assert_eq!(dict.apply("I want to earn money"), "I want to earn money");
        assert_eq!(dict.apply("cast iron skillet"), "cast iron skillet");
    }

    #[test]
    fn test_build_hotwords_payload_aggregation_and_capping() {
        let dict = VoiceDictionary::from_csv("Taurine, Kubernetes, API, movies folder");
        let triggers = vec!["movies folder".to_string(), "open terminal".to_string()];

        let payload = build_hotwords_payload(&dict, &triggers).expect("payload must be some");
        let parts: Vec<&str> = payload.split('/').collect();

        // Triggers come first: "movies folder", "open terminal"
        assert_eq!(parts[0], "movies folder");
        assert_eq!(parts[1], "open terminal");
        // "movies folder" in dict was deduplicated with trigger
        assert_eq!(parts[2], "Taurine");
        assert_eq!(parts[3], "Kubernetes");
        assert_eq!(parts[4], "API");
        assert_eq!(parts.len(), 5);

        // Test 30-phrase cap with triggers prioritized
        let many_triggers: Vec<String> = (0..25).map(|i| format!("trigger {i}")).collect();
        let many_dict_terms: String = (0..25)
            .map(|i| format!("term {i}"))
            .collect::<Vec<_>>()
            .join(",");
        let big_dict = VoiceDictionary::from_csv(&many_dict_terms);

        let capped_payload = build_hotwords_payload(&big_dict, &many_triggers).unwrap();
        let capped_parts: Vec<&str> = capped_payload.split('/').collect();
        assert_eq!(capped_parts.len(), 30);
        // All 25 triggers must be present at the front
        for (i, part) in capped_parts.iter().enumerate().take(25) {
            assert_eq!(*part, format!("trigger {i}"));
        }
        // Remaining 5 slots filled by dictionary terms
        assert_eq!(capped_parts[25], "term 0");
        assert_eq!(capped_parts[29], "term 4");
    }

    #[test]
    fn test_build_hotwords_payload_empty() {
        let empty_dict = VoiceDictionary::from_csv("");
        let no_triggers: Vec<String> = Vec::new();
        assert_eq!(build_hotwords_payload(&empty_dict, &no_triggers), None);
    }
}
