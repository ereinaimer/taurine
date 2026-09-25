//! Zero-dependency Double Metaphone-inspired phonetic encoder.
//!
//! Maps words to consonant-skeleton keys so voice trigger matching compares
//! how phrases *sound*, not only how the recognizer spelled them
//! (`folder` vs `follower`). Simplified for short command vocabularies:
//! vowel elision, duplicate collapse, and a small consonant folding set.
//!
//! honey: covers the command-vocabulary subset (C/G alternate branches only);
//! port full DDMetaphone branch table if trigger vocab grows past ~1k phrases
//! or non-English triggers arrive.

/// Compute `(primary, alternate)` phonetic keys for one word.
///
/// Uppercase ASCII letters only; anything else is skipped. The first letter
/// is always kept; later vowels (`AEIOUY`), `H`, and `W` are elided;
/// consecutive duplicates collapse. Consonant folding: `D/T->T` (`TH->T`),
/// `B->P` (except silent `-MB`), `C->K` (`CE/CI/CY->S`, `SH->X`, `PH->F`),
/// `G->K` (`GE/GI/GY->J`), `Q->K`, `V->F`, `X->KS`, `Z->S`. The alternate key
/// assumes hard `C`/`G` (`K`) before front vowels where the primary assumes
/// soft (`S`/`J`).
pub fn double_metaphone(word: &str) -> (String, String) {
    let letters: Vec<char> = word
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    (encode(&letters, false), encode(&letters, true))
}

fn is_front_vowel(c: char) -> bool {
    matches!(c, 'E' | 'I' | 'Y')
}

fn encode(letters: &[char], alternate: bool) -> String {
    if letters.is_empty() {
        return String::new();
    }
    let mut key = String::with_capacity(letters.len());
    let mut last_pushed: Option<char> = None;
    let push = |c: char, last_pushed: &mut Option<char>, key: &mut String| {
        if *last_pushed != Some(c) {
            key.push(c);
            *last_pushed = Some(c);
        }
    };

    let mut i = 0;
    // Silent initial clusters: KN/GN/PN/WR drop the first letter, initial X -> S.
    if letters.len() >= 2 {
        match (letters[0], letters[1]) {
            ('K' | 'G' | 'P', 'N') | ('W', 'R') => i = 1,
            ('X', _) => {
                key.push('S');
                last_pushed = Some('S');
                i = 1;
            }
            _ => {}
        }
    }
    // Leading vowel or H/W kept verbatim as the word-initial sound.
    if i == 0 && matches!(letters[0], 'A' | 'E' | 'I' | 'O' | 'U' | 'Y' | 'H' | 'W') {
        push(letters[0], &mut last_pushed, &mut key);
        i = 1;
    }

    while i < letters.len() {
        let c = letters[i];
        let next = letters.get(i + 1).copied();
        match c {
            'A' | 'E' | 'I' | 'O' | 'U' | 'Y' => {
                last_pushed = None;
            }
            'H' | 'W' => {}
            'B' => {
                if !(i > 0 && letters[i - 1] == 'M' && next.is_none()) {
                    push('P', &mut last_pushed, &mut key);
                }
            }
            'C' => {
                if next == Some('H') {
                    push('X', &mut last_pushed, &mut key);
                    i += 1;
                } else if next.is_some_and(is_front_vowel) {
                    push(
                        if alternate { 'K' } else { 'S' },
                        &mut last_pushed,
                        &mut key,
                    );
                } else {
                    push('K', &mut last_pushed, &mut key);
                }
            }
            'D' => push('T', &mut last_pushed, &mut key),
            'F' | 'J' | 'L' | 'M' | 'N' | 'R' => push(c, &mut last_pushed, &mut key),
            'G' => {
                let prev = if i > 0 {
                    letters.get(i - 1).copied()
                } else {
                    None
                };
                // Terminal or pre-consonantal NG represents the velar nasal /ŋ/ (folds to N), not /K/
                if prev == Some('N')
                    && (next.is_none() || !matches!(next, Some('A' | 'E' | 'I' | 'O' | 'U' | 'Y')))
                {
                    // Already pushed N, skip pushing plosive K
                } else if next.is_some_and(is_front_vowel) {
                    push(
                        if alternate { 'K' } else { 'J' },
                        &mut last_pushed,
                        &mut key,
                    );
                } else {
                    push('K', &mut last_pushed, &mut key);
                }
            }
            'K' => push('K', &mut last_pushed, &mut key),
            'P' => {
                if next == Some('H') {
                    push('F', &mut last_pushed, &mut key);
                    i += 1;
                } else {
                    push('P', &mut last_pushed, &mut key);
                }
            }
            'Q' | 'Z' => push(if c == 'Q' { 'K' } else { 'S' }, &mut last_pushed, &mut key),
            'S' => {
                if next == Some('H') {
                    push('X', &mut last_pushed, &mut key);
                    i += 1;
                } else {
                    push('S', &mut last_pushed, &mut key);
                }
            }
            'T' => push('T', &mut last_pushed, &mut key),
            'V' => push('F', &mut last_pushed, &mut key),
            'X' => {
                push('K', &mut last_pushed, &mut key);
                push('S', &mut last_pushed, &mut key);
            }
            _ => {}
        }
        i += 1;
    }
    key
}

/// Primary phonetic key for one word.
pub fn primary_key(word: &str) -> String {
    double_metaphone(word).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_metaphone_keys() {
        assert_eq!(double_metaphone("folder").0, "FLTR");
        assert_eq!(double_metaphone("follower").0, "FLR");
        assert_eq!(double_metaphone("movies").0, "MFS");
        assert!(!double_metaphone("folder").1.is_empty());

        // Voiced/unvoiced plosive initial consonant folding
        assert_eq!(double_metaphone("Dorren").0, "TRN");
        assert_eq!(double_metaphone("Taurine").0, "TRN");

        // Vowel duplicate reset (non-consecutive consonants preserved)
        assert_eq!(double_metaphone("Tether").0, "TTR");
        assert_eq!(double_metaphone("this").0, "TS");
        assert_eq!(double_metaphone("that").0, "TT");

        // Terminal/pre-consonantal NG velar nasal folding
        assert_eq!(double_metaphone("Toring").0, "TRN");
        assert_eq!(double_metaphone("Theoring").0, "TRN");
        assert_eq!(double_metaphone("Darin").0, "TRN");
        assert_eq!(double_metaphone("drawin").0, "TRN");
    }
}
