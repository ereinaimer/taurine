use crate::engine::evaluator::{CompletionRewrite, TriggerAssistSelectionMode};
pub(crate) fn pop_char_from_query(query: &str) -> String {
    let mut next = query.to_string();
    let _ = next.pop();
    next
}

pub(crate) fn pop_word_from_query(query: &str) -> String {
    let mut chars: Vec<char> = query.chars().collect();

    while chars.last().is_some_and(|ch| ch.is_whitespace()) {
        chars.pop();
    }

    let Some(last_char) = chars.last().copied() else {
        return String::new();
    };
    let is_alphanumeric = last_char.is_alphanumeric();

    while let Some(ch) = chars.last() {
        if ch.is_whitespace() {
            break;
        }

        if ch.is_alphanumeric() == is_alphanumeric {
            chars.pop();
        } else {
            break;
        }
    }

    chars.into_iter().collect()
}

impl crate::engine::evaluator::Evaluator {
    pub fn is_completion_active(&self) -> bool {
        self.completion.active && self.is_buffer_valid_for_completion()
    }

    pub fn is_buffer_valid_for_completion(&self) -> bool {
        if self.completion.is_emoji {
            let active_trigger = self.state.inline_emoji_trigger_char();
            let allow_spaces = self.state.action_key() == crate::settings::ActionKey::Enter;
            self.buffer
                .extract_trigger_word(active_trigger, allow_spaces)
                .is_some()
        } else {
            self.buffer.extract_tail_word().is_some()
        }
    }

    pub fn cancel_completion(&mut self) {
        self.completion.deactivate(&self.state.completion_active);
    }

    pub fn has_active_selection(&self) -> bool {
        self.is_completion_active() && self.completion.has_selection()
    }

    pub fn activate_triggerless_completion(&mut self) -> Option<CompletionRewrite> {
        let tail_word = self.buffer.extract_tail_word()?;
        let suggestions = self.state.matching_word_triggers(&tail_word);
        if suggestions.is_empty() {
            return None;
        }

        self.completion
            .activate(&self.state.completion_active, false);
        self.completion.current_text = tail_word.clone();
        self.completion.original_query = tail_word;
        self.completion.suggestions = suggestions;

        self.cycle_completion_next()
    }

    pub fn activate_triggerless_completion_no_cycle(&mut self) -> Option<()> {
        let tail_word = self.buffer.extract_tail_word()?;
        let suggestions = self.state.matching_word_triggers(&tail_word);
        if suggestions.is_empty() {
            return None;
        }

        self.completion
            .activate(&self.state.completion_active, false);
        self.completion.current_text = tail_word.clone();
        self.completion.suggestions = suggestions;
        self.rebuild_history_items(&tail_word);
        self.completion.original_query = tail_word;
        Some(())
    }

    pub fn cycle_completion_next(&mut self) -> Option<CompletionRewrite> {
        self.cycle_completion(true)
    }

    pub fn cycle_completion_prev(&mut self) -> Option<CompletionRewrite> {
        self.cycle_completion(false)
    }

    pub fn navigate_history_older(&mut self) -> Option<CompletionRewrite> {
        self.navigate_history(true)
    }

    pub fn navigate_history_newer(&mut self) -> Option<CompletionRewrite> {
        self.navigate_history(false)
    }

    pub fn rewrite_backspace_query(&mut self) -> Option<CompletionRewrite> {
        self.rewrite_selected_query(false)
    }

    pub fn rewrite_word_backspace_query(&mut self) -> Option<CompletionRewrite> {
        self.rewrite_selected_query(true)
    }

    pub(crate) fn is_word_boundary(&self) -> bool {
        let buf = self.buffer.buffer_string();
        let chars: Vec<char> = buf.chars().collect();
        if chars.len() < 2 {
            return true;
        }
        let prev = chars[chars.len() - 2];
        prev.is_whitespace()
    }

    pub(crate) fn update_completion_after_char(&mut self, c: char) {
        if self
            .state
            .instant_expand
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let emoji_trigger = self.state.inline_emoji_trigger_char();
        let emoji_enabled = self.state.inline_emoji_enabled();

        if emoji_enabled && c == emoji_trigger && !self.completion.active && self.is_word_boundary()
        {
            self.completion
                .activate(&self.state.completion_active, true);
            self.rebuild_history_items("");
            return;
        }

        if !self.completion.active {
            return;
        }

        let mut query = self.completion.current_text.clone();
        query.push(c);
        self.apply_user_query(query);
    }

