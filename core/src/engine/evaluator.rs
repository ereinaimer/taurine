use std::sync::Arc;

use crate::engine::variables::ExpansionStep;
use crate::engine::variables::system::clip::MAX_PAYLOAD_BYTES;
use crate::metrics::AutomationMetricKind;

use crate::engine::buffer::FastBuffer;
use crate::engine::state::{EngineMode, EngineState};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineEvent {
    Char(char),
    Backspace,
    WordBackspace,
    ActionDelimiter,
    Interrupt, // Esc, Mouse clicks, or loss of focus
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExpansionFollowUp {
    InlineAi {
        prompt: String,
        system_prompt_override: Option<String>,
    },
    /// One or more `| ai(prompt)` transformers were found in the template.
    /// Each entry carries the resolved source text and the prompt to apply.
    /// The final text must be pre-resolved before injection (shown as spinner while processing).
    AiTransformer {
        /// The fully interpolated template output, with AI placeholder markers embedded.
        /// Markers use the form: `\x03<base64-input>\x1F<prompt>\x04`.
        template_with_markers: String,
    },
}

/// Instructions the daemon must execute to perform a text expansion.
///
/// The daemon's only job is to relay these instructions to the OS:
/// 1. Send `delete_count` backspaces to erase the trigger sequence.
/// 2. Execute each `ExpansionStep` in the `steps` sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpansionResult {
    /// Number of characters to delete (the trigger char + keyword + the trailing space).
    pub delete_count: usize,
    /// Ordered sequence of actions (text pastes, key presses, delays).
    pub steps: Vec<ExpansionStep>,
    /// The trigger keyword that was matched.
    pub trigger: String,
    /// Exact trigger text to restore during Backspace Undo, including the prefix character.
    pub undo_trigger: Option<String>,
    /// Whether this expansion was a mathematical calculation.
    pub is_calculation: bool,
    /// Metric policy classification for this expansion.
    pub metric_kind: AutomationMetricKind,
    /// Whether the daemon should record snippet/calculation usage for this expansion.
    pub track_usage: bool,
    /// Optional daemon-side follow-up that should run after expansion injection.
    pub follow_up: Option<ExpansionFollowUp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRewrite {
    pub delete_count: usize,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct TriggerCompletionState {
    active: bool,
    original_query: String,
    current_text: String,
    suggestions: Vec<String>,
    selected_index: Option<usize>,
    history_items: Vec<String>,
    history_index: Option<usize>,
    selection_mode: Option<TriggerAssistSelectionMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriggerAssistSelectionMode {
    Completion,
    History,
}

impl TriggerCompletionState {
    fn activate(&mut self, active_atomic: &std::sync::atomic::AtomicBool) {
        self.active = true;
        active_atomic.store(true, std::sync::atomic::Ordering::Relaxed);
        self.original_query.clear();
        self.current_text.clear();
        self.suggestions.clear();
        self.selected_index = None;
        self.history_items.clear();
        self.history_index = None;
        self.selection_mode = None;
    }

    fn deactivate(&mut self, active_atomic: &std::sync::atomic::AtomicBool) {
        self.active = false;
        active_atomic.store(false, std::sync::atomic::Ordering::Relaxed);
        self.original_query.clear();
        self.current_text.clear();
        self.suggestions.clear();
        self.selected_index = None;
        self.history_items.clear();
        self.history_index = None;
        self.selection_mode = None;
    }

    fn clear_selection(&mut self) {
        self.selected_index = None;
        self.history_index = None;
        self.selection_mode = None;
    }

    fn has_selection(&self) -> bool {
        self.selection_mode.is_some()
    }
}

pub struct Evaluator {
    pub buffer: FastBuffer,
    pub state: Arc<EngineState>,
    completion: TriggerCompletionState,
}

impl Evaluator {
    pub fn new(state: Arc<EngineState>) -> Self {
        state
            .completion_active
            .store(false, std::sync::atomic::Ordering::Relaxed);
        Self {
            buffer: FastBuffer::new(),
            state,
            completion: TriggerCompletionState::default(),
        }
    }

    fn get_thinking_text(&self) -> String {
        let style = self
            .state
            .spinner_style
            .read()
            .map(|s| *s)
            .unwrap_or_default();
        match style {
            crate::settings::SpinnerStyle::Braille => "⠋".to_string(),
            crate::settings::SpinnerStyle::Arc => "◜".to_string(),
            crate::settings::SpinnerStyle::Classic => "|".to_string(),
        }
    }

    fn get_initial_spinner_text(&self, template: &str) -> String {
        if crate::engine::variables::contains_non_sys_markers(template) {
            self.get_thinking_text()
        } else {
            let style = self
                .state
                .spinner_style
                .read()
                .map(|s| *s)
                .unwrap_or_default();
            crate::utils::spinner::get_frames(style)[0].to_string()
        }
    }

    fn trigger_prefix(&self) -> char {
        use std::sync::atomic::Ordering;
        let trigger_char_u32 = self.state.trigger_char.load(Ordering::Relaxed);
        std::char::from_u32(trigger_char_u32).unwrap_or('>')
    }

    fn full_trigger_text(&self, keyword: &str) -> String {
        format!("{}{}", self.trigger_prefix(), keyword)
    }

    fn allows_blind_undo(&self, steps: &[ExpansionStep]) -> bool {
        let mut text_bytes = 0usize;

        for step in steps {
            match step {
                ExpansionStep::Text(text) => {
                    text_bytes = text_bytes.saturating_add(text.len());
                }
                // Structural templates move the caret away from the absolute tail, so a blind
                // backspace replay would corrupt surrounding text instead of the expansion.
                ExpansionStep::KeyPress(_)
                | ExpansionStep::Delay(_)
                | ExpansionStep::MouseClick
                | ExpansionStep::MouseRClick
                | ExpansionStep::MouseMClick
                | ExpansionStep::MouseMove(_, _)
                | ExpansionStep::MouseScroll(_)
                | ExpansionStep::MouseHold
                | ExpansionStep::MouseRelease => return false,
                ExpansionStep::Script(_)
                | ExpansionStep::InlineRun(_, _)
                | ExpansionStep::Image(_, _) => return false,
            }
        }

        // Clipboard history can legally hold a full 1 MB payload. Treat that ceiling as unsafe
        // for blind undo so Taurine never floods the OS with a huge backspace replay.
        text_bytes < MAX_PAYLOAD_BYTES
    }

    fn undo_trigger_for_steps(&self, keyword: &str, steps: &[ExpansionStep]) -> Option<String> {
        self.allows_blind_undo(steps)
            .then(|| self.full_trigger_text(keyword))
    }

    pub fn is_completion_active(&self) -> bool {
        self.completion.active
            && self
                .buffer
                .extract_trigger_word(self.trigger_prefix())
                .is_some()
    }

    pub fn cancel_completion(&mut self) {
        self.completion.deactivate(&self.state.completion_active);
    }

    pub fn has_active_selection(&self) -> bool {
        self.is_completion_active() && self.completion.has_selection()
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

    fn update_completion_after_char(&mut self, c: char) {
        let trigger_char = self.trigger_prefix();
        if c == trigger_char && !self.completion.active {
            self.completion.activate(&self.state.completion_active);
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

    fn sync_completion_from_buffer(&mut self) {
        if !self.completion.active {
            return;
        }

        let trigger_char = self.trigger_prefix();
        let Some(query) = self.buffer.extract_trigger_word(trigger_char) else {
            self.completion.deactivate(&self.state.completion_active);
            return;
        };

        self.apply_user_query(query);
    }

    fn apply_user_query(&mut self, query: String) {
        self.completion.current_text = query.clone();
        self.completion.original_query = query.clone();
        self.completion.clear_selection();
        self.rebuild_completion_suggestions(&query);
        self.rebuild_history_items(&query);
    }

    fn lookup_query(&self) -> &str {
        &self.completion.original_query
    }

    fn visible_text(&self) -> &str {
        &self.completion.current_text
    }

    fn reset_history_selection_for_completion_lookup(&mut self) {
        if matches!(
            self.completion.selection_mode,
            Some(TriggerAssistSelectionMode::History)
        ) {
            self.completion.clear_selection();
        } else {
            self.completion.history_index = None;
        }
    }

    fn reset_completion_selection_for_history_lookup(&mut self) {
        if matches!(
            self.completion.selection_mode,
            Some(TriggerAssistSelectionMode::Completion)
        ) {
            self.completion.clear_selection();
        } else {
            self.completion.selected_index = None;
        }
    }

    fn rebuild_completion_suggestions(&mut self, query: &str) {
        if !self.completion.active
            || !self.state.inline_tab_completion_enabled()
            || query.is_empty()
        {
            self.completion.suggestions.clear();
            return;
        }

        self.completion.suggestions = self.state.matching_word_triggers(query);
    }

    fn rebuild_history_items(&mut self, query: &str) {
        if !self.completion.active || !self.state.inline_history_enabled() {
            self.completion.history_items.clear();
            return;
        }

        self.completion.history_items = self.state.matching_word_trigger_history(query);
    }

    fn rewrite_current_text(&mut self, replacement: String) -> CompletionRewrite {
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

    fn rewrite_selected_query(&mut self, word: bool) -> Option<CompletionRewrite> {
        if self
            .buffer
            .extract_trigger_word(self.trigger_prefix())
            .is_none()
        {
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

    fn cycle_completion(&mut self, forward: bool) -> Option<CompletionRewrite> {
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

        if self
            .buffer
            .extract_trigger_word(self.trigger_prefix())
            .is_none()
        {
            self.completion.deactivate(&self.state.completion_active);
            return None;
        }

        if !self.completion.active || self.completion.original_query.is_empty() {
            return None;
        }

        self.reset_history_selection_for_completion_lookup();
        let base_query = self.lookup_query().to_string();
        let suggestions = self.state.matching_word_triggers(&base_query);
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

    fn navigate_history(&mut self, older: bool) -> Option<CompletionRewrite> {
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

        if self
            .buffer
            .extract_trigger_word(self.trigger_prefix())
            .is_none()
        {
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

    fn evaluate_buffer_for_expansion(
        &mut self,
        active_window: Option<&str>,
    ) -> Option<ExpansionResult> {
        let trigger_char = self.trigger_prefix();

        if let Some(keyword) = self.buffer.extract_trigger_word(trigger_char)
            && let Some(expansion) = self.state.fetch_expansion(&keyword, active_window)
        {
            let delete_count = 1 + keyword.chars().count();
            let metric_kind = metric_kind_for_steps(expansion.is_calculation, &expansion.steps);
            self.buffer.clear();

            if let Some(template) = expansion.ai_transformer_template {
                let initial_text = self.get_initial_spinner_text(&template);
                // Template has | ai(...) or async system markers — trigger async pre-resolution before injecting.
                return Some(ExpansionResult {
                    delete_count,
                    steps: vec![ExpansionStep::Text(initial_text)],
                    trigger: keyword,
                    undo_trigger: None,
                    is_calculation: false,
                    metric_kind: AutomationMetricKind::InlineAi,
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
                metric_kind,
                track_usage: true,
                follow_up: None,
            });
        }

        // Triggerless mode: look up the bare tail word without a trigger character prefix.
        if self
            .state
            .triggerless_mode
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let mut candidates = self.buffer.extract_suffix_candidates();
            candidates.sort_by_key(|a| std::cmp::Reverse(a.0.len()));

            for (word, prev_char) in candidates {
                let is_boundary =
                    prev_char.is_none_or(|c| c.is_whitespace() || c.is_ascii_punctuation());
                if is_boundary
                    && let Some(expansion) = self.state.fetch_expansion(&word, active_window)
                {
                    let delete_count = word.chars().count();
                    let metric_kind =
                        metric_kind_for_steps(expansion.is_calculation, &expansion.steps);
                    self.buffer.clear();
                    if let Some(template) = expansion.ai_transformer_template {
                        let initial_text = self.get_initial_spinner_text(&template);
                        return Some(ExpansionResult {
                            delete_count,
                            steps: vec![ExpansionStep::Text(initial_text)],
                            trigger: word,
                            undo_trigger: None,
                            is_calculation: false,
                            metric_kind: AutomationMetricKind::InlineAi,
                            track_usage: true,
                            follow_up: Some(ExpansionFollowUp::AiTransformer {
                                template_with_markers: template,
                            }),
                        });
                    }

                    return Some(ExpansionResult {
                        delete_count,
                        steps: expansion.steps,
                        trigger: word,
                        undo_trigger: None,
                        is_calculation: expansion.is_calculation,
                        metric_kind,
                        track_usage: true,
                        follow_up: None,
                    });
                }
            }
        }

        // Regex matching fallback
        let buf_str = self.buffer.buffer_string();
        if let Some((keyword, action, captures)) =
            self.state.match_regex_action(&buf_str, active_window)
        {
            use crate::engine::catalog::expand_automation_action_with_args;
            use crate::engine::variables::ArgMap;

            let arg_map = ArgMap {
                positional: captures,
                ..Default::default()
            };

            if let Some(expansion) = expand_automation_action_with_args(action, &arg_map, &keyword)
            {
                let delete_count = keyword.chars().count();
                let metric_kind = metric_kind_for_steps(expansion.is_calculation, &expansion.steps);
                self.buffer.clear();

                if let Some(template) = expansion.ai_transformer_template {
                    let initial_text = self.get_initial_spinner_text(&template);
                    return Some(ExpansionResult {
                        delete_count,
                        steps: vec![ExpansionStep::Text(initial_text)],
                        trigger: keyword.clone(),
                        undo_trigger: None,
                        is_calculation: false,
                        metric_kind: AutomationMetricKind::InlineAi,
                        track_usage: true,
                        follow_up: Some(ExpansionFollowUp::AiTransformer {
                            template_with_markers: template,
                        }),
                    });
                }

                let undo_trigger = self
                    .allows_blind_undo(&expansion.steps)
                    .then(|| keyword.clone());

                return Some(ExpansionResult {
                    delete_count,
                    steps: expansion.steps,
                    trigger: keyword,
                    undo_trigger,
                    is_calculation: expansion.is_calculation,
                    metric_kind,
                    track_usage: true,
                    follow_up: None,
                });
            }
        }

        None
    }

    pub fn process_event(
        &mut self,
        event: EngineEvent,
        active_window: Option<&str>,
    ) -> Option<ExpansionResult> {
        use std::sync::atomic::Ordering;
        if self.state.ignore_fullscreen_enabled.load(Ordering::Relaxed)
            && self.state.is_os_fullscreen.load(Ordering::Relaxed)
        {
            self.buffer.clear();
            self.completion.deactivate(&self.state.completion_active);
            return None;
        }

        if let EngineMode::AiCapture { .. } = self.state.engine_mode() {
            return self.process_ai_capture_event(event);
        }

        match event {
            EngineEvent::Interrupt => {
                // Severe interrupts ruin active sequences
                self.buffer.clear();
                self.completion.deactivate(&self.state.completion_active);
                None
            }
            EngineEvent::Backspace => {
                if self.completion.has_selection() {
                    let _ = self.rewrite_backspace_query();
                } else {
                    // Backtrack buffer safely
                    self.buffer.pop();
                    self.sync_completion_from_buffer();
                }
                None
            }
            EngineEvent::WordBackspace => {
                if self.completion.has_selection() {
                    let _ = self.rewrite_word_backspace_query();
                } else {
                    // Backtrack a whole word
                    self.buffer.pop_word();
                    self.sync_completion_from_buffer();
                }
                None
            }
            EngineEvent::ActionDelimiter => {
                let was_completion_active = self.completion.active;
                let result = self.evaluate_buffer_for_expansion(active_window);
                if was_completion_active {
                    self.completion.deactivate(&self.state.completion_active);
                }
                if result.is_none() {
                    self.buffer.push(' ');
                }
                result
            }
            EngineEvent::Char(c) => {
                // Normal typing tracking
                self.buffer.push(c);
                self.update_completion_after_char(c);

                let mode = self.state.get_ai_delimiter_mode();
                let open_delim = match mode {
                    crate::settings::AiDelimiterMode::Symmetric => {
                        self.state.get_ai_symmetric_delimiter()
                    }
                    crate::settings::AiDelimiterMode::Asymmetric => {
                        self.state.get_ai_open_delimiter()
                    }
                };
                if self.buffer.buffer_string().ends_with(&open_delim) {
                    if self.completion.active {
                        self.completion.deactivate(&self.state.completion_active);
                    }
                    return Some(self.start_inline_ai_capture(&open_delim));
                }

                if self.state.instant_expand.load(Ordering::Relaxed)
                    && let Some(result) = self.evaluate_buffer_for_expansion(active_window)
                {
                    if self.completion.active {
                        self.completion.deactivate(&self.state.completion_active);
                    }
                    return Some(result);
                }

                None
            }
        }
    }

    fn process_ai_capture_event(&mut self, event: EngineEvent) -> Option<ExpansionResult> {
        self.buffer.clear();

        match event {
            EngineEvent::Interrupt => {
                self.state.clear_ai_prompt_buffer();
                self.state.set_engine_mode(EngineMode::Normal);
                None
            }
            EngineEvent::Backspace => {
                if self.state.is_ai_prompt_empty() {
                    self.state.set_engine_mode(EngineMode::Normal);
                    return None;
                }
                self.state.pop_ai_prompt_char();
                None
            }
            EngineEvent::WordBackspace => {
                if self.state.is_ai_prompt_empty() {
                    self.state.set_engine_mode(EngineMode::Normal);
                    return None;
                }
                self.state.pop_ai_prompt_word();
                None
            }
            EngineEvent::ActionDelimiter => {
                if let Some(expansion) = self.finish_inline_ai_capture_if_ready() {
                    return Some(expansion);
                }
                let action_delimiter = *self.state.action_delimiter.read().unwrap();
                let char_rep = match action_delimiter {
                    crate::settings::ActionDelimiter::Space => ' ',
                    crate::settings::ActionDelimiter::Enter => '\n',
                };
                self.state.append_ai_prompt_char(char_rep);
                None
            }
            EngineEvent::Char(c) => {
                if c == ' '
                    && let Some(expansion) = self.finish_inline_ai_capture_if_ready()
                {
                    return Some(expansion);
                }

                self.state.append_ai_prompt_char(c);

                None
            }
        }
    }

    fn start_inline_ai_capture(&mut self, open_delim: &str) -> ExpansionResult {
        self.buffer.clear();
        self.state.clear_ai_prompt_buffer();
        self.state.set_engine_mode(EngineMode::AiCapture {
            system_prompt_override: None,
        });
        self.completion.deactivate(&self.state.completion_active);

        ExpansionResult {
            delete_count: open_delim.chars().count(),
            steps: vec![ExpansionStep::Text(open_delim.to_string())],
            trigger: open_delim.to_string(),
            undo_trigger: None,
            is_calculation: false,
            metric_kind: AutomationMetricKind::InlineAi,
            track_usage: false,
            follow_up: None,
        }
    }

    fn finish_inline_ai_capture_if_ready(&mut self) -> Option<ExpansionResult> {
        let mode = self.state.get_ai_delimiter_mode();
        let close_delim = match mode {
            crate::settings::AiDelimiterMode::Symmetric => self.state.get_ai_symmetric_delimiter(),
            crate::settings::AiDelimiterMode::Asymmetric => self.state.get_ai_close_delimiter(),
        };

        let captured = self.state.ai_prompt_buffer();
        if !captured.ends_with(&close_delim) {
            return None;
        }

        let prompt = captured.strip_suffix(&close_delim)?;
        if prompt.is_empty() {
            return None;
        }

        let open_delim = match mode {
            crate::settings::AiDelimiterMode::Symmetric => self.state.get_ai_symmetric_delimiter(),
            crate::settings::AiDelimiterMode::Asymmetric => self.state.get_ai_open_delimiter(),
        };
        let delete_count = captured.chars().count() + open_delim.chars().count();

        let system_prompt_override = if let EngineMode::AiCapture {
            system_prompt_override,
        } = self.state.engine_mode()
        {
            system_prompt_override
        } else {
            None
        };

        self.state.clear_ai_prompt_buffer();
        self.state.set_engine_mode(EngineMode::Normal);
        self.buffer.clear();

        Some(ExpansionResult {
            delete_count,
            steps: vec![ExpansionStep::Text(self.get_thinking_text())],
            trigger: "inline_ai".to_string(),
            undo_trigger: None,
            is_calculation: false,
            metric_kind: AutomationMetricKind::InlineAi,
            track_usage: false,
            follow_up: Some(ExpansionFollowUp::InlineAi {
                prompt: prompt.to_string(),
                system_prompt_override,
            }),
        })
    }
}

fn metric_kind_for_steps(is_calculation: bool, steps: &[ExpansionStep]) -> AutomationMetricKind {
    if is_calculation {
        return AutomationMetricKind::Calculation;
    }

    if matches!(steps, [ExpansionStep::Script(_)]) {
        return AutomationMetricKind::Script;
    }

    AutomationMetricKind::Snippet
}

fn pop_char_from_query(query: &str) -> String {
    let mut next = query.to_string();
    let _ = next.pop();
    next
}

fn pop_word_from_query(query: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    trait EvaluatorTestExt {
        fn process(&mut self, event: EngineEvent) -> Option<ExpansionResult>;
    }
    impl EvaluatorTestExt for Evaluator {
        fn process(&mut self, event: EngineEvent) -> Option<ExpansionResult> {
            self.process_event(event, None)
        }
    }

    use tracing::error;

    fn assert_no_follow_up(result: &ExpansionResult) {
        assert_eq!(result.follow_up, None);
    }

    fn assert_inline_ai_follow_up(
        result: &ExpansionResult,
        prompt: &str,
        system_prompt_override: Option<&str>,
    ) {
        assert_eq!(
            result.follow_up,
            Some(ExpansionFollowUp::InlineAi {
                prompt: prompt.to_string(),
                system_prompt_override: system_prompt_override.map(str::to_string),
            })
        );
    }

    fn setup() -> Evaluator {
        let state = Arc::new(EngineState::new('/'));
        state.load_actions(vec![
            (
                "gm".to_string(),
                crate::db::crud::AutomationAction::text("Good morning!"),
            ),
            (
                "shrug".to_string(),
                crate::db::crud::AutomationAction::text(r#"¯\_(ツ)_/¯"#),
            ),
        ]);
        Evaluator::new(state)
    }

    fn assert_completion_rewrite(
        rewrite: Option<CompletionRewrite>,
        delete_count: usize,
        replacement: &str,
    ) {
        assert_eq!(
            rewrite,
            Some(CompletionRewrite {
                delete_count,
                replacement: replacement.to_string(),
            })
        );
    }

    #[test]
    fn typing_trigger_char_enters_completion_state() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);

        assert!(eval.is_completion_active());
        assert_eq!(eval.completion.original_query, "");
        assert_eq!(eval.completion.current_text, "");
        assert!(eval.completion.suggestions.is_empty());
    }

    #[test]
    fn completion_query_tracks_typed_chars_and_ignores_hotkeys() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gpush".to_string(),
                crate::db::crud::AutomationAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
            (
                "gco".to_string(),
                crate::db::crud::AutomationAction::text("git checkout"),
            ),
        ]);
        state.load_hotkey_actions(vec![(
            "ctrl+shift+g".to_string(),
            crate::db::crud::AutomationAction::text("hotkey"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert!(eval.is_completion_active());
        assert_eq!(eval.completion.original_query, "g");
        assert_eq!(eval.completion.current_text, "g");
        assert_eq!(
            eval.completion.suggestions,
            vec!["gco".to_string(), "gpush".to_string(), "gs".to_string()]
        );
    }

    #[test]
    fn completion_tab_with_empty_query_is_swallowed_without_rewrite() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_eq!(eval.cycle_completion_next(), None);
        assert!(eval.is_completion_active());
        assert_eq!(eval.completion.current_text, "");
        assert!(eval.completion.selected_index.is_none());
    }

    #[test]
    fn completion_tab_cycles_sorted_matches_and_wraps() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
            (
                "gpush".to_string(),
                crate::db::crud::AutomationAction::text("git push"),
            ),
            (
                "gco".to_string(),
                crate::db::crud::AutomationAction::text("git checkout"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.cycle_completion_next(), 1, "gco");
        assert_eq!(
            eval.buffer.extract_trigger_word('>'),
            Some("gco".to_string())
        );
        assert_eq!(eval.completion.current_text, "gco");
        assert_eq!(eval.completion.selected_index, Some(0));

        assert_completion_rewrite(eval.cycle_completion_next(), 3, "gpush");
        assert_eq!(
            eval.buffer.extract_trigger_word('>'),
            Some("gpush".to_string())
        );
        assert_eq!(eval.completion.selected_index, Some(1));

        assert_completion_rewrite(eval.cycle_completion_next(), 5, "gs");
        assert_eq!(
            eval.buffer.extract_trigger_word('>'),
            Some("gs".to_string())
        );
        assert_eq!(eval.completion.selected_index, Some(2));

        assert_completion_rewrite(eval.cycle_completion_next(), 2, "gco");
        assert_eq!(
            eval.buffer.extract_trigger_word('>'),
            Some("gco".to_string())
        );
        assert_eq!(eval.completion.selected_index, Some(0));
    }

    #[test]
    fn completion_shift_tab_cycles_backward_and_wraps() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
            (
                "gpush".to_string(),
                crate::db::crud::AutomationAction::text("git push"),
            ),
            (
                "gco".to_string(),
                crate::db::crud::AutomationAction::text("git checkout"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.cycle_completion_prev(), 1, "gs");
        assert_eq!(eval.completion.selected_index, Some(2));

        assert_completion_rewrite(eval.cycle_completion_prev(), 2, "gpush");
        assert_eq!(eval.completion.selected_index, Some(1));

        assert_completion_rewrite(eval.cycle_completion_prev(), 5, "gco");
        assert_eq!(eval.completion.selected_index, Some(0));
    }

    #[test]
    fn completion_tab_with_no_matches_is_swallowed_without_rewrite() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">z".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_eq!(eval.cycle_completion_next(), None);
        assert!(eval.is_completion_active());
        assert_eq!(eval.completion.current_text, "z");
        assert!(eval.completion.selected_index.is_none());
    }

    #[test]
    fn completion_cancel_leaves_buffer_text_untouched() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        for c in ">gs".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        eval.cancel_completion();

        assert!(!eval.is_completion_active());
        assert_eq!(
            eval.buffer.extract_trigger_word('>'),
            Some("gs".to_string())
        );
    }

    #[test]
    fn completion_backspace_updates_query_and_exits_after_trigger_is_removed() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
            (
                "gpush".to_string(),
                crate::db::crud::AutomationAction::text("git push"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">gs".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_eq!(eval.process(EngineEvent::Backspace), None);
        assert!(eval.is_completion_active());
        assert_eq!(eval.completion.current_text, "g");
        assert_eq!(eval.completion.original_query, "g");
        assert_eq!(
            eval.completion.suggestions,
            vec!["gpush".to_string(), "gs".to_string()]
        );

        assert_eq!(eval.process(EngineEvent::Backspace), None);
        assert!(eval.is_completion_active());
        assert_eq!(eval.completion.current_text, "");
        assert_eq!(eval.buffer.extract_trigger_word('>'), Some(String::new()));

        assert_eq!(eval.process(EngineEvent::Backspace), None);
        assert!(!eval.is_completion_active());
        assert_eq!(eval.buffer.extract_trigger_word('>'), None);
        assert_eq!(eval.cycle_completion_next(), None);
    }

    #[test]
    fn completion_space_after_rewrite_uses_existing_word_expansion_path() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gco".to_string(),
                crate::db::crud::AutomationAction::text("git checkout"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.cycle_completion_next(), 1, "gco");
        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("space should expand the selected completion");

        assert_eq!(result.trigger, "gco");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("git checkout".to_string())]
        );
        assert!(!eval.is_completion_active());
    }

    #[test]
    fn completion_does_not_break_inline_ai_capture_priority() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let result = eval
            .process(EngineEvent::Char('>'))
            .expect("inline ai capture should start");

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert_eq!(result.trigger, ">>");
        assert!(!eval.is_completion_active());
        assert_eq!(eval.completion.selection_mode, None);
        assert_eq!(eval.navigate_history_older(), None);
    }

    #[test]
    fn history_up_selects_most_recent_trigger_and_stops_at_oldest() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "email".to_string(),
                crate::db::crud::AutomationAction::text("team update"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
            (
                "uuid".to_string(),
                crate::db::crud::AutomationAction::text("1234"),
            ),
        ]);
        state.load_word_trigger_history(vec![
            "gs".to_string(),
            "email".to_string(),
            "uuid".to_string(),
        ]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);

        assert_completion_rewrite(eval.navigate_history_older(), 0, "gs");
        assert_eq!(eval.completion.history_index, Some(0));
        assert_eq!(eval.completion.current_text, "gs");

        assert_completion_rewrite(eval.navigate_history_older(), 2, "email");
        assert_eq!(eval.completion.history_index, Some(1));
        assert_eq!(eval.completion.current_text, "email");

        assert_completion_rewrite(eval.navigate_history_older(), 5, "uuid");
        assert_eq!(eval.completion.history_index, Some(2));
        assert_eq!(eval.completion.current_text, "uuid");

        assert_eq!(eval.navigate_history_older(), None);
        assert_eq!(eval.completion.history_index, Some(2));
        assert_eq!(eval.completion.current_text, "uuid");
    }

    #[test]
    fn history_down_restores_original_query_after_prefix_filtered_navigation() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "email".to_string(),
                crate::db::crud::AutomationAction::text("team update"),
            ),
            (
                "gpush".to_string(),
                crate::db::crud::AutomationAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
            (
                "uuid".to_string(),
                crate::db::crud::AutomationAction::text("1234"),
            ),
        ]);
        state.load_word_trigger_history(vec![
            "gs".to_string(),
            "email".to_string(),
            "gpush".to_string(),
            "uuid".to_string(),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.navigate_history_older(), 1, "gs");
        assert_eq!(
            eval.completion.history_items,
            vec!["gs".to_string(), "gpush".to_string()]
        );
        assert_eq!(eval.completion.history_index, Some(0));

        assert_completion_rewrite(eval.navigate_history_older(), 2, "gpush");
        assert_eq!(eval.completion.history_index, Some(1));

        assert_completion_rewrite(eval.navigate_history_newer(), 5, "gs");
        assert_eq!(eval.completion.history_index, Some(0));

        assert_completion_rewrite(eval.navigate_history_newer(), 2, "g");
        assert_eq!(eval.completion.history_index, None);
        assert_eq!(eval.completion.current_text, "g");
        assert_eq!(eval.completion.original_query, "g");

        assert_eq!(eval.navigate_history_newer(), None);
    }

    #[test]
    fn history_backspace_after_selection_clears_history_selection_and_treats_buffer_as_query() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gpush".to_string(),
                crate::db::crud::AutomationAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string(), "gpush".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.navigate_history_older(), 1, "gs");
        assert_eq!(eval.completion.history_index, Some(0));

        assert_eq!(eval.process(EngineEvent::Backspace), None);
        assert_eq!(eval.completion.current_text, "");
        assert_eq!(eval.completion.original_query, "");
        assert_eq!(eval.completion.history_index, None);
        assert_eq!(eval.completion.selection_mode, None);
        assert_eq!(
            eval.completion.history_items,
            vec!["gs".to_string(), "gpush".to_string()]
        );
        assert_eq!(eval.buffer.extract_trigger_word('>'), Some(String::new()));
    }

    #[test]
    fn history_backspace_edits_original_query_not_selected_history_item() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gitstatus".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        state.load_word_trigger_history(vec!["gitstatus".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">git".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.navigate_history_older(), 3, "gitstatus");
        assert_eq!(eval.completion.current_text, "gitstatus");

        assert_eq!(eval.process(EngineEvent::Backspace), None);
        assert_eq!(eval.completion.original_query, "gi");
        assert_eq!(eval.completion.current_text, "gi");
        assert_eq!(
            eval.buffer.extract_trigger_word('>'),
            Some("gi".to_string())
        );
        assert_eq!(eval.completion.history_index, None);
        assert_eq!(eval.completion.selection_mode, None);
    }

    #[test]
    fn completion_backspace_edits_original_query_not_selected_completion() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gpush".to_string(),
                crate::db::crud::AutomationAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let _ = eval
            .cycle_completion_next()
            .expect("completion should select");
        assert_eq!(eval.process(EngineEvent::Backspace), None);
        assert_eq!(eval.completion.original_query, "");
        assert_eq!(eval.completion.current_text, "");
        assert_eq!(eval.completion.selected_index, None);
        assert_eq!(eval.completion.selection_mode, None);
        assert_eq!(eval.buffer.extract_trigger_word('>'), Some(String::new()));
    }

    #[test]
    fn history_space_after_selection_uses_existing_word_expansion_path() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        state.load_word_trigger_history(vec!["gs".to_string()]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_completion_rewrite(eval.navigate_history_older(), 0, "gs");

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("space should expand the selected history entry");

        assert_eq!(result.trigger, "gs");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("git status".to_string())]
        );
        assert!(!eval.is_completion_active());
    }

    #[test]
    fn history_navigation_uses_original_query_even_after_tab_completion_rewrite() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gpush".to_string(),
                crate::db::crud::AutomationAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
            (
                "gaa".to_string(),
                crate::db::crud::AutomationAction::text("git add --all"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string(), "gpush".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.cycle_completion_next(), 1, "gaa");
        assert_eq!(eval.completion.original_query, "g");

        assert_completion_rewrite(eval.navigate_history_older(), 3, "gs");
        assert_eq!(eval.completion.original_query, "g");
        assert_eq!(eval.completion.history_index, Some(0));
        assert_eq!(
            eval.completion.selection_mode,
            Some(TriggerAssistSelectionMode::History)
        );
    }

    #[test]
    fn history_to_completion_mode_switch_uses_original_query_prefix() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gaa".to_string(),
                crate::db::crud::AutomationAction::text("git add --all"),
            ),
            (
                "gpm".to_string(),
                crate::db::crud::AutomationAction::text("git push master"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string(), "gpm".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.navigate_history_older(), 1, "gs");
        assert_eq!(eval.completion.original_query, "g");

        assert_completion_rewrite(eval.cycle_completion_next(), 2, "gaa");
        assert_eq!(eval.completion.original_query, "g");
        assert_eq!(eval.completion.selected_index, Some(0));
        assert_eq!(
            eval.completion.selection_mode,
            Some(TriggerAssistSelectionMode::Completion)
        );
    }

    #[test]
    fn completion_to_history_mode_switch_uses_original_query_prefix() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gaa".to_string(),
                crate::db::crud::AutomationAction::text("git add --all"),
            ),
            (
                "gpm".to_string(),
                crate::db::crud::AutomationAction::text("git push master"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string(), "gpm".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.cycle_completion_next(), 1, "gaa");
        assert_eq!(eval.completion.original_query, "g");

        assert_completion_rewrite(eval.navigate_history_older(), 3, "gs");
        assert_eq!(eval.completion.original_query, "g");
        assert_eq!(eval.completion.history_index, Some(0));
        assert_eq!(eval.completion.selected_index, None);
    }

    #[test]
    fn history_then_completion_then_space_expands_visible_trigger() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gaa".to_string(),
                crate::db::crud::AutomationAction::text("git add --all"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.navigate_history_older(), 1, "gs");
        assert_completion_rewrite(eval.cycle_completion_next(), 2, "gaa");

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("space should expand the currently visible completion");

        assert_eq!(result.trigger, "gaa");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("git add --all".to_string())]
        );
    }

    #[test]
    fn inline_tab_completion_setting_disables_completion_rewrites() {
        use std::sync::atomic::Ordering;

        let state = Arc::new(EngineState::new('>'));
        state
            .inline_tab_completion_enabled
            .store(false, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_eq!(eval.cycle_completion_next(), None);
        assert_eq!(eval.cycle_completion_prev(), None);
        assert_eq!(eval.completion.current_text, "g");
        assert_eq!(eval.completion.original_query, "g");
        assert!(eval.completion.suggestions.is_empty());
        assert_eq!(eval.buffer.extract_trigger_word('>'), Some("g".to_string()));
    }

    #[test]
    fn inline_history_setting_disables_history_rewrites() {
        use std::sync::atomic::Ordering;

        let state = Arc::new(EngineState::new('>'));
        state.inline_history_enabled.store(false, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        state.load_word_trigger_history(vec!["gs".to_string()]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);

        assert_eq!(eval.navigate_history_older(), None);
        assert_eq!(eval.navigate_history_newer(), None);
        assert_eq!(eval.completion.current_text, "");
        assert_eq!(eval.completion.original_query, "");
        assert!(eval.completion.history_items.is_empty());
        assert_eq!(eval.buffer.extract_trigger_word('>'), Some(String::new()));
    }

    #[test]
    fn tab_completion_still_works_when_history_is_disabled() {
        use std::sync::atomic::Ordering;

        let state = Arc::new(EngineState::new('>'));
        state.inline_history_enabled.store(false, Ordering::Relaxed);
        state.load_actions(vec![
            (
                "gco".to_string(),
                crate::db::crud::AutomationAction::text("git checkout"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::AutomationAction::text("git status"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.cycle_completion_next(), 1, "gco");
    }

    #[test]
    fn history_still_works_when_tab_completion_is_disabled() {
        use std::sync::atomic::Ordering;

        let state = Arc::new(EngineState::new('>'));
        state
            .inline_tab_completion_enabled
            .store(false, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        state.load_word_trigger_history(vec!["gs".to_string()]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);

        assert_completion_rewrite(eval.navigate_history_older(), 0, "gs");
    }

    #[test]
    fn disabling_inline_assist_does_not_break_word_expansion_or_inline_ai() {
        use std::sync::atomic::Ordering;

        let state = Arc::new(EngineState::new('>'));
        state
            .inline_tab_completion_enabled
            .store(false, Ordering::Relaxed);
        state.inline_history_enabled.store(false, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);

        let mut eval = Evaluator::new(state.clone());
        for c in ">gs".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let expansion = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("word trigger expansion should still work");
        assert_eq!(expansion.trigger, "gs");

        let mut ai_eval = Evaluator::new(state.clone());
        assert_eq!(ai_eval.process(EngineEvent::Char('>')), None);
        let ai_result = ai_eval
            .process(EngineEvent::Char('>'))
            .expect("inline ai should start");
        assert_eq!(ai_result.trigger, ">>");
        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
    }

    #[test]
    fn test_standard_typing_no_trigger() {
        let mut eval = setup();
        for c in "hello world".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }
        // Buffer should have successfully recorded the string
        assert_eq!(eval.buffer.len, 11);
    }

    #[test]
    fn test_successful_trigger_requires_space() {
        let mut eval = setup();
        // Type standard string leading to a trigger
        for c in "Hello /gm".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        // Exact sequence matching should occur when space fires
        let result = eval.process(EngineEvent::ActionDelimiter).unwrap();
        // delete_count = '/' (1) + "gm" (2) = 3
        assert_eq!(result.delete_count, 3);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("Good morning!".to_string())]
        );
        assert!(result.track_usage);
        assert_no_follow_up(&result);

        // State machine buffer should reset upon expansion
        assert_eq!(eval.buffer.len, 0);
    }

    #[test]
    fn test_interrupt_ruins_active_sequence() {
        let mut eval = setup();
        // Type half of a sequence
        for c in "/gm".chars() {
            eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            });
        }

        // An interrupt (e.g. mouse click) happens
        eval.process(EngineEvent::Interrupt);

        // The space no longer expands because the buffer was wiped
        assert_eq!(eval.process(EngineEvent::ActionDelimiter), None);
    }

    #[test]
    fn test_backspace_supports_typo_correction() {
        let mut eval = setup();
        // Type string with typo: /gn
        for c in "/gn".chars() {
            eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            });
        }

        // Delete 'n'
        eval.process(EngineEvent::Backspace);

        // Retype 'm'
        eval.process(EngineEvent::Char('m'));

        // Fire expansion
        let result = eval.process(EngineEvent::ActionDelimiter).unwrap();
        assert_eq!(result.delete_count, 3);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("Good morning!".to_string())]
        );
        assert_eq!(result.undo_trigger.as_deref(), Some("/gm"));
        assert!(!result.is_calculation);
        assert!(result.track_usage);
        assert_no_follow_up(&result);
    }

    #[test]
    fn cursor_templates_skip_blind_undo_registration() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "sig".to_string(),
            crate::db::crud::AutomationAction::text("Best,\n[cursor]\nErin"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">sig".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("cursor template should expand");
        assert_eq!(result.undo_trigger, None);
        assert!(
            result
                .steps
                .iter()
                .any(|step| matches!(step, ExpansionStep::KeyPress(alias) if alias == "left"))
        );
    }

    #[test]
    fn inline_run_templates_skip_blind_undo_registration() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "runme".to_string(),
            crate::db::crud::AutomationAction::text("before [exec.bash(echo hi)] after"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">runme".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("run template should expand");
        assert_eq!(result.undo_trigger, None);
        assert!(
            result
                .steps
                .iter()
                .any(|step| matches!(step, ExpansionStep::InlineRun(_, _)))
        );
    }

    #[test]
    fn clipboard_payload_at_history_ceiling_skips_blind_undo_registration() {
        crate::engine::variables::system::clip::set_mock_clip(Some("x".repeat(MAX_PAYLOAD_BYTES)));

        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "clip".to_string(),
            crate::db::crud::AutomationAction::text("[clip]"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">clip".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("clipboard template should expand");
        assert_eq!(result.undo_trigger, None);

        crate::engine::variables::system::clip::set_mock_clip(None);
    }

    #[test]
    fn test_longer_keyword_has_correct_delete_count() {
        let mut eval = setup();
        // "/shrug" = 1 trigger + 5 keyword + 1 space = 7
        for c in "/shrug".chars() {
            eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            });
        }
        let result = eval.process(EngineEvent::ActionDelimiter).unwrap();
        assert_eq!(result.delete_count, 6);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(r#"¯\_(ツ)_/¯"#.to_string())]
        );
        assert!(result.track_usage);
        assert_no_follow_up(&result);
    }

    #[test]
    fn test_unknown_trigger_does_not_expand() {
        let mut eval = setup();
        for c in "/unknown".chars() {
            eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            });
        }
        assert_eq!(eval.process(EngineEvent::ActionDelimiter), None);
    }

    #[test]
    fn test_multiple_trigger_chars_rejects_ambiguous_sequence() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "brb".to_string(),
                crate::db::crud::AutomationAction::text("Be right back!"),
            ),
            (
                "gm".to_string(),
                crate::db::crud::AutomationAction::text("Good morning!"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">brb>gm".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }
        // Ambiguous: two `>` in one span — do not expand with a partial delete.
        assert_eq!(eval.process(EngineEvent::ActionDelimiter), None);
    }

    /// Simulates two separate expansions in a row: first snippet finishes (buffer cleared), then
    /// user types the second trigger — must not merge or double-fire.
    #[test]
    fn test_back_to_back_separate_triggers_like_user_typing_brb_then_gm() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "brb".to_string(),
                crate::db::crud::AutomationAction::text("Be right back!"),
            ),
            (
                "gm".to_string(),
                crate::db::crud::AutomationAction::text("Good morning!"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">brb ".chars() {
            if c == ' ' {
                let r = eval.process(EngineEvent::ActionDelimiter).unwrap();
                assert_eq!(
                    r.steps,
                    vec![ExpansionStep::Text("Be right back!".to_string())]
                );
                assert_eq!(r.delete_count, 1 + "brb".len());
            } else {
                assert_eq!(
                    eval.process(if c == ' ' {
                        EngineEvent::ActionDelimiter
                    } else {
                        EngineEvent::Char(c)
                    }),
                    None
                );
            }
        }
        assert_eq!(eval.buffer.len, 0);

        for c in ">gm ".chars() {
            if c == ' ' {
                let r = eval.process(EngineEvent::ActionDelimiter).unwrap();
                assert_eq!(
                    r.steps,
                    vec![ExpansionStep::Text("Good morning!".to_string())]
                );
                assert_eq!(r.delete_count, 1 + "gm".len());
            } else {
                assert_eq!(
                    eval.process(if c == ' ' {
                        EngineEvent::ActionDelimiter
                    } else {
                        EngineEvent::Char(c)
                    }),
                    None
                );
            }
        }
        assert_eq!(eval.buffer.len, 0);
    }

    /// Same keyword twice in a row must yield two independent expansions (no merged buffer).
    #[test]
    fn test_same_trigger_twice_in_a_row_two_expansions() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::AutomationAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state);

        for _ in 0..2 {
            for c in ">gm ".chars() {
                if c == ' ' {
                    let r = eval.process(EngineEvent::ActionDelimiter).unwrap();
                    assert_eq!(
                        r.steps,
                        vec![ExpansionStep::Text("Good morning!".to_string())]
                    );
                    assert_eq!(r.delete_count, 1 + 2);
                } else {
                    assert_eq!(
                        eval.process(if c == ' ' {
                            EngineEvent::ActionDelimiter
                        } else {
                            EngineEvent::Char(c)
                        }),
                        None
                    );
                }
            }
            assert_eq!(eval.buffer.len, 0);
        }
    }

    /// After a failed match (unknown keyword), a later valid trigger on a fresh suffix must work.
    #[test]
    fn test_unknown_keyword_then_valid_trigger_still_expands() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::AutomationAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">nope ".chars() {
            if c == ' ' {
                assert_eq!(eval.process(EngineEvent::ActionDelimiter), None);
            } else {
                assert_eq!(
                    eval.process(if c == ' ' {
                        EngineEvent::ActionDelimiter
                    } else {
                        EngineEvent::Char(c)
                    }),
                    None
                );
            }
        }
        assert!(eval.buffer.len > 0);

        eval.process(EngineEvent::Interrupt);
        for c in ">gm ".chars() {
            if c == ' ' {
                let r = eval.process(EngineEvent::ActionDelimiter).unwrap();
                assert_eq!(
                    r.steps,
                    vec![ExpansionStep::Text("Good morning!".to_string())]
                );
            } else {
                assert_eq!(
                    eval.process(if c == ' ' {
                        EngineEvent::ActionDelimiter
                    } else {
                        EngineEvent::Char(c)
                    }),
                    None
                );
            }
        }
    }

    #[test]
    fn test_end_to_end_dynamic_variable_expansion() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "repo".to_string(),
            crate::db::crud::AutomationAction::text("https://github.com/[0=org]/[1=repo]"),
        )]);
        let mut eval = Evaluator::new(state);

        let input = r#"Hello >repo:"ereinaimer":"taurine" "#;
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            }) {
                last_result = Some(res);
            }
        }

        let result = last_result.expect("Expansion should have triggered on the space");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(
                "https://github.com/ereinaimer/taurine".to_string()
            )]
        );
        assert_eq!(result.trigger, r#"repo:"ereinaimer":"taurine""#);
        // trigger_char + keyword + space
        assert_eq!(result.delete_count, 1 + result.trigger.len());
    }

    #[test]
    fn test_end_to_end_dynamic_variable_named_args_and_defaults() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gh".to_string(),
            crate::db::crud::AutomationAction::text("https://github.com/[username]/[repo=taurine]"),
        )]);
        let mut eval = Evaluator::new(state);

        let input = r#">gh:"username=ereinaimer" "#;
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            }) {
                last_result = Some(res);
            }
        }

        let result = last_result.expect("Expansion should have triggered");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(
                "https://github.com/ereinaimer/taurine".to_string()
            )]
        );
        assert_eq!(result.trigger, r#"gh:"username=ereinaimer""#);
    }
    #[test]
    fn test_backspace_with_args_bug() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gh".to_string(),
            crate::db::crud::AutomationAction::text("https://github.com/ereinaimer/taurine"),
        )]);
        let mut eval = Evaluator::new(state);

        let input = ">gh:blah";
        for c in input.chars() {
            eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            });
        }

        // Backspace blah (WordBackspace)
        eval.process(EngineEvent::WordBackspace);

        let input2 = "irrelevant";
        for c in input2.chars() {
            eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            });
        }

        let result = eval.process(EngineEvent::ActionDelimiter);
        let result = result.expect("Expansion should have triggered");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(
                "https://github.com/ereinaimer/taurine".to_string()
            )]
        );
    }

    #[test]
    fn test_inline_math_evaluation_simple() {
        let state = Arc::new(EngineState::new('>'));
        // No snippets loaded. Math should act as fallback.
        let mut eval = Evaluator::new(state);

        let input = ">5+2 ";
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            }) {
                last_result = Some(res);
            }
        }

        let result = last_result.expect("Math expansion should have triggered");
        assert_eq!(result.steps, vec![ExpansionStep::Text("7".to_string())]);
        assert_eq!(result.trigger, "5+2");
        assert_eq!(result.undo_trigger.as_deref(), Some(">5+2"));
        assert!(result.is_calculation);
        assert!(result.track_usage);
        assert_no_follow_up(&result);
    }

    #[test]
    fn test_inline_math_evaluation_complex() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        let input = ">((5+2)/7%2)*2 ";
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            }) {
                last_result = Some(res);
            }
        }

        let result = last_result.expect("Math expansion should have triggered");
        // ((5+2) / 7 % 2) * 2 = (7 / 7 % 2) * 2 = (1 % 2) * 2 = 1 * 2 = 2
        assert_eq!(result.steps, vec![ExpansionStep::Text("2".to_string())]);
        assert_eq!(result.trigger, "((5+2)/7%2)*2");
        assert!(result.track_usage);
        assert_no_follow_up(&result);
    }

    #[test]
    fn test_inline_math_rounding() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        let input = ">(5+3)/7 ";
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionDelimiter
            } else {
                EngineEvent::Char(c)
            }) {
                last_result = Some(res);
            }
        }

        let result = last_result.expect("Math expansion should have triggered");
        // (5+3)/7 = 8/7 = 1.142857... rounds to 1.1429
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("1.1429".to_string())]
        );
        assert!(result.track_usage);
        assert_no_follow_up(&result);
    }

    #[test]
    fn test_inline_math_bedmas() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        let cases = vec![
            ("2+3*4", "14"),
            ("(2+3)*4", "20"),
            ("10-2^3", "2"),
            ("2^3^2", "512"), // Right-associative
            ("-2^2", "-4"),   // Unary minus precedence
            ("4/2*3", "6"),   // Left-to-right DM
            ("10%3^2", "1"),
            ("2*(3+4)^2", "98"),
            ("1+2*3/4-5%2", "1.5"), // Mixed
            ("1.5e3+100", "1600"),
            ("2E-2", "0.02"),
            ("sqrt(16)", "4"),
            ("abs(-55)", "55"),
            ("floor(4.9)", "4"),
            ("round(4.5)", "5"),
            ("2(3+4)", "14"),
            ("2pi", "6.2832"),
            ("(2)(3)", "6"),
            ("2^3(4)", "32"),
        ];

        for (input_str, expected) in cases {
            eval.buffer.clear();
            let mut result = None;
            for c in format!(">{} ", input_str).chars() {
                if let Some(res) = eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }) {
                    result = Some(res);
                }
            }
            let res = result.unwrap_or_else(|| {
                error!("Failed to expand: {}", input_str);
                panic!("Failed to expand: {}", input_str);
            });
            assert_eq!(
                res.steps,
                vec![ExpansionStep::Text(expected.to_string())],
                "Failed case: {}",
                input_str
            );
        }
    }

    #[test]
    fn inline_ai_capture_trigger_enters_micro_state_and_paints_opening_delimiter() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let result = eval
            .process(EngineEvent::Char('>'))
            .expect("Should enter capture");

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert_eq!(result.trigger, ">>");
        assert_eq!(result.steps, vec![ExpansionStep::Text(">>".to_string())]);
        assert_eq!(result.undo_trigger, None);
        assert_no_follow_up(&result);
    }

    #[test]
    fn inline_ai_capture_exits_on_backtick_then_space_and_hands_prompt_to_stream() {
        let state = Arc::new(EngineState::new('>'));
        *state.action_delimiter.write().unwrap() = crate::settings::ActionDelimiter::Space;
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));

        for c in "What is Rust?<<".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("closing backtick plus space should submit captured prompt");

        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert_eq!(state.ai_prompt_buffer(), "");
        assert_eq!(result.delete_count, "What is Rust?<<".chars().count() + 2);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(eval.get_thinking_text())]
        );
        assert_eq!(result.undo_trigger, None);
        assert_inline_ai_follow_up(&result, "What is Rust?", None);
    }

    #[test]
    fn inline_ai_success_path_returns_to_normal_and_allows_later_word_expansion() {
        let state = Arc::new(EngineState::new('>'));
        *state.action_delimiter.write().unwrap() = crate::settings::ActionDelimiter::Space;
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::AutomationAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));
        for c in "What is Rust?<<".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let ai_result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("inline ai follow-up should dispatch on closing delimiter plus space");
        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert_inline_ai_follow_up(&ai_result, "What is Rust?", None);

        for c in ">gm".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let expansion = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("normal word trigger should still expand after inline ai success");
        assert_eq!(
            expansion.steps,
            vec![ExpansionStep::Text("Good morning!".to_string())]
        );
    }

    #[test]
    fn test_ai_capture_interrupted_by_esc_reverts_to_normal() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::AutomationAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));
        for c in "draft".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_eq!(eval.process(EngineEvent::Interrupt), None);
        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert!(state.is_ai_prompt_empty());

        for c in ">gm".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let expansion = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("normal word trigger should still expand after inline ai cancelled");
        assert_eq!(
            expansion.steps,
            vec![ExpansionStep::Text("Good morning!".to_string())]
        );
    }

    #[test]
    fn test_ai_capture_backspaced_to_empty_reverts_to_normal() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::AutomationAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert!(state.is_ai_prompt_empty());
        assert_eq!(eval.process(EngineEvent::Backspace), None);
        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert!(state.is_ai_prompt_empty());

        for c in ">gm".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }
        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("normal trigger should work after empty-buffer backspace exits capture");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("Good morning!".to_string())]
        );
    }

    #[test]
    fn test_ai_capture_word_backspaced_to_empty_reverts_to_normal() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::AutomationAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert!(state.is_ai_prompt_empty());
        assert_eq!(eval.process(EngineEvent::WordBackspace), None);
        assert_eq!(state.engine_mode(), EngineMode::Normal);

        for c in ">gm".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }
        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("normal trigger should work after empty-buffer word-backspace exits capture");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("Good morning!".to_string())]
        );
    }

    #[test]
    fn test_ai_capture_finish_with_asymmetric_delimiters() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));
        for c in "prompt<<".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("action delimiter should submit captured asymmetric prompt");

        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert!(state.is_ai_prompt_empty());
        assert_eq!(result.delete_count, "prompt<<".chars().count() + 2);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(eval.get_thinking_text())]
        );
        assert_inline_ai_follow_up(&result, "prompt", None);
    }

    #[test]
    fn test_ai_capture_finish_with_symmetric_delimiters() {
        let state = Arc::new(EngineState::new('>'));
        state.set_ai_delimiter_mode(crate::settings::AiDelimiterMode::Symmetric);
        state.set_ai_symmetric_delimiter("^".to_string());
        let mut eval = Evaluator::new(state.clone());

        let _ = eval.process(EngineEvent::Char('^'));
        for c in "prompt^".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("action delimiter should submit captured symmetric prompt");

        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert!(state.is_ai_prompt_empty());
        assert_eq!(result.delete_count, "prompt^".chars().count() + 1);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(eval.get_thinking_text())]
        );
        assert_inline_ai_follow_up(&result, "prompt", None);
    }

    #[test]
    fn inline_ai_capture_keeps_collecting_without_closing_backtick_space() {
        let state = Arc::new(EngineState::new('>'));
        *state.action_delimiter.write().unwrap() = crate::settings::ActionDelimiter::Space;
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));

        for c in "draft prompt ".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert_eq!(state.ai_prompt_buffer(), "draft prompt ");
    }

    #[test]
    fn inline_ai_thinking_text_matches_spec() {
        let state = Arc::new(EngineState::new('>'));
        let eval = Evaluator::new(state);
        assert_eq!(eval.get_thinking_text(), "⠋");
    }

    #[test]
    fn inline_ai_capture_works_with_custom_delimiter() {
        let state = Arc::new(EngineState::new('>'));
        *state.action_delimiter.write().unwrap() = crate::settings::ActionDelimiter::Space;
        state.set_ai_delimiter_mode(crate::settings::AiDelimiterMode::Asymmetric);
        state.set_ai_open_delimiter("[[".to_string());
        state.set_ai_close_delimiter("]]".to_string());
        let mut eval = Evaluator::new(state.clone());

        // 1. Enter capture
        assert_eq!(eval.process(EngineEvent::Char('[')), None);
        let start_res = eval
            .process(EngineEvent::Char('['))
            .expect("Should enter capture");

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert_eq!(start_res.steps, vec![ExpansionStep::Text("[[".to_string())]);

        // 2. Type prompt
        for c in "Hello AI]]".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionDelimiter
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        // 3. Finish capture
        let finish_res = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("Should finish capture");
        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert_inline_ai_follow_up(&finish_res, "Hello AI", None);
    }

    #[test]
    fn triggerless_mode_expands_bare_words() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(true, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "gs".chars() {
            eval.process(EngineEvent::Char(c));
        }

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("triggerless match");
        assert_eq!(result.delete_count, 2);
        assert_eq!(result.trigger, "gs");
        assert_eq!(result.undo_trigger, None);
    }

    #[test]
    fn triggerless_mode_does_not_fire_when_disabled() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(false, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "gs".chars() {
            eval.process(EngineEvent::Char(c));
        }

        assert_eq!(eval.process(EngineEvent::ActionDelimiter), None);
    }

    #[test]
    fn triggerless_mode_fires_with_punctuation_prefix() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(true, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "(gs".chars() {
            eval.process(EngineEvent::Char(c));
        }

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("should expand triggerless with punctuation prefix");
        assert_eq!(result.delete_count, 2);
        assert_eq!(result.trigger, "gs");
    }

    #[test]
    fn triggered_mode_still_works_when_triggerless_enabled() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(true, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">gs".chars() {
            eval.process(EngineEvent::Char(c));
        }

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("triggered match");
        assert_eq!(result.delete_count, 3);
        assert_eq!(result.trigger, "gs");
        assert_eq!(result.undo_trigger, Some(">gs".to_string()));
    }

    #[test]
    fn instant_expand_triggered() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.instant_expand.store(true, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_eq!(eval.process(EngineEvent::Char('g')), None);

        let result = eval
            .process(EngineEvent::Char('s'))
            .expect("Should expand instantly");
        assert_eq!(result.delete_count, 3);
        assert_eq!(result.trigger, "gs");
    }

    #[test]
    fn instant_expand_triggerless() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.instant_expand.store(true, Ordering::Relaxed);
        state.triggerless_mode.store(true, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('g')), None);

        let result = eval
            .process(EngineEvent::Char('s'))
            .expect("Should expand instantly");
        assert_eq!(result.delete_count, 2);
        assert_eq!(result.trigger, "gs");
    }

    #[test]
    fn instant_expand_disabled_by_default() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::AutomationAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_eq!(eval.process(EngineEvent::Char('g')), None);
        assert_eq!(eval.process(EngineEvent::Char('s')), None);

        let result = eval
            .process(EngineEvent::ActionDelimiter)
            .expect("Should expand on delimiter");
        assert_eq!(result.delete_count, 3);
    }

    #[test]
    fn test_evaluator_regex_expansion_and_undo() {
        let state = Arc::new(EngineState::new('>'));
        state.load_regex_actions(vec![(
            "issue-(\\d+)".to_string(),
            crate::db::crud::AutomationAction::text("https://github.com/issues/[0]"),
        )]);
        let mut eval = Evaluator::new(state);

        // Simulate typing 'issue-42'
        for c in "issue-42".chars() {
            eval.process(EngineEvent::Char(c));
        }

        // Delimiter triggers evaluation
        let result = eval.process(EngineEvent::ActionDelimiter);
        assert!(result.is_some());
        let res = result.unwrap();
        assert_eq!(res.delete_count, 8); // 'issue-42' length
        assert_eq!(res.undo_trigger.as_deref(), Some("issue-42"));
    }
}
