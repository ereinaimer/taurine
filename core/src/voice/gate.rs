use std::cmp::Ordering;

use crate::db::crud::normalize_voice_phrase;
use crate::db::crud::{ResolvedInvocation, threshold_for_phrase};
use crate::engine::variables::ArgMap;

use super::pattern::{VoicePattern, VoicePatternPart};
use super::pattern_matcher::match_voice_pattern;
use super::phonetic::double_metaphone;

/// Outcome of the 3-witness verification gate.
#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    /// All three witnesses agree: execute immediately.
    Fire(ResolvedInvocation),
    /// All three witnesses agree, but confirmation is requested by user/script.
    AskConfirm(ResolvedInvocation),
    /// Verification failed; drop the event with a descriptive reason.
    Drop { reason: String },
}

/// 3-Witness Decision Gate parameters.
#[derive(Debug, Clone)]
pub struct GateWitnesses<'a> {
    pub vad_confidence: f32,
    pub kws_phrase: &'a str,
    pub kws_confidence: f32,
    pub verifier_transcript: &'a str,
}

/// Evaluates a detected voice trigger against the 3-witness gate.
///
/// Witnesses:
/// 1. VAD: Speech presence confidence >= 0.50
/// 2. KWS: Keyword Spotter confidence >= trigger.strict_threshold
/// 3. Verifier: Secondary recognizer (Moonshine) fuzzy match >= 0.80
pub fn evaluate_gate(witnesses: &GateWitnesses<'_>, trigger: &ResolvedInvocation) -> GateDecision {
    // Witness 1: VAD presence
    if witnesses.vad_confidence < 0.5 {
        return GateDecision::Drop {
            reason: format!(
                "VAD confidence {:.2} below required threshold 0.50",
                witnesses.vad_confidence
            ),
        };
    }

    // Witness 2: KWS confidence
    if witnesses.kws_confidence < trigger.strict_threshold {
        return GateDecision::Drop {
            reason: format!(
                "KWS confidence {:.2} below trigger threshold {:.2}",
                witnesses.kws_confidence, trigger.strict_threshold
            ),
        };
    }

    // Witness 3: Verifier transcript similarity
    let norm_phrase = trigger.invocation.trim().to_lowercase();
    let norm_verif = witnesses.verifier_transcript.trim().to_lowercase();

    let similarity = if norm_phrase == norm_verif || norm_verif.contains(&norm_phrase) {
        1.0
    } else {
        score_trigger(&norm_verif, &norm_phrase)
            .max(strsim::normalized_damerau_levenshtein(&norm_phrase, &norm_verif) as f32)
    };

    let required_threshold = trigger
        .strict_threshold
        .min(threshold_for_phrase(&norm_phrase));

    if similarity < required_threshold {
        return GateDecision::Drop {
            reason: format!(
                "Verifier similarity {:.2} ('{}') below required {:.2} for trigger '{}'",
                similarity, witnesses.verifier_transcript, required_threshold, trigger.invocation
            ),
        };
    }

    if trigger.require_confirmation {
        GateDecision::AskConfirm(trigger.clone())
    } else {
        GateDecision::Fire(trigger.clone())
    }
}

/// Best-matching voice trigger for a transcript, if any.
#[derive(Debug, Clone)]
pub struct TriggerMatch {
    pub trigger: ResolvedInvocation,
    pub score: f32,
}

/// Minimum gap between the top and runner-up candidates.
const RUNNER_UP_MARGIN: f32 = 0.10;

/// Word-pair lexical similarity: best of edit and prefix-weighted measures.
pub(crate) fn word_sim(a: &str, b: &str) -> f64 {
    strsim::normalized_damerau_levenshtein(a, b).max(strsim::jaro_winkler(a, b))
}