    pub(crate) fn sync_completion_from_buffer(&mut self) {
        if !self.completion.active {
            return;
        }

        let allow_spaces = self.state.action_key() == crate::settings::ActionKey::Enter;

        let query = if self.completion.is_emoji {
            let active_trigger = self.state.inline_emoji_trigger_char();
            let Some(query) = self
                .buffer
                .extract_trigger_word(active_trigger, allow_spaces)
            else {
                self.completion.deactivate(&self.state.completion_active);
                return;
            };

            if query
                .chars()
                .any(|ch| !ch.is_alphanumeric() && ch != '-' && ch != '_')
            {
                self.completion.deactivate(&self.state.completion_active);
                return;
            }
            query
        } else {
            let Some(tail) = self.buffer.extract_tail_word() else {
                self.completion.deactivate(&self.state.completion_active);
                return;
            };
            tail
        };

        self.apply_user_query(query);
    }

    pub(crate) fn apply_user_query(&mut self, query: String) {
        if self.completion.is_emoji {
            let emoji_trigger = self.state.inline_emoji_trigger_char();
            let query_with_trigger = if query.starts_with(emoji_trigger) {
                query.clone()
            } else {
                format!("{}{}", emoji_trigger, query)
            };
            self.completion.current_text = query_with_trigger;
        } else {
            self.completion.current_text = query.clone();
        }
        self.completion.original_query = query.clone();
        self.completion.clear_selection();
        self.rebuild_completion_suggestions(&query);
        self.rebuild_history_items(&query);
    }

    pub(crate) fn lookup_query(&self) -> &str {
        &self.completion.original_query
    }

    pub(crate) fn visible_text(&self) -> &str {
        &self.completion.current_text
    }

    pub(crate) fn reset_history_selection_for_completion_lookup(&mut self) {
        if matches!(
            self.completion.selection_mode,
            Some(TriggerAssistSelectionMode::History)
        ) {
            self.completion.clear_selection();
        } else {
            self.completion.history_index = None;
        }
    }

    pub(crate) fn reset_completion_selection_for_history_lookup(&mut self) {
        if matches!(
            self.completion.selection_mode,
            Some(TriggerAssistSelectionMode::Completion)
        ) {
            self.completion.clear_selection();
        } else {
            self.completion.selected_index = None;
        }
    }

    pub(crate) fn rebuild_completion_suggestions(&mut self, query: &str) {
        if !self.completion.active
            || !self.state.inline_tab_completion_enabled()
            || query.is_empty()
        {
            self.completion.suggestions.clear();
            return;
        }

        if self.completion.is_emoji {
            let emoji_trigger = self.state.inline_emoji_trigger_char();
            let clean_query = query.strip_prefix(emoji_trigger).unwrap_or(query);
            let raw_suggestions = crate::engine::emoji::search_emoji_shortcodes(clean_query);
            self.completion.suggestions = raw_suggestions
                .into_iter()
                .map(|s| format!("{}{}", emoji_trigger, s))
                .collect();
        } else {
            let suggestions = self.state.matching_word_triggers(query);
            self.completion.suggestions = suggestions;
        }
    }

    pub(crate) fn rebuild_history_items(&mut self, query: &str) {
        if !self.completion.active
            || !self.state.inline_history_enabled()
            || self.completion.is_emoji
        {
            self.completion.history_items.clear();
            return;
        }

        self.completion.history_items = self.state.matching_word_trigger_history(query);
    }

    pub(crate) fn rewrite_current_text(&mut self, replacement: String) -> CompletionRewrite {
        let delete_count = self.visible_text().chars().count();
        self.buffer.pop_n(delete_count);
        for c in replacement.chars() {
            self.buffer.push(c);
        }

        self.completion.current_text = replacement.clone();

        CompletionRewrite {
            delete_count,
            replacement,
        }
    }

    pub(crate) fn rewrite_selected_query(&mut self, word: bool) -> Option<CompletionRewrite> {
        let is_valid = self.is_buffer_valid_for_completion();

        if !is_valid {
            self.completion.deactivate(&self.state.completion_active);
            return None;
        }

        if !self.completion.active || !self.completion.has_selection() {
            return None;
        }

        let next_query = if word {
            pop_word_from_query(self.lookup_query())
        } else {
            pop_char_from_query(self.lookup_query())
        };
        let rewrite = self.rewrite_current_text(next_query.clone());
        self.apply_user_query(next_query);
        Some(rewrite)
    }

