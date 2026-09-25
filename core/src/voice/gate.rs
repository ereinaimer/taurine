use crate::db::crud::voice_triggers::VoiceTriggerRow;

/// Outcome of the 3-witness verification gate.
#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    /// All three witnesses agree: execute immediately.
    Fire(VoiceTriggerRow),
    /// All three witnesses agree, but confirmation is requested by user/script.
    AskConfirm(VoiceTriggerRow),
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
pub fn evaluate_gate(witnesses: &GateWitnesses<'_>, trigger: &VoiceTriggerRow) -> GateDecision {
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
    let norm_phrase = trigger.spoken_phrase.trim().to_lowercase();
    let norm_verif = witnesses.verifier_transcript.trim().to_lowercase();

    let similarity = if norm_phrase == norm_verif || norm_verif.contains(&norm_phrase) {
        1.0
    } else {
        strsim::normalized_damerau_levenshtein(&norm_phrase, &norm_verif)
    };

    if similarity < 0.80 {
        return GateDecision::Drop {
            reason: format!(
                "Verifier similarity {:.2} ('{}') below 0.80 for trigger '{}'",
                similarity, witnesses.verifier_transcript, trigger.spoken_phrase
            ),
        };
    }

    if trigger.require_confirmation {
        GateDecision::AskConfirm(trigger.clone())
    } else {
        GateDecision::Fire(trigger.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_trigger(phrase: &str, require_confirm: bool) -> VoiceTriggerRow {
        VoiceTriggerRow {
            id: "test-id".to_string(),
            spoken_phrase: phrase.to_string(),
            output: "echo hello".to_string(),
            action_type: "text".to_string(),
            target_os: "any".to_string(),
            only_apps: None,
            except_apps: None,
            require_confirmation: require_confirm,
            strict_threshold: 0.75,
            usage_count: 0,
            is_enabled: true,
            is_deleted: false,
            version: 1,
            created_at: 0,
            updated_at: 0,
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
}