/// Phonetic key-pair similarity over primary/alternate key combinations.
pub(crate) fn keys_sim(a: &(String, String), b: &(String, String)) -> f64 {
    let pairs = [(&a.0, &b.0), (&a.0, &b.1), (&a.1, &b.0), (&a.1, &b.1)];
    pairs
        .iter()
        .filter(|(x, y)| !x.is_empty() && !y.is_empty())
        .map(|(x, y)| word_sim(x, y))
        .fold(0.0f64, f64::max)
}

/// Composite similarity between a transcript and one trigger phrase:
///
/// `Score = 0.40 * LexicalSim + 0.40 * PhoneticSim + 0.20 * TokenJaccard`
///
/// Lexical and phonetic terms align each trigger word to its best transcript
/// word (order-free, robust to surrounding chatter); Jaccard rewards exact
/// token overlap.
pub fn score_trigger(transcript: &str, phrase: &str) -> f32 {
    let norm_transcript = normalize_voice_phrase(transcript);
    let norm_phrase = normalize_voice_phrase(phrase);
    let t_tokens: Vec<&str> = norm_transcript.split_whitespace().collect();
    let p_tokens: Vec<&str> = norm_phrase.split_whitespace().collect();
    if t_tokens.is_empty() || p_tokens.is_empty() {
        return 0.0;
    }

    let lexical = p_tokens
        .iter()
        .map(|p| {
            t_tokens
                .iter()
                .map(|t| word_sim(p, t))
                .fold(0.0f64, f64::max)
        })
        .sum::<f64>()
        / p_tokens.len() as f64;
    let lexical = lexical.max(strsim::normalized_damerau_levenshtein(
        &norm_phrase,
        &norm_transcript,
    ));

    let phonetic = p_tokens
        .iter()
        .map(|p| {
            let p_keys = double_metaphone(p);
            t_tokens
                .iter()
                .map(|t| keys_sim(&p_keys, &double_metaphone(t)))
                .fold(0.0f64, f64::max)
        })
        .sum::<f64>()
        / p_tokens.len() as f64;

    let t_set: std::collections::HashSet<&str> = t_tokens.into_iter().collect();
    let p_set: std::collections::HashSet<&str> = p_tokens.into_iter().collect();
    let shared = t_set.intersection(&p_set).count() as f64;
    let union = t_set.union(&p_set).count() as f64;
    let jaccard = if union > 0.0 { shared / union } else { 0.0 };

    (0.40 * lexical + 0.40 * phonetic + 0.20 * jaccard) as f32
}

/// Rank a transcript against all active triggers, returning the winner.
///
/// Enforces the dynamic per-phrase threshold (`threshold_for_phrase`) and a
/// runner-up margin of 0.10 so ambiguous speech never fires.
pub fn rank_voice_triggers(
    transcript: &str,
    triggers: &[ResolvedInvocation],
) -> Option<TriggerMatch> {
    // O(T*W^2) string sims per utterance; trigger lists are small
    // (<1k rows). Index keys if the list grows past that.
    let mut scored: Vec<(f64, &ResolvedInvocation)> = triggers
        .iter()
        .map(|t| (f64::from(score_trigger(transcript, &t.invocation)), t))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
    let (top_score, top) = scored.first()?;
    let norm = normalize_voice_phrase(&top.invocation);
    if *top_score < f64::from(threshold_for_phrase(&norm)) {
        return None;
    }
    if *top_score < 0.90
        && scored.len() >= 2
        && *top_score - scored[1].0 < f64::from(RUNNER_UP_MARGIN)
    {
        return None;
    }
    Some(TriggerMatch {
        trigger: (*top).clone(),
        score: *top_score as f32,
    })
}

/// A voice match carrying extracted slot arguments (`ArgMap::default()` for
/// static triggers).
#[derive(Debug, Clone)]
pub struct RankedVoiceMatch {
    pub trigger: ResolvedInvocation,
    pub score: f32,
    pub args: ArgMap,
}

