use super::gate::{keys_sim, word_sim};
use super::pattern::{VoicePattern, VoicePatternPart};
use super::phonetic::double_metaphone;
use crate::engine::variables::ArgMap;

/// Per-word anchor gate: exact match or lexical/phonetic similarity >= 0.80
/// (same bar as the verifier witness in `gate.rs`).
fn anchor_word_match(spoken: &str, anchor: &str) -> bool {
    if spoken.eq_ignore_ascii_case(anchor) {
        return true;
    }
    let spoken = spoken.to_lowercase();
    let anchor = anchor.to_lowercase();
    if word_sim(&spoken, &anchor) >= 0.80 {
        return true;
    }
    keys_sim(&double_metaphone(&spoken), &double_metaphone(&anchor)) >= 0.80
}

fn clean_slot_text(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c: char| c.is_ascii_punctuation())
        .trim()
        .to_string()
}

/// Match one anchor word sequence at `pos`; returns index past the anchor.
fn match_anchor_at(tokens: &[&str], pos: usize, anchor: &str) -> Option<usize> {
    let mut idx = pos;
    for word in anchor.split_whitespace() {
        let spoken = tokens.get(idx)?;
        if !anchor_word_match(spoken, word) {
            return None;
        }
        idx += 1;
    }
    Some(idx)
}

/// Recursive segmented alignment. `out` collects `(slot_name, value)` in order.
fn match_parts(
    parts: &[VoicePatternPart],
    tokens: &[&str],
    pos: usize,
    out: &mut Vec<(String, String)>,
) -> Option<usize> {
    let Some((part, rest)) = parts.split_first() else {
        return Some(pos);
    };
    match part {
        VoicePatternPart::Anchor(anchor) => {
            let end = match_anchor_at(tokens, pos, anchor)?;
            match_parts(rest, tokens, end, out)
        }
        VoicePatternPart::Slot(name) => {
            // Rightmost split first so repeated intermediate anchors
            // (e.g. "to") bind greedily to the message, not the recipient.
            for split in (pos..=tokens.len()).rev() {
                let value = clean_slot_text(&tokens[pos..split].join(" "));
                if value.is_empty() {
                    continue;
                }
                let checkpoint = out.len();
                out.push((name.clone(), value));
                if let Some(end) = match_parts(rest, tokens, split, out) {
                    // Trailing slot must consume the whole transcript.
                    if rest.is_empty() && end != tokens.len() {
                        out.truncate(checkpoint);
                        continue;
                    }
                    return Some(end);
                }
                out.truncate(checkpoint);
            }
            None
        }
    }
}

/// Match a spoken transcript against a voice pattern, extracting slot spans
/// into an `ArgMap` (`named` by slot name, `positional` in slot order).
/// Anchors align in order from the first word with a fuzzy per-word gate;
/// slot spans are captured verbatim and must be non-empty.
pub fn match_voice_pattern(transcript: &str, pattern: &VoicePattern) -> Option<ArgMap> {
    let tokens: Vec<&str> = transcript.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    let mut pairs = Vec::new();
    let end = match_parts(&pattern.parts, &tokens, 0, &mut pairs)?;
    if end != tokens.len() {
        return None;
    }
    let mut args = ArgMap::default();
    for (name, value) in pairs {
        args.positional.push(value.clone());
        args.named.insert(name, value);
    }
    Some(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::pattern::VoicePattern;

    #[test]
    fn test_match_single_slot_trailing() {
        let pattern = VoicePattern::parse("say hi to [person]").unwrap();
        let args = match_voice_pattern("say hi to Sarah", &pattern).unwrap();
        assert_eq!(args.named.get("person").map(|s| s.as_str()), Some("Sarah"));
        assert_eq!(args.positional, vec!["Sarah"]);
    }

    #[test]
    fn test_match_multi_slot_with_intermediate_anchor() {
        let pattern = VoicePattern::parse("send [msg] to [person]").unwrap();
        let args = match_voice_pattern("send I will be late to Sarah", &pattern).unwrap();
        assert_eq!(
            args.named.get("msg").map(|s| s.as_str()),
            Some("I will be late")
        );
        assert_eq!(args.named.get("person").map(|s| s.as_str()), Some("Sarah"));
    }

    #[test]
    fn test_match_rightmost_anchor_disambiguation() {
        let pattern = VoicePattern::parse("send [msg] to [person]").unwrap();
        let args = match_voice_pattern("send happy birthday to you to Sarah.", &pattern).unwrap();
        assert_eq!(
            args.named.get("msg").map(|s| s.as_str()),
            Some("happy birthday to you")
        );
        assert_eq!(args.named.get("person").map(|s| s.as_str()), Some("Sarah"));
    }

    #[test]
    fn test_strips_trailing_punctuation() {
        let pattern = VoicePattern::parse("remind [person] about [task]").unwrap();
        let args = match_voice_pattern("remind Alex, about the meeting.", &pattern).unwrap();
        assert_eq!(args.named.get("person").map(|s| s.as_str()), Some("Alex"));
        assert_eq!(
            args.named.get("task").map(|s| s.as_str()),
            Some("the meeting")
        );
    }

    #[test]
    fn test_empty_slot_does_not_match() {
        let pattern = VoicePattern::parse("send [msg] to [person]").unwrap();
        assert!(match_voice_pattern("send to Sarah", &pattern).is_none());
    }

    #[test]
    fn test_missing_anchor_does_not_match() {
        let pattern = VoicePattern::parse("send [msg] to [person]").unwrap();
        assert!(match_voice_pattern("send hello Sarah", &pattern).is_none());
    }
}
