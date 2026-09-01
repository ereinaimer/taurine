use super::evaluator::ExpansionFollowUp;
use crate::engine::evaluator::ExpansionResult;
use crate::engine::variables::ExpansionStep;
use crate::stats::TriggerStatKind;
pub(crate) fn stat_kind_for_steps(
    is_calculation: bool,
    steps: &[ExpansionStep],
) -> TriggerStatKind {
    if is_calculation {
        return TriggerStatKind::Calculation;
    }

    if matches!(steps, [ExpansionStep::Script(_)]) {
        return TriggerStatKind::Script;
    }

    TriggerStatKind::Snippet
}

static DICT_REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)\b(?:(?:meaning|definition) (?:of|for) (?P<def1>[a-zA-Z]+(?:[-'][a-zA-Z]+)*)|what does (?P<def2>[a-zA-Z]+(?:[-'][a-zA-Z]+)*) mean\??|define (?P<def3>[a-zA-Z]+(?:[-'][a-zA-Z]+)*)|(?P<def4>[a-zA-Z]+(?:[-'][a-zA-Z]+)*) (?:meaning|definition|means\??)|synonyms? (?:of|for) (?P<syn1>[a-zA-Z]+(?:[-'][a-zA-Z]+)*)|(?P<syn2>[a-zA-Z]+(?:[-'][a-zA-Z]+)*) synonyms?|antonyms? (?:of|for) (?P<ant1>[a-zA-Z]+(?:[-'][a-zA-Z]+)*)|(?P<ant2>[a-zA-Z]+(?:[-'][a-zA-Z]+)*) antonyms?|opposites? (?:of|for) (?P<ant3>[a-zA-Z]+(?:[-'][a-zA-Z]+)*)|(?P<ant4>[a-zA-Z]+(?:[-'][a-zA-Z]+)*) opposites?)$").expect("Valid dictionary regex")
});