/// Anchor words of a pattern joined for scoring.
fn anchor_text(pattern: &VoicePattern) -> String {
    pattern
        .parts
        .iter()
        .filter_map(|part| match part {
            VoicePatternPart::Anchor(text) => Some(text.as_str()),
            VoicePatternPart::Slot(_) => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rank a transcript against static and parameterized voice triggers.
///
/// Static triggers keep the legacy fuzzy ranking: an exact static hit
/// (score >= 0.90 with matching word count) wins outright. Otherwise
/// parameterized patterns are tried and the best anchor-scoring one wins;
/// when none match, the static fuzzy result stands.
pub fn rank_voice_invocations(
    transcript: &str,
    triggers: &[ResolvedInvocation],
) -> Option<RankedVoiceMatch> {
    // O(T*W^2) string sims per utterance; trigger lists are small
    // (<1k rows). Index keys if the list grows past that.
    let mut statik = Vec::new();
    let mut parameterized = Vec::new();
    for trigger in triggers {
        match VoicePattern::parse(&trigger.invocation) {
            Ok(pattern) if pattern.is_parameterized() => parameterized.push((trigger, pattern)),
            _ => statik.push(trigger.clone()),
        }
    }
    let static_rank = rank_voice_triggers(transcript, &statik);
    if let Some(ranked) = &static_rank {
        let spoken_words = normalize_voice_phrase(transcript)
            .split_whitespace()
            .count();
        let trigger_words = normalize_voice_phrase(&ranked.trigger.invocation)
            .split_whitespace()
            .count();
        if ranked.score >= 0.90 && spoken_words == trigger_words {
            return Some(RankedVoiceMatch {
                trigger: ranked.trigger.clone(),
                score: ranked.score,
                args: ArgMap::default(),
            });
        }
    }
    let mut best: Option<RankedVoiceMatch> = None;
    for (trigger, pattern) in &parameterized {
        let Some(args) = match_voice_pattern(transcript, pattern) else {
            continue;
        };
        let anchors = anchor_text(pattern);
        let score = score_trigger(transcript, &anchors);
        if score < threshold_for_phrase(&normalize_voice_phrase(&anchors)) {
            continue;
        }
        if best.as_ref().is_none_or(|top| score > top.score) {
            best = Some(RankedVoiceMatch {
                trigger: (*trigger).clone(),
                score,
                args,
            });
        }
    }
    if let Some(winner) = best {
        return Some(winner);
    }
    static_rank.map(|ranked| RankedVoiceMatch {
        trigger: ranked.trigger,
        score: ranked.score,
        args: ArgMap::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::{InvocationType, TriggerAction};

    fn test_trigger(phrase: &str, require_confirm: bool) -> ResolvedInvocation {
        ResolvedInvocation {
            trigger_id: "test-id".to_string(),
            invocation: phrase.to_string(),
            invocation_type: InvocationType::Voice,
            action: TriggerAction::text("echo hello"),
            require_confirmation: require_confirm,
            strict_threshold: 0.75,
        }
    }

    #[test]
    fn test_gate_fires_on_all_witnesses_agreeing() {
        let trigger = test_trigger("open chrome", false);
        let witnesses = GateWitnesses {
            vad_confidence: 0.95,
            kws_phrase: "open chrome",
            kws_confidence: 0.88,
            verifier_transcript: "open chrome",
        };

        let decision = evaluate_gate(&witnesses, &trigger);
        assert_eq!(decision, GateDecision::Fire(trigger));
    }

    #[test]
    fn test_gate_asks_confirmation_when_configured() {
        let trigger = test_trigger("deploy staging", true);
        let witnesses = GateWitnesses {
            vad_confidence: 0.90,
            kws_phrase: "deploy staging",
            kws_confidence: 0.85,
            verifier_transcript: "deploy staging",
        };

        let decision = evaluate_gate(&witnesses, &trigger);
        assert_eq!(decision, GateDecision::AskConfirm(trigger));
    }

    #[test]
    fn test_gate_drops_on_low_vad() {
        let trigger = test_trigger("open chrome", false);
        let witnesses = GateWitnesses {
            vad_confidence: 0.35, // below 0.5
            kws_phrase: "open chrome",
            kws_confidence: 0.90,
            verifier_transcript: "open chrome",
        };

        let decision = evaluate_gate(&witnesses, &trigger);
        assert!(matches!(decision, GateDecision::Drop { .. }));
    }

    #[test]
    fn test_gate_drops_on_low_kws_confidence() {
        let trigger = test_trigger("open chrome", false);
        let witnesses = GateWitnesses {
            vad_confidence: 0.90,
            kws_phrase: "open chrome",
            kws_confidence: 0.60, // below 0.75
            verifier_transcript: "open chrome",
        };

        let decision = evaluate_gate(&witnesses, &trigger);
        assert!(matches!(decision, GateDecision::Drop { .. }));
    }

    #[test]
    fn test_gate_drops_on_verifier_divergence() {
        let trigger = test_trigger("open chrome", false);
        let witnesses = GateWitnesses {
            vad_confidence: 0.90,
            kws_phrase: "open chrome",
            kws_confidence: 0.85,
            verifier_transcript: "go home", // completely different
        };

        let decision = evaluate_gate(&witnesses, &trigger);
        assert!(matches!(decision, GateDecision::Drop { .. }));
    }

    #[test]
    fn test_gate_allows_minor_fuzzy_slur() {
        let trigger = test_trigger("open chrome", false);
        let witnesses = GateWitnesses {
            vad_confidence: 0.90,
            kws_phrase: "open chrome",
            kws_confidence: 0.85,
            verifier_transcript: "open chome", // typo / minor phonetic slur, >0.8 similarity
        };

        let decision = evaluate_gate(&witnesses, &trigger);
        assert_eq!(decision, GateDecision::Fire(trigger));
    }

    #[test]
    fn test_rank_voice_triggers_movies_folder() {
        let triggers = vec![
            test_trigger("movies folder", false),
            test_trigger("open browser", false),
        ];
        let matched = rank_voice_triggers("movies follower", &triggers).expect("must match");
        assert_eq!(matched.trigger.invocation, "movies folder");
        assert!(matched.score >= 0.80, "score was {}", matched.score);
    }

    #[test]
    fn test_rank_voice_triggers_margin_rejection() {
        // Background chatter shares a word with two triggers but matches
        // neither well enough and has no clear winner: must not fire.
        let triggers = vec![
            test_trigger("movies folder", false),
            test_trigger("movies player", false),
        ];
        assert!(rank_voice_triggers("movies are great", &triggers).is_none());
    }

    #[test]
    fn test_gate_allows_phonetic_slur_movies_folder() {
        let trigger = test_trigger("movies folder", false);
        let witnesses = GateWitnesses {
            vad_confidence: 0.90,
            kws_phrase: "movies folder",
            kws_confidence: 0.85,
            verifier_transcript: "movies follower",
        };

        let decision = evaluate_gate(&witnesses, &trigger);
        assert_eq!(decision, GateDecision::Fire(trigger));
    }

    #[test]
    fn gate_fires_through_alias_with_confirmation() {
        let inv = ResolvedInvocation {
            trigger_id: "id".into(),
            invocation: "say hi".into(),
            invocation_type: InvocationType::Voice,
            action: TriggerAction::text("Hello!"),
            require_confirmation: true,
            strict_threshold: 0.85,
        };
        let decision = evaluate_gate(
            &GateWitnesses {
                vad_confidence: 0.9,
                kws_phrase: "say hi",
                kws_confidence: 0.9,
                verifier_transcript: "say hi",
            },
            &inv,
        );
        assert!(matches!(decision, GateDecision::AskConfirm(_)));
    }
}