    pub(crate) fn cycle_completion(&mut self, forward: bool) -> Option<CompletionRewrite> {
        if !self.state.inline_tab_completion_enabled() {
            self.completion.suggestions.clear();
            self.completion.selected_index = None;
            if matches!(
                self.completion.selection_mode,
                Some(TriggerAssistSelectionMode::Completion)
            ) {
                self.completion.selection_mode = None;
            }
            return None;
        }

        let is_valid = self.is_buffer_valid_for_completion();

        if !is_valid {
            self.completion.deactivate(&self.state.completion_active);
            return None;
        }

        if !self.completion.active || self.completion.original_query.is_empty() {
            return None;
        }

        self.reset_history_selection_for_completion_lookup();
        let base_query = self.lookup_query().to_string();

        let suggestions = if self.completion.is_emoji {
            let emoji_trigger = self.state.inline_emoji_trigger_char();
            let clean_query = base_query
                .strip_prefix(emoji_trigger)
                .unwrap_or(&base_query);
            let raw_suggestions = crate::engine::emoji::search_emoji_shortcodes(clean_query);
            raw_suggestions
                .into_iter()
                .map(|s| format!("{}{}", emoji_trigger, s))
                .collect()
        } else {
            self.state.matching_word_triggers(&base_query)
        };

        if suggestions.is_empty() {
            self.completion.suggestions.clear();
            self.completion.selected_index = None;
            return None;
        }

        self.completion.suggestions = suggestions;
        self.rebuild_history_items(&base_query);
        self.completion.history_index = None;
        let suggestion_count = self.completion.suggestions.len();
        let next_index = match (self.completion.selected_index, forward) {
            (Some(index), true) => (index + 1) % suggestion_count,
            (Some(index), false) => (index + suggestion_count - 1) % suggestion_count,
            (None, true) => 0,
            (None, false) => suggestion_count - 1,
        };

        let replacement = self.completion.suggestions[next_index].clone();
        let rewrite = self.rewrite_current_text(replacement);
        self.completion.selected_index = Some(next_index);
        self.completion.selection_mode = Some(TriggerAssistSelectionMode::Completion);

        Some(rewrite)
    }

    pub(crate) fn navigate_history(&mut self, older: bool) -> Option<CompletionRewrite> {
        if self.completion.is_emoji {
            return None;
        }

        if !self.state.inline_history_enabled() {
            self.completion.history_items.clear();
            self.completion.history_index = None;
            if matches!(
                self.completion.selection_mode,
                Some(TriggerAssistSelectionMode::History)
            ) {
                self.completion.selection_mode = None;
            }
            return None;
        }

        // For history navigation, allow an empty buffer — the user may have activated
        // completion explicitly (e.g. via Tab on empty input) and then navigated history.
        // Only hard-deactivate when the buffer is non-empty but has no valid tail word
        // (i.e. ends in whitespace), since that genuinely signals a word-boundary exit.
        if self.buffer.len > 0 && !self.is_buffer_valid_for_completion() {
            self.completion.deactivate(&self.state.completion_active);
            return None;
        }

        if !self.completion.active {
            return None;
        }

        self.reset_completion_selection_for_history_lookup();
        let base_query = self.lookup_query().to_string();
        let history_items = self.state.matching_word_trigger_history(&base_query);
        if history_items.is_empty() {
            self.completion.history_items.clear();
            self.completion.history_index = None;
            return None;
        }

        self.completion.history_items = history_items;
        self.completion.suggestions = self.state.matching_word_triggers(&base_query);
        self.completion.selected_index = None;

        if older {
            let next_index = match self.completion.history_index {
                Some(index) if index + 1 >= self.completion.history_items.len() => return None,
                Some(index) => index + 1,
                None => 0,
            };
            let replacement = self.completion.history_items[next_index].clone();
            let rewrite = self.rewrite_current_text(replacement);
            self.completion.history_index = Some(next_index);
            self.completion.selection_mode = Some(TriggerAssistSelectionMode::History);
            return Some(rewrite);
        }

        let current_index = self.completion.history_index?;
        if current_index == 0 {
            self.completion.clear_selection();
            if self.visible_text() == base_query {
                return None;
            }
            return Some(self.rewrite_current_text(base_query));
        }

        let next_index = current_index - 1;
        let replacement = self.completion.history_items[next_index].clone();
        let rewrite = self.rewrite_current_text(replacement);
        self.completion.history_index = Some(next_index);
        self.completion.selection_mode = Some(TriggerAssistSelectionMode::History);
        Some(rewrite)
    }
}
