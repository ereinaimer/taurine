use crate::engine::evaluator::ExpansionResult;
use crate::engine::variables::ExpansionStep;
use crate::stats::TriggerStatKind;

impl crate::engine::evaluator::Evaluator {
    pub(crate) fn check_all_inline_fallbacks(&self) -> Option<ExpansionResult> {
        let buf_str = self.buffer.buffer_string();
        if buf_str.trim().is_empty() {
            return None;
        }

        let words: Vec<&str> = buf_str.split_whitespace().collect();
        let max_words = 6.min(words.len());
        if max_words == 0 {
            return None;
        }

        let mut candidates = Vec::with_capacity(max_words);
        for k in (1..=max_words).rev() {
            let suffix_words = &words[words.len() - k..];
            candidates.push(suffix_words.join(" "));
        }

        // 1. Try unit conversion fallback
        for candidate in &candidates {
            if let Some(result_text) =
                crate::engine::conversion::convert_natural(candidate, &self.state)
            {
                let delete_count = candidate.chars().count();
                return Some(ExpansionResult {
                    delete_count,
                    steps: vec![ExpansionStep::Text(result_text)],
                    trigger: candidate.clone(),
                    undo_trigger: Some(candidate.clone()),
                    is_calculation: true,
                    stat_kind: TriggerStatKind::Calculation,
                    track_usage: true,
                    follow_up: None,
                });
            }
        }

        // 2. Try timezone fallback
        if self.state.inline_datetime_enabled() {
            let time_format = crate::settings::get_cached_inline_datetime_time_format();
            let dialect = self.state.get_inline_datetime_dialect();

            for candidate in &candidates {
                if let Some(result_text) = crate::engine::timezones::parse_timezone_expression(
                    candidate,
                    &time_format,
                    &dialect,
                ) {
                    let delete_count = candidate.chars().count();
                    return Some(ExpansionResult {
                        delete_count,
                        steps: vec![ExpansionStep::Text(result_text)],
                        trigger: candidate.clone(),
                        undo_trigger: Some(candidate.clone()),
                        is_calculation: true,
                        stat_kind: TriggerStatKind::Calculation,
                        track_usage: true,
                        follow_up: None,
                    });
                }
            }

            // 3. Try datetime fallback
            for candidate in &candidates {
                if !crate::engine::dates::has_expansion_intent(candidate) {
                    continue;
                }

                let candidate_clean = candidate.trim_start_matches('+');
                if crate::engine::catalog::is_excluded_phrase(candidate_clean) {
                    continue;
                }

                if let Some((dt, is_date, is_time)) =
                    crate::engine::dates::parse_natural_date(candidate_clean, &dialect)
                {
                    let pattern = if is_date && is_time {
                        self.state.get_inline_datetime_datetime_format()
                    } else if is_time {
                        self.state.get_inline_datetime_time_format()
                    } else {
                        self.state.get_inline_datetime_date_format()
                    };

                    let date_str = crate::engine::dates::format_datetime(dt, &pattern);
                    let delete_count = candidate.chars().count();

                    return Some(ExpansionResult {
                        delete_count,
                        steps: vec![ExpansionStep::Text(date_str)],
                        trigger: candidate.clone(),
                        undo_trigger: Some(candidate.clone()),
                        is_calculation: true,
                        stat_kind: TriggerStatKind::Calculation,
                        track_usage: true,
                        follow_up: None,
                    });
                }
            }
        }

        // 4. Try color fallback
        for candidate in &candidates {
            if let Some(result_text) = crate::engine::conversion::convert_color(candidate) {
                let delete_count = candidate.chars().count();
                return Some(ExpansionResult {
                    delete_count,
                    steps: vec![ExpansionStep::Text(result_text)],
                    trigger: candidate.clone(),
                    undo_trigger: Some(candidate.clone()),
                    is_calculation: true,
                    stat_kind: TriggerStatKind::Calculation,
                    track_usage: true,
                    follow_up: None,
                });
            }
        }

        // 5. Try natural language emoji fallback
        if self.state.inline_emoji_enabled() {
            for candidate in &candidates {
                let normalized: String = candidate
                    .split_whitespace()
                    .map(|w| w.trim_end_matches(|c: char| !c.is_alphanumeric()))
                    .collect::<Vec<_>>()
                    .join(" ");
                if let Some(trimmed) = normalized.strip_suffix(" emoji")
                    && !trimmed.is_empty()
                    && !trimmed.chars().all(|c| c.is_whitespace())
                {
                    let matches = crate::engine::emoji::search_natural_language_emojis(trimmed);
                    if !matches.is_empty() {
                        let emoji_char = matches[0].clone();
                        let delete_count = candidate.chars().count();
                        return Some(ExpansionResult {
                            delete_count,
                            steps: vec![ExpansionStep::Text(emoji_char)],
                            trigger: candidate.clone(),
                            undo_trigger: Some(candidate.clone()),
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
