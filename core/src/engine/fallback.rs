use crate::engine::evaluator::ExpansionResult;
use crate::engine::variables::ExpansionStep;
use crate::stats::TriggerStatKind;

impl crate::engine::evaluator::Evaluator {
    pub(crate) fn check_inline_unit_conversion_fallback(
        &self,
        action_key: crate::settings::ActionKey,
    ) -> Option<ExpansionResult> {
        if action_key != crate::settings::ActionKey::Enter {
            return None;
        }
        if !self
            .state
            .triggerless_mode
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }

        let buf_str = self.buffer.buffer_string();
        if buf_str.trim().is_empty() {
            return None;
        }

        let words: Vec<&str> = buf_str.split_whitespace().collect();
        let max_words = 6.min(words.len());

        for k in (1..=max_words).rev() {
            let suffix_words = &words[words.len() - k..];
            let candidate = suffix_words.join(" ");

            if let Some(result_text) =
                crate::engine::conversion::convert_natural(&candidate, &self.state)
            {
                let delete_count = candidate.chars().count();
                return Some(ExpansionResult {
                    delete_count,
                    steps: vec![ExpansionStep::Text(result_text)],
                    trigger: candidate.clone(),
                    undo_trigger: Some(candidate),
                    is_calculation: true,
                    stat_kind: TriggerStatKind::Calculation,
                    track_usage: true,
                    follow_up: None,
                });
            }
        }
        None
    }

    pub(crate) fn check_inline_datetime_fallback(
        &self,
        action_key: crate::settings::ActionKey,
    ) -> Option<ExpansionResult> {
        if !self.state.inline_datetime_enabled() {
            return None;
        }
        if action_key != crate::settings::ActionKey::Enter {
            return None;
        }

        let buf_str = self.buffer.buffer_string();
        if buf_str.trim().is_empty() {
            return None;
        }

        let words: Vec<&str> = buf_str.split_whitespace().collect();
        let max_words = 6.min(words.len());
        let dialect = self.state.get_inline_datetime_dialect();

        for k in (1..=max_words).rev() {
            let suffix_words = &words[words.len() - k..];
            let candidate = suffix_words.join(" ");

            // Gate: must have an explicit direction signal. Bare quantities ("2 days"),
            // bare times ("3pm"), bare absolute dates ("2024-06-15") and bare "now"
            // are excluded — they are ambiguous or calendar-anchored.
            if !crate::engine::dates::has_expansion_intent(&candidate) {
                continue;
            }

            // Strip leading + so chrono_english receives a clean phrase;
            // - prefix is handled inside preprocess_date_phrase (converted to "ago" suffix)
            let candidate_clean = candidate.trim_start_matches('+').to_string();

            if crate::engine::catalog::is_excluded_phrase(&candidate_clean) {
                continue;
            }

            if let Some((dt, is_date, is_time)) =
                crate::engine::dates::parse_natural_date(&candidate_clean, &dialect)
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
                    undo_trigger: Some(candidate),
                    is_calculation: true,
                    stat_kind: TriggerStatKind::Calculation,
                    track_usage: true,
                    follow_up: None,
                });
            }
        }
        None
    }

    pub(crate) fn check_inline_timezone_fallback(
        &self,
        action_key: crate::settings::ActionKey,
    ) -> Option<ExpansionResult> {
        if !self.state.inline_datetime_enabled() {
            return None;
        }
        if action_key != crate::settings::ActionKey::Enter {
            return None;
        }

        let buf_str = self.buffer.buffer_string();
        if buf_str.trim().is_empty() {
            return None;
        }

        let words: Vec<&str> = buf_str.split_whitespace().collect();
        let max_words = 6.min(words.len());
        let time_format = crate::settings::get_cached_inline_datetime_time_format();
        let dialect = self.state.get_inline_datetime_dialect();

        for k in (1..=max_words).rev() {
            let suffix_words = &words[words.len() - k..];
            let candidate = suffix_words.join(" ");

            if let Some(result_text) = crate::engine::timezones::parse_timezone_expression(
                &candidate,
                &time_format,
                &dialect,
            ) {
                let delete_count = candidate.chars().count();
                return Some(ExpansionResult {
                    delete_count,
                    steps: vec![ExpansionStep::Text(result_text)],
                    trigger: candidate.clone(),
                    undo_trigger: Some(candidate),
                    is_calculation: true,
                    stat_kind: TriggerStatKind::Calculation,
                    track_usage: true,
                    follow_up: None,
                });
            }
        }
        None
    }

    pub(crate) fn check_inline_color_fallback(
        &self,
        action_key: crate::settings::ActionKey,
    ) -> Option<ExpansionResult> {
        if action_key != crate::settings::ActionKey::Enter {
            return None;
        }

        let buf_str = self.buffer.buffer_string();
        if buf_str.trim().is_empty() {
            return None;
        }

        let words: Vec<&str> = buf_str.split_whitespace().collect();
        let max_words = 6.min(words.len());

        for k in (1..=max_words).rev() {
            let suffix_words = &words[words.len() - k..];
            let candidate = suffix_words.join(" ");

            if let Some(result_text) = crate::engine::conversion::convert_color(&candidate) {
                let delete_count = candidate.chars().count();
                return Some(ExpansionResult {
                    delete_count,
                    steps: vec![ExpansionStep::Text(result_text)],
                    trigger: candidate.clone(),
                    undo_trigger: Some(candidate),
                    is_calculation: true,
                    stat_kind: TriggerStatKind::Calculation,
                    track_usage: true,
                    follow_up: None,
                });
            }
        }
        None
    }
}