impl crate::engine::evaluator::Evaluator {
    pub(crate) fn evaluate_buffer_for_expansion_lazy(
        &mut self,
        window: &crate::engine::catalog::WindowResolver,
        mut fetch_window: Option<impl FnOnce() -> Option<String>>,
    ) -> Option<ExpansionResult> {
        let emoji_trigger = self.state.inline_emoji_trigger_char();
        let emoji_enabled = self.state.inline_emoji_enabled();
        let instant_expand = self
            .state
            .instant_expand
            .load(std::sync::atomic::Ordering::Relaxed);

        // Try inline unit conversion fallback (disabled in instant expand mode)
        if !instant_expand && let Some(word) = self.buffer.extract_tail_word() {
            // Try inline color conversion (compact syntax) first
            if let Some(result_text) = crate::engine::conversion::convert_color(&word) {
                let delete_count = word.chars().count();
                self.buffer.clear();
                return Some(ExpansionResult {
                    delete_count,
                    steps: vec![ExpansionStep::Text(result_text)],
                    trigger: word.clone(),
                    undo_trigger: Some(word),
                    is_calculation: true,
                    stat_kind: TriggerStatKind::Calculation,
                    track_usage: true,
                    follow_up: None,
                });
            }

            let (cleaned_word, intervals) = crate::engine::comma::preprocess(&word);
            if crate::engine::conversion::is_conversion_pattern(&cleaned_word)
                && let Some(result_text) =
                    crate::engine::conversion::convert(&cleaned_word, &self.state)
            {
                let formatted = if let Some(ref ivs) = intervals {
                    crate::engine::comma::format_result(&result_text, ivs)
                } else {
                    result_text
                };
                let delete_count = word.chars().count();
                self.buffer.clear();
                return Some(ExpansionResult {
                    delete_count,
                    steps: vec![ExpansionStep::Text(formatted)],
                    trigger: word.clone(),
                    undo_trigger: Some(word),
                    is_calculation: true,
                    stat_kind: TriggerStatKind::Calculation,
                    track_usage: true,
                    follow_up: None,
                });
            }
        }

        if !instant_expand
            && emoji_enabled
            && let Some(word) = self.buffer.extract_trigger_word(emoji_trigger)
            && let Some(emoji_char) = crate::engine::emoji::lookup_emoji(&word)
        {
            let delete_count = 1 + word.chars().count();
            self.buffer.clear();
            return Some(ExpansionResult {
                delete_count,
                steps: vec![ExpansionStep::Text(emoji_char)],
                trigger: word.clone(),
                undo_trigger: Some(format!("{}{}", emoji_trigger, word)),
                is_calculation: false,
                stat_kind: TriggerStatKind::Snippet,
                track_usage: true,
                follow_up: None,
            });
        }
        if !instant_expand
            && self
                .state
                .inline_dictionary_enabled
                .load(std::sync::atomic::Ordering::Relaxed)
        {
            let buf_str = self.buffer.buffer_string();
            let mut word_opt = None;
            let mut matched_len = 0;
            let mut lookup_type = crate::engine::dictionary::DictionaryLookupType::Meaning;

            if let Some(caps) = DICT_REGEX.captures(&buf_str) {
                if let Some(m) = caps
                    .name("def1")
                    .or(caps.name("def2"))
                    .or(caps.name("def3"))
                    .or(caps.name("def4"))
                {
                    word_opt = Some(m.as_str().to_string());
                    matched_len = caps.get(0).map(|x| x.as_str().chars().count()).unwrap_or(0);
                    lookup_type = crate::engine::dictionary::DictionaryLookupType::Meaning;
                } else if let Some(m) = caps.name("syn1").or(caps.name("syn2")) {
                    word_opt = Some(m.as_str().to_string());
                    matched_len = caps.get(0).map(|x| x.as_str().chars().count()).unwrap_or(0);
                    lookup_type = crate::engine::dictionary::DictionaryLookupType::Synonyms;
                } else if let Some(m) = caps
                    .name("ant1")
                    .or(caps.name("ant2"))
                    .or(caps.name("ant3"))
                    .or(caps.name("ant4"))
                {
                    word_opt = Some(m.as_str().to_string());
                    matched_len = caps.get(0).map(|x| x.as_str().chars().count()).unwrap_or(0);
                    lookup_type = crate::engine::dictionary::DictionaryLookupType::Antonyms;
                }
            }

            if let Some(word) = word_opt {
                let initial_text = self.get_thinking_text();
                self.buffer.clear();
                return Some(ExpansionResult {
                    delete_count: matched_len,
                    steps: vec![ExpansionStep::Text(initial_text)],
                    trigger: word.clone(),
                    undo_trigger: None,
                    is_calculation: false,
                    stat_kind: TriggerStatKind::Snippet,
                    track_usage: true,
                    follow_up: Some(ExpansionFollowUp::DictionaryLookup { word, lookup_type }),
                });
            }
        }

        let mut candidates = self.buffer.extract_suffix_candidates();
        candidates.sort_by_key(|a| std::cmp::Reverse(a.0.len()));

        for (word, prev_char) in candidates {
            let is_boundary = !instant_expand
                || prev_char.is_none_or(|c| c.is_whitespace() || c.is_ascii_punctuation());
            if is_boundary
                && let Some(expansion) = self.state.fetch_expansion_no_date_fallback_lazy(
                    &word,
                    window,
                    fetch_window.take(),
                )
            {
                let delete_count = word.chars().count();
                let stat_kind = stat_kind_for_steps(expansion.is_calculation, &expansion.steps);
                self.buffer.clear();
                if let Some(template) = expansion.ai_transformer_template {
                    let initial_text = self.get_initial_spinner_text(&template);
                    return Some(ExpansionResult {
                        delete_count,
                        steps: vec![ExpansionStep::Text(initial_text)],
                        trigger: word,
                        undo_trigger: None,
                        is_calculation: false,
                        stat_kind: TriggerStatKind::InlineAi,
                        track_usage: true,
                        follow_up: Some(ExpansionFollowUp::AiTransformer {
                            template_with_markers: template,
                        }),
                    });
                }

                let undo_trigger = self.undo_trigger_for_steps(&word, &expansion.steps);

                return Some(ExpansionResult {
                    delete_count,
                    steps: expansion.steps,
                    trigger: word,
                    undo_trigger,
                    is_calculation: expansion.is_calculation,
                    stat_kind,
                    track_usage: true,
                    follow_up: None,
                });
            }
        }

        // Regex matching fallback — skip allocation if no regex patterns loaded
        if !self.state.regex_catalog.is_empty() {
            let buf_str = self.buffer.buffer_string();
            if let Some((keyword, action, captures)) =
                self.state
                    .match_regex_action_lazy(&buf_str, window, fetch_window.take())
            {
                use crate::engine::catalog::expand_trigger_action_with_args;
                use crate::engine::variables::ArgMap;

                let arg_map = ArgMap {
                    positional: captures,
                    ..Default::default()
                };

                if let Some(expansion) = expand_trigger_action_with_args(action, &arg_map, &keyword)
                {
                    let delete_count = keyword.chars().count();
                    let stat_kind = stat_kind_for_steps(expansion.is_calculation, &expansion.steps);
                    self.buffer.clear();

                    if let Some(template) = expansion.ai_transformer_template {
                        let initial_text = self.get_initial_spinner_text(&template);
                        return Some(ExpansionResult {
                            delete_count,
                            steps: vec![ExpansionStep::Text(initial_text)],
                            trigger: keyword.clone(),
                            undo_trigger: None,
                            is_calculation: false,
                            stat_kind: TriggerStatKind::InlineAi,
                            track_usage: true,
                            follow_up: Some(ExpansionFollowUp::AiTransformer {
                                template_with_markers: template,
                            }),
                        });
                    }

                    let undo_trigger = self.undo_trigger_for_steps(&keyword, &expansion.steps);

                    return Some(ExpansionResult {
                        delete_count,
                        steps: expansion.steps,
                        trigger: keyword,
                        undo_trigger,
                        is_calculation: expansion.is_calculation,
                        stat_kind,
                        track_usage: true,
                        follow_up: None,
                    });
                }
            }
        }
        if !instant_expand && let Some(result) = self.check_inline_unit_conversion_fallback() {
            self.buffer.clear();
            return Some(result);
        }

        if !instant_expand && let Some(result) = self.check_inline_timezone_fallback() {
            self.buffer.clear();
            return Some(result);
        }

        if !instant_expand && let Some(result) = self.check_inline_datetime_fallback() {
            self.buffer.clear();
            return Some(result);
        }

        if !instant_expand && let Some(result) = self.check_inline_color_fallback() {
            self.buffer.clear();
            return Some(result);
        }

        if emoji_enabled {
            let buf_str = self.buffer.buffer_string();
            let words: Vec<&str> = buf_str.split_whitespace().collect();
            for i in (0..words.len().min(4)).rev() {
                let phrase = words[words.len() - 1 - i..].join(" ");
                let normalized: String = words[words.len() - 1 - i..]
                    .iter()
                    .map(|w| w.trim_end_matches(|c: char| !c.is_alphanumeric()))
                    .collect::<Vec<_>>()
                    .join(" ");
                if let Some(trimmed) = normalized.strip_suffix(" emoji") {
                    if trimmed.is_empty() || trimmed.chars().all(|c| c.is_whitespace()) {
                        continue;
                    }
                    let matches = crate::engine::emoji::search_natural_language_emojis(trimmed);
                    if !matches.is_empty() {
                        let emoji_char = matches[0].clone();
                        let delete_count = phrase.chars().count();
                        self.buffer.clear();
                        return Some(ExpansionResult {
                            delete_count,
                            steps: vec![ExpansionStep::Text(emoji_char)],
                            trigger: phrase.clone(),
                            undo_trigger: Some(phrase),
                            is_calculation: false,
                            stat_kind: TriggerStatKind::Snippet,
                            track_usage: true,
                            follow_up: None,
                        });
                    }
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dict_regex_captures_natural_language_triggers() {
        let cases = [
            ("meaning of self-esteem", Some(("self-esteem", "Meaning"))),
            ("definition of well-being", Some(("well-being", "Meaning"))),
            ("what does o'clock mean", Some(("o'clock", "Meaning"))),
            ("synonyms for happy", Some(("happy", "Synonyms"))),
            ("antonyms for dark", Some(("dark", "Antonyms"))),
            ("opposites for dark", Some(("dark", "Antonyms"))),
            ("meaning of happy", Some(("happy", "Meaning"))),
        ];

        for (input, expected) in cases {
            let captures = DICT_REGEX.captures(input);
            if let Some(expected_val) = expected {
                let caps = captures.unwrap_or_else(|| panic!("Failed to match input: {}", input));

                let (word, type_str) = if let Some(w) = caps
                    .name("def1")
                    .or(caps.name("def2"))
                    .or(caps.name("def3"))
                    .or(caps.name("def4"))
                {
                    (w.as_str(), "Meaning")
                } else if let Some(w) = caps.name("syn1").or(caps.name("syn2")) {
                    (w.as_str(), "Synonyms")
                } else if let Some(w) = caps
                    .name("ant1")
                    .or(caps.name("ant2"))
                    .or(caps.name("ant3"))
                    .or(caps.name("ant4"))
                {
                    (w.as_str(), "Antonyms")
                } else {
                    panic!("Match found but no group caught for {}", input);
                };

                assert_eq!(word, expected_val.0, "Word mismatch for input {}", input);
                assert_eq!(
                    type_str, expected_val.1,
                    "Type mismatch for input {}",
                    input
                );
            } else {
                assert!(
                    captures.is_none(),
                    "Expected no match for {}, but got one",
                    input
                );
            }
        }
    }
}
