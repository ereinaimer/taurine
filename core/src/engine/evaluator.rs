use std::sync::Arc;

use crate::engine::variables::ExpansionStep;
use crate::stats::TriggerStatKind;

use crate::engine::buffer::FastBuffer;
use crate::engine::state::{EngineMode, EngineState};

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    Char(char),
    Backspace,
    WordBackspace,
    ActionKey,
    Interrupt, // Esc, Mouse clicks, or loss of focus
    Paste(String),
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
    pub stat_kind: TriggerStatKind,
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
pub(crate) struct TriggerCompletionState {
    pub(crate) active: bool,
    pub(crate) is_emoji: bool,
    pub(crate) is_triggerless: bool,
    pub(crate) original_query: String,
    pub(crate) current_text: String,
    pub(crate) suggestions: Vec<String>,
    pub(crate) selected_index: Option<usize>,
    pub(crate) history_items: Vec<String>,
    pub(crate) history_index: Option<usize>,
    pub(crate) selection_mode: Option<TriggerAssistSelectionMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TriggerAssistSelectionMode {
    Completion,
    History,
}

impl TriggerCompletionState {
    pub(crate) fn activate(
        &mut self,
        active_atomic: &std::sync::atomic::AtomicBool,
        is_emoji: bool,
        is_triggerless: bool,
    ) {
        self.active = true;
        self.is_emoji = is_emoji;
        self.is_triggerless = is_triggerless;
        active_atomic.store(true, std::sync::atomic::Ordering::Relaxed);
        self.original_query.clear();
        self.current_text.clear();
        self.suggestions.clear();
        self.selected_index = None;
        self.history_items.clear();
        self.history_index = None;
        self.selection_mode = None;
    }

    pub(crate) fn deactivate(&mut self, active_atomic: &std::sync::atomic::AtomicBool) {
        self.active = false;
        self.is_emoji = false;
        self.is_triggerless = false;
        active_atomic.store(false, std::sync::atomic::Ordering::Relaxed);
        self.original_query.clear();
        self.current_text.clear();
        self.suggestions.clear();
        self.selected_index = None;
        self.history_items.clear();
        self.history_index = None;
        self.selection_mode = None;
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selected_index = None;
        self.history_index = None;
        self.selection_mode = None;
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.selection_mode.is_some()
    }
}

pub struct Evaluator {
    pub buffer: FastBuffer,
    pub state: Arc<EngineState>,
    pub(crate) completion: TriggerCompletionState,
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

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.completion.deactivate(&self.state.completion_active);
    }

    pub(crate) fn get_thinking_text(&self) -> String {
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

    pub(crate) fn get_initial_spinner_text(&self, template: &str) -> String {
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

    pub(crate) fn trigger_prefix(&self) -> char {
        use std::sync::atomic::Ordering;
        let trigger_char_u32 = self.state.trigger_char.load(Ordering::Relaxed);
        std::char::from_u32(trigger_char_u32).unwrap_or('>')
    }

    pub(crate) fn full_trigger_text(&self, keyword: &str) -> String {
        format!("{}{}", self.trigger_prefix(), keyword)
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
            EngineEvent::ActionKey => {
                let was_completion_active = self.completion.active;
                let result = self.evaluate_buffer_for_expansion(active_window);
                if was_completion_active {
                    self.completion.deactivate(&self.state.completion_active);
                }
                if result.is_none() && !self.completion.active {
                    let char_rep = match self.state.action_key() {
                        crate::settings::ActionKey::Space => ' ',
                        crate::settings::ActionKey::Enter => '\n',
                    };
                    self.buffer.push(char_rep);
                }
                result
            }
            EngineEvent::Paste(_) => None,
            EngineEvent::Char(c) => {
                // Normal typing tracking
                self.buffer.push(c);
                self.update_completion_after_char(c);

                let mode = self.state.get_inline_ai_trigger_mode();
                let open_delim = match mode {
                    crate::settings::InlineAiTriggerMode::Symmetric => {
                        self.state.get_inline_ai_trigger()
                    }
                    crate::settings::InlineAiTriggerMode::Asymmetric => {
                        self.state.get_inline_ai_trigger_open()
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::variables::system::clip::MAX_PAYLOAD_BYTES;

    #[test]
    fn test_evaluator_reset_clears_buffer_and_deactivates_completions() {
        let state = Arc::new(EngineState::new(';'));
        let mut eval = Evaluator::new(state.clone());
        eval.buffer.push('a');
        eval.completion
            .activate(&state.completion_active, false, false);

        eval.reset();

        assert_eq!(eval.buffer.buffer_string(), "");
        assert!(!eval.is_completion_active());
    }

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
                crate::db::crud::TriggerAction::text("Good morning!"),
            ),
            (
                "shrug".to_string(),
                crate::db::crud::TriggerAction::text(r#"¯\_(ツ)_/¯"#),
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
    fn test_inline_currency_to_words_expansion() {
        let state = Arc::new(EngineState::new('>'));
        state
            .inline_currency_to_words_enabled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crate::settings::set_cached_inline_currency_to_words_enabled(true);
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut eval = Evaluator::new(state);

        // Test 1: "$1,200"
        for c in "$1,200".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        let val = res.unwrap();
        assert!(val.is_calculation);
        assert_eq!(val.trigger, "$1,200");
        assert_eq!(
            val.steps[0],
            ExpansionStep::Text("One thousand two hundred dollars".to_string())
        );

        // Test 2: "USD 1,200"
        eval.buffer.clear();
        for c in "USD 1,200".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        let val = res.unwrap();
        assert!(val.is_calculation);
        assert_eq!(val.trigger, "USD 1,200");
        assert_eq!(
            val.steps[0],
            ExpansionStep::Text("One thousand two hundred dollars".to_string())
        );

        // Test 3: "-EUR 50.99"
        eval.buffer.clear();
        for c in "-EUR 50.99".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        let val = res.unwrap();
        assert!(val.is_calculation);
        assert_eq!(val.trigger, "-EUR 50.99");
        assert_eq!(
            val.steps[0],
            ExpansionStep::Text("Negative fifty euros and ninety-nine cents".to_string())
        );

        // Test 4: "INR 0"
        eval.buffer.clear();
        for c in "INR 0".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        let val = res.unwrap();
        assert!(val.is_calculation);
        assert_eq!(val.trigger, "INR 0");
        assert_eq!(val.steps[0], ExpansionStep::Text("Zero rupees".to_string()));
    }

    #[test]
    fn test_inline_currency_to_words_disabled_does_not_expand() {
        let state = Arc::new(EngineState::new('>'));
        state
            .inline_currency_to_words_enabled
            .store(false, std::sync::atomic::Ordering::Relaxed);
        crate::settings::set_cached_inline_currency_to_words_enabled(false);
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut eval = Evaluator::new(state);

        for c in "$1,200".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_none());
    }

    #[test]
    fn test_evaluator_natural_unit_conversion_triggerless() {
        use self::EvaluatorTestExt;
        use std::collections::HashMap;
        let state = Arc::new(EngineState::new('>'));
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut eval = Evaluator::new(state);

        let mut mock = HashMap::new();
        mock.insert("USD".to_string(), 1.0);
        mock.insert("EUR".to_string(), 0.915);
        crate::engine::conversion::MOCK_RATES.with(|m| *m.borrow_mut() = Some(mock));

        for c in "100 dollars to Euros".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey).unwrap();
        assert_eq!(
            result.steps[0],
            ExpansionStep::Text("91.5 Euros".to_string())
        );

        crate::engine::conversion::MOCK_RATES.with(|m| *m.borrow_mut() = None);
    }

    #[test]
    fn test_evaluator_natural_unit_conversion_prefix_triggered() {
        use self::EvaluatorTestExt;
        use std::collections::HashMap;
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        let mut mock = HashMap::new();
        mock.insert("USD".to_string(), 1.0);
        mock.insert("EUR".to_string(), 0.915);
        crate::engine::conversion::MOCK_RATES.with(|m| *m.borrow_mut() = Some(mock));

        for c in ">100 dollars to Euros".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey).unwrap();
        assert_eq!(
            result.steps[0],
            ExpansionStep::Text("91.5 Euros".to_string())
        );

        crate::engine::conversion::MOCK_RATES.with(|m| *m.borrow_mut() = None);
    }

    #[test]
    fn test_inline_datetime_expansion() {
        let state = Arc::new(EngineState::new('>'));
        state
            .inline_datetime_enabled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut eval = Evaluator::new(state);

        // Test 1: "next friday"
        for c in "next friday".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        let val = res.unwrap();
        assert!(val.is_calculation);
        assert_eq!(val.trigger, "next friday");

        eval.reset();

        // Test 2: "2 days from tomorrow"
        for c in "2 days from tomorrow".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res2 = eval.process(EngineEvent::ActionKey);
        assert!(res2.is_some());
        assert_eq!(res2.unwrap().trigger, "2 days from tomorrow");

        eval.reset();

        // Test 3: "2 days from now"
        for c in "2 days from now".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res3 = eval.process(EngineEvent::ActionKey);
        assert!(res3.is_some());
        assert_eq!(res3.unwrap().trigger, "2 days from now");

        eval.reset();

        // Test 4: "11 hours from now"
        for c in "11 hours from now".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res4 = eval.process(EngineEvent::ActionKey);
        assert!(res4.is_some());
        assert_eq!(res4.unwrap().trigger, "11 hours from now");

        eval.reset();

        // Test 5: "+13 hours"
        for c in "+13 hours".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res5 = eval.process(EngineEvent::ActionKey);
        assert!(res5.is_some());
        assert_eq!(res5.unwrap().trigger, "+13 hours");

        eval.reset();

        // Test 6: "now" (on its own in triggerless mode should NOT expand)
        for c in "now".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res6 = eval.process(EngineEvent::ActionKey);
        assert!(res6.is_none());

        eval.reset();

        // Test 7: ">now" (prefixed mode should expand!)
        for c in ">now".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res7 = eval.process(EngineEvent::ActionKey);
        assert!(res7.is_some());
        assert_eq!(res7.unwrap().trigger, "now");

        eval.reset();

        // Test 8: "15 mins from now"
        for c in "15 mins from now".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res8 = eval.process(EngineEvent::ActionKey);
        assert!(res8.is_some());
        assert_eq!(res8.unwrap().trigger, "15 mins from now");
    }

    #[test]
    fn test_inline_timezone_expansion() {
        let state = Arc::new(EngineState::new('>'));
        state
            .inline_datetime_enabled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut eval = Evaluator::new(state);

        // Test 1: "time in tokyo" current time query
        for c in "time in tokyo".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some(), "time in tokyo should expand");
        assert!(res.as_ref().unwrap().is_calculation);
        assert_eq!(res.as_ref().unwrap().trigger, "time in tokyo");
        let output = &res.unwrap().steps;
        assert!(!output.is_empty());

        eval.reset();

        // Test 2: "now in dubai" current time query
        for c in "now in dubai".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res2 = eval.process(EngineEvent::ActionKey);
        assert!(res2.is_some(), "now in dubai should expand");
        assert_eq!(res2.unwrap().trigger, "now in dubai");

        eval.reset();

        // Test 3: "10am pst to ist" conversion query
        for c in "10am pst to ist".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res3 = eval.process(EngineEvent::ActionKey);
        assert!(res3.is_some(), "10am pst to ist should expand");
        assert_eq!(res3.unwrap().trigger, "10am pst to ist");

        eval.reset();

        // Test 4: unknown city does NOT expand
        for c in "time in nonexistent1234".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res4 = eval.process(EngineEvent::ActionKey);
        assert!(res4.is_none(), "unknown city should not expand");

        eval.reset();

        // Test 5: disabled datetime = no timezone expansion
        crate::settings::set_cached_inline_datetime_enabled(false);
        let mut eval2 = Evaluator::new(Arc::new(EngineState::new('>')));
        for c in "time in tokyo".chars() {
            eval2.process(EngineEvent::Char(c));
        }
        let res5 = eval2.process(EngineEvent::ActionKey);
        assert!(res5.is_none(), "should not expand when datetime disabled");

        eval2.reset();

        // Test 6: "tokyo time" current time query
        for c in "tokyo time".chars() {
            eval2.process(EngineEvent::Char(c));
        }
        let res6 = eval2.process(EngineEvent::ActionKey);
        assert!(res6.is_none(), "should not expand when datetime disabled");
        // Restore enabled for other tests
        crate::settings::set_cached_inline_datetime_enabled(true);
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
                crate::db::crud::TriggerAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
            (
                "gco".to_string(),
                crate::db::crud::TriggerAction::text("git checkout"),
            ),
        ]);
        state.load_hotkey_actions(vec![(
            "ctrl+shift+g".to_string(),
            crate::db::crud::TriggerAction::text("hotkey"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
                crate::db::crud::TriggerAction::text("git status"),
            ),
            (
                "gpush".to_string(),
                crate::db::crud::TriggerAction::text("git push"),
            ),
            (
                "gco".to_string(),
                crate::db::crud::TriggerAction::text("git checkout"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.cycle_completion_next(), 1, "gco");
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
            Some("gco".to_string())
        );
        assert_eq!(eval.completion.current_text, "gco");
        assert_eq!(eval.completion.selected_index, Some(0));

        assert_completion_rewrite(eval.cycle_completion_next(), 3, "gpush");
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
            Some("gpush".to_string())
        );
        assert_eq!(eval.completion.selected_index, Some(1));

        assert_completion_rewrite(eval.cycle_completion_next(), 5, "gs");
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
            Some("gs".to_string())
        );
        assert_eq!(eval.completion.selected_index, Some(2));

        assert_completion_rewrite(eval.cycle_completion_next(), 2, "gco");
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
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
                crate::db::crud::TriggerAction::text("git status"),
            ),
            (
                "gpush".to_string(),
                crate::db::crud::TriggerAction::text("git push"),
            ),
            (
                "gco".to_string(),
                crate::db::crud::TriggerAction::text("git checkout"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">z".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        eval.cancel_completion();

        assert!(!eval.is_completion_active());
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
            Some("gs".to_string())
        );
    }

    #[test]
    fn completion_backspace_updates_query_and_exits_after_trigger_is_removed() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
            (
                "gpush".to_string(),
                crate::db::crud::TriggerAction::text("git push"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">gs".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
            Some(String::new())
        );

        assert_eq!(eval.process(EngineEvent::Backspace), None);
        assert!(!eval.is_completion_active());
        assert_eq!(eval.buffer.extract_trigger_word('>', false), None);
        assert_eq!(eval.cycle_completion_next(), None);
    }

    #[test]
    fn completion_space_after_rewrite_uses_existing_word_expansion_path() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "gco".to_string(),
                crate::db::crud::TriggerAction::text("git checkout"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.cycle_completion_next(), 1, "gco");
        let result = eval
            .process(EngineEvent::ActionKey)
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
                crate::db::crud::TriggerAction::text("team update"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
            (
                "uuid".to_string(),
                crate::db::crud::TriggerAction::text("1234"),
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
                crate::db::crud::TriggerAction::text("team update"),
            ),
            (
                "gpush".to_string(),
                crate::db::crud::TriggerAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
            (
                "uuid".to_string(),
                crate::db::crud::TriggerAction::text("1234"),
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
                    EngineEvent::ActionKey
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
                crate::db::crud::TriggerAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string(), "gpush".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
            Some(String::new())
        );
    }

    #[test]
    fn history_backspace_edits_original_query_not_selected_history_item() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gitstatus".to_string(),
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        state.load_word_trigger_history(vec!["gitstatus".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">git".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
            eval.buffer.extract_trigger_word('>', false),
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
                crate::db::crud::TriggerAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
            Some(String::new())
        );
    }

    #[test]
    fn history_space_after_selection_uses_existing_word_expansion_path() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        state.load_word_trigger_history(vec!["gs".to_string()]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_completion_rewrite(eval.navigate_history_older(), 0, "gs");

        let result = eval
            .process(EngineEvent::ActionKey)
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
                crate::db::crud::TriggerAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
            (
                "gaa".to_string(),
                crate::db::crud::TriggerAction::text("git add --all"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string(), "gpush".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
                crate::db::crud::TriggerAction::text("git add --all"),
            ),
            (
                "gpm".to_string(),
                crate::db::crud::TriggerAction::text("git push master"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string(), "gpm".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
                crate::db::crud::TriggerAction::text("git add --all"),
            ),
            (
                "gpm".to_string(),
                crate::db::crud::TriggerAction::text("git push master"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string(), "gpm".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
                crate::db::crud::TriggerAction::text("git add --all"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string()]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        assert_completion_rewrite(eval.navigate_history_older(), 1, "gs");
        assert_completion_rewrite(eval.cycle_completion_next(), 2, "gaa");

        let result = eval
            .process(EngineEvent::ActionKey)
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
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
            Some("g".to_string())
        );
    }

    #[test]
    fn inline_history_setting_disables_history_rewrites() {
        use std::sync::atomic::Ordering;

        let state = Arc::new(EngineState::new('>'));
        state.inline_history_enabled.store(false, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        state.load_word_trigger_history(vec!["gs".to_string()]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);

        assert_eq!(eval.navigate_history_older(), None);
        assert_eq!(eval.navigate_history_newer(), None);
        assert_eq!(eval.completion.current_text, "");
        assert_eq!(eval.completion.original_query, "");
        assert!(eval.completion.history_items.is_empty());
        assert_eq!(
            eval.buffer.extract_trigger_word('>', false),
            Some(String::new())
        );
    }

    #[test]
    fn tab_completion_still_works_when_history_is_disabled() {
        use std::sync::atomic::Ordering;

        let state = Arc::new(EngineState::new('>'));
        state.inline_history_enabled.store(false, Ordering::Relaxed);
        state.load_actions(vec![
            (
                "gco".to_string(),
                crate::db::crud::TriggerAction::text("git checkout"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">g".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
            crate::db::crud::TriggerAction::text("git status"),
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
            crate::db::crud::TriggerAction::text("git status"),
        )]);

        let mut eval = Evaluator::new(state.clone());
        for c in ">gs".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let expansion = eval
            .process(EngineEvent::ActionKey)
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
                    EngineEvent::ActionKey
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
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        // Exact sequence matching should occur when space fires
        let result = eval.process(EngineEvent::ActionKey).unwrap();
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
                EngineEvent::ActionKey
            } else {
                EngineEvent::Char(c)
            });
        }

        // An interrupt (e.g. mouse click) happens
        eval.process(EngineEvent::Interrupt);

        // The space no longer expands because the buffer was wiped
        assert_eq!(eval.process(EngineEvent::ActionKey), None);
    }

    #[test]
    fn test_backspace_supports_typo_correction() {
        let mut eval = setup();
        // Type string with typo: /gn
        for c in "/gn".chars() {
            eval.process(if c == ' ' {
                EngineEvent::ActionKey
            } else {
                EngineEvent::Char(c)
            });
        }

        // Delete 'n'
        eval.process(EngineEvent::Backspace);

        // Retype 'm'
        eval.process(EngineEvent::Char('m'));

        // Fire expansion
        let result = eval.process(EngineEvent::ActionKey).unwrap();
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
            crate::db::crud::TriggerAction::text("Best,\n[cursor]\nErin"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">sig".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionKey)
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
            crate::db::crud::TriggerAction::text("before [exec.bash(echo hi)] after"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">runme".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionKey)
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
            crate::db::crud::TriggerAction::text("[clip]"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">clip".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionKey)
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
                EngineEvent::ActionKey
            } else {
                EngineEvent::Char(c)
            });
        }
        let result = eval.process(EngineEvent::ActionKey).unwrap();
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
                EngineEvent::ActionKey
            } else {
                EngineEvent::Char(c)
            });
        }
        assert_eq!(eval.process(EngineEvent::ActionKey), None);
    }

    #[test]
    fn test_multiple_trigger_chars_rejects_ambiguous_sequence() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "brb".to_string(),
                crate::db::crud::TriggerAction::text("Be right back!"),
            ),
            (
                "gm".to_string(),
                crate::db::crud::TriggerAction::text("Good morning!"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">brb>gm".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }
        // Ambiguous: two `>` in one span — do not expand with a partial delete.
        assert_eq!(eval.process(EngineEvent::ActionKey), None);
    }

    /// Simulates two separate expansions in a row: first snippet finishes (buffer cleared), then
    /// user types the second trigger — must not merge or double-fire.
    #[test]
    fn test_back_to_back_separate_triggers_like_user_typing_brb_then_gm() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![
            (
                "brb".to_string(),
                crate::db::crud::TriggerAction::text("Be right back!"),
            ),
            (
                "gm".to_string(),
                crate::db::crud::TriggerAction::text("Good morning!"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">brb ".chars() {
            if c == ' ' {
                let r = eval.process(EngineEvent::ActionKey).unwrap();
                assert_eq!(
                    r.steps,
                    vec![ExpansionStep::Text("Be right back!".to_string())]
                );
                assert_eq!(r.delete_count, 1 + "brb".len());
            } else {
                assert_eq!(
                    eval.process(if c == ' ' {
                        EngineEvent::ActionKey
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
                let r = eval.process(EngineEvent::ActionKey).unwrap();
                assert_eq!(
                    r.steps,
                    vec![ExpansionStep::Text("Good morning!".to_string())]
                );
                assert_eq!(r.delete_count, 1 + "gm".len());
            } else {
                assert_eq!(
                    eval.process(if c == ' ' {
                        EngineEvent::ActionKey
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
            crate::db::crud::TriggerAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state);

        for _ in 0..2 {
            for c in ">gm ".chars() {
                if c == ' ' {
                    let r = eval.process(EngineEvent::ActionKey).unwrap();
                    assert_eq!(
                        r.steps,
                        vec![ExpansionStep::Text("Good morning!".to_string())]
                    );
                    assert_eq!(r.delete_count, 1 + 2);
                } else {
                    assert_eq!(
                        eval.process(if c == ' ' {
                            EngineEvent::ActionKey
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
            crate::db::crud::TriggerAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">nope ".chars() {
            if c == ' ' {
                assert_eq!(eval.process(EngineEvent::ActionKey), None);
            } else {
                assert_eq!(
                    eval.process(if c == ' ' {
                        EngineEvent::ActionKey
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
                let r = eval.process(EngineEvent::ActionKey).unwrap();
                assert_eq!(
                    r.steps,
                    vec![ExpansionStep::Text("Good morning!".to_string())]
                );
            } else {
                assert_eq!(
                    eval.process(if c == ' ' {
                        EngineEvent::ActionKey
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
            crate::db::crud::TriggerAction::text("https://github.com/[0=org]/[1=repo]"),
        )]);
        let mut eval = Evaluator::new(state);

        let input = r#"Hello >repo:"ereinaimer":"taurine" "#;
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionKey
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
            crate::db::crud::TriggerAction::text("https://github.com/[username]/[repo=taurine]"),
        )]);
        let mut eval = Evaluator::new(state);

        let input = r#">gh:"username=ereinaimer" "#;
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionKey
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
            crate::db::crud::TriggerAction::text("https://github.com/ereinaimer/taurine"),
        )]);
        let mut eval = Evaluator::new(state);

        let input = ">gh:blah";
        for c in input.chars() {
            eval.process(if c == ' ' {
                EngineEvent::ActionKey
            } else {
                EngineEvent::Char(c)
            });
        }

        // Backspace blah (WordBackspace)
        eval.process(EngineEvent::WordBackspace);

        let input2 = "irrelevant";
        for c in input2.chars() {
            eval.process(if c == ' ' {
                EngineEvent::ActionKey
            } else {
                EngineEvent::Char(c)
            });
        }

        let result = eval.process(EngineEvent::ActionKey);
        let result = result.expect("Expansion should have triggered");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(
                "https://github.com/ereinaimer/taurine".to_string()
            )]
        );
    }

    #[test]
    fn test_inline_conversion_with_commas() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        let input = "100,000c=f ";
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionKey
            } else {
                EngineEvent::Char(c)
            }) {
                last_result = Some(res);
            }
        }

        let result = last_result.expect("Conversion expansion should have triggered");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("180,032f".to_string())]
        );
        assert_eq!(result.trigger, "100,000c=f");
        assert_eq!(result.undo_trigger.as_deref(), Some("100,000c=f"));
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
                EngineEvent::ActionKey
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
                EngineEvent::ActionKey
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
                EngineEvent::ActionKey
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
                    EngineEvent::ActionKey
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
    fn test_inline_math_single_operand_ignored() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        // Single numbers should return None (not swallow action key)
        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_eq!(eval.process(EngineEvent::Char('5')), None);
        assert_eq!(eval.process(EngineEvent::ActionKey), None);

        eval.buffer.clear();
        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_eq!(eval.process(EngineEvent::Char('(')), None);
        assert_eq!(eval.process(EngineEvent::Char('5')), None);
        assert_eq!(eval.process(EngineEvent::Char(')')), None);
        assert_eq!(eval.process(EngineEvent::ActionKey), None);

        // Constants alone should return None (not swallow action key)
        eval.buffer.clear();
        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_eq!(eval.process(EngineEvent::Char('p')), None);
        assert_eq!(eval.process(EngineEvent::Char('i')), None);
        assert_eq!(eval.process(EngineEvent::ActionKey), None);

        // Operations with constants should expand
        eval.buffer.clear();
        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_eq!(eval.process(EngineEvent::Char('2')), None);
        assert_eq!(eval.process(EngineEvent::Char('p')), None);
        assert_eq!(eval.process(EngineEvent::Char('i')), None);
        assert!(eval.process(EngineEvent::ActionKey).is_some());
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
        state.set_action_key(crate::settings::ActionKey::Space);
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));

        for c in "What is Rust?<<".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionKey)
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
        state.set_action_key(crate::settings::ActionKey::Space);
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::TriggerAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));
        for c in "What is Rust?<<".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let ai_result = eval
            .process(EngineEvent::ActionKey)
            .expect("inline ai follow-up should dispatch on closing delimiter plus space");
        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert_inline_ai_follow_up(&ai_result, "What is Rust?", None);

        for c in ">gm".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let expansion = eval
            .process(EngineEvent::ActionKey)
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
            crate::db::crud::TriggerAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));
        for c in "draft".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let expansion = eval
            .process(EngineEvent::ActionKey)
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
            crate::db::crud::TriggerAction::text("Good morning!"),
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
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }
        let result = eval
            .process(EngineEvent::ActionKey)
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
            crate::db::crud::TriggerAction::text("Good morning!"),
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
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }
        let result = eval
            .process(EngineEvent::ActionKey)
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
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionKey)
            .expect("action key should submit captured asymmetric prompt");

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
        state.set_inline_ai_trigger_mode(crate::settings::InlineAiTriggerMode::Symmetric);
        state.set_inline_ai_trigger("^".to_string());
        let mut eval = Evaluator::new(state.clone());

        let _ = eval.process(EngineEvent::Char('^'));
        for c in "prompt^".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        let result = eval
            .process(EngineEvent::ActionKey)
            .expect("action key should submit captured symmetric prompt");

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
        state.set_action_key(crate::settings::ActionKey::Space);
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));

        for c in "draft prompt ".chars() {
            assert_eq!(
                eval.process(if c == ' ' {
                    EngineEvent::ActionKey
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
        state.set_action_key(crate::settings::ActionKey::Space);
        state.set_inline_ai_trigger_mode(crate::settings::InlineAiTriggerMode::Asymmetric);
        state.set_inline_ai_trigger_open("[[".to_string());
        state.set_inline_ai_trigger_close("]]".to_string());
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
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(c)
                }),
                None
            );
        }

        // 3. Finish capture
        let finish_res = eval
            .process(EngineEvent::ActionKey)
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
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "gs".chars() {
            eval.process(EngineEvent::Char(c));
        }

        let result = eval
            .process(EngineEvent::ActionKey)
            .expect("triggerless match");
        assert_eq!(result.delete_count, 2);
        assert_eq!(result.trigger, "gs");
        assert_eq!(result.undo_trigger, Some("gs".to_string()));
    }

    #[test]
    fn triggerless_mode_does_not_fire_when_disabled() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(false, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "gs".chars() {
            eval.process(EngineEvent::Char(c));
        }

        assert_eq!(eval.process(EngineEvent::ActionKey), None);
    }

    #[test]
    fn triggerless_mode_fires_with_punctuation_prefix() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(true, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "(gs".chars() {
            eval.process(EngineEvent::Char(c));
        }

        let result = eval
            .process(EngineEvent::ActionKey)
            .expect("should expand triggerless with punctuation prefix");
        assert_eq!(result.delete_count, 2);
        assert_eq!(result.trigger, "gs");
    }

    #[test]
    fn triggerless_mode_expands_middle_word_with_enter_action_key() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(true, Ordering::Relaxed);
        state.set_action_key(crate::settings::ActionKey::Enter);
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::TriggerAction::text("Good Morning"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "notegm".chars() {
            eval.process(EngineEvent::Char(c));
        }

        let result = eval
            .process(EngineEvent::ActionKey)
            .expect("should expand triggerless with Enter action key despite boundary prefix");
        assert_eq!(result.delete_count, 2);
        assert_eq!(result.trigger, "gm");
        assert_eq!(result.undo_trigger, Some("gm".to_string()));
    }

    #[test]
    fn triggerless_mode_does_not_expand_middle_word_with_space_action_key() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(true, Ordering::Relaxed);
        state.set_action_key(crate::settings::ActionKey::Space);
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::TriggerAction::text("Good Morning"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "notegm".chars() {
            eval.process(EngineEvent::Char(c));
        }

        assert_eq!(eval.process(EngineEvent::ActionKey), None);
    }

    #[test]
    fn triggerless_mode_with_instant_expand_enforces_boundary_even_for_enter_action_key() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(true, Ordering::Relaxed);
        state.instant_expand.store(true, Ordering::Relaxed);
        state.set_action_key(crate::settings::ActionKey::Enter);
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::TriggerAction::text("Good Morning"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "noteg".chars() {
            assert_eq!(eval.process(EngineEvent::Char(c)), None);
        }

        assert_eq!(eval.process(EngineEvent::Char('m')), None);
    }

    #[test]
    fn triggered_mode_still_works_when_triggerless_enabled() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state.triggerless_mode.store(true, Ordering::Relaxed);
        state.load_actions(vec![(
            "gs".to_string(),
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">gs".chars() {
            eval.process(EngineEvent::Char(c));
        }

        let result = eval
            .process(EngineEvent::ActionKey)
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
            crate::db::crud::TriggerAction::text("git status"),
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
            crate::db::crud::TriggerAction::text("git status"),
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
            crate::db::crud::TriggerAction::text("git status"),
        )]);
        let mut eval = Evaluator::new(state);

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        assert_eq!(eval.process(EngineEvent::Char('g')), None);
        assert_eq!(eval.process(EngineEvent::Char('s')), None);

        let result = eval
            .process(EngineEvent::ActionKey)
            .expect("Should expand on delimiter");
        assert_eq!(result.delete_count, 3);
    }

    #[test]
    fn test_evaluator_regex_expansion_and_undo() {
        let state = Arc::new(EngineState::new('>'));
        state.load_regex_actions(vec![(
            "issue-(\\d+)".to_string(),
            crate::db::crud::TriggerAction::text("https://github.com/issues/[0]"),
        )]);
        let mut eval = Evaluator::new(state);

        // Simulate typing 'issue-42'
        for c in "issue-42".chars() {
            eval.process(EngineEvent::Char(c));
        }

        // Delimiter triggers evaluation
        let result = eval.process(EngineEvent::ActionKey);
        assert!(result.is_some());
        let res = result.unwrap();
        assert_eq!(res.delete_count, 8); // 'issue-42' length
        assert_eq!(res.undo_trigger.as_deref(), Some("issue-42"));
    }

    #[test]
    fn test_no_regex_allocation_when_catalog_empty() {
        let state = Arc::new(EngineState::new('/'));
        state.load_actions(vec![(
            "hi".to_string(),
            crate::db::crud::TriggerAction::text("hello"),
        )]);
        let mut eval = Evaluator::new(state);
        for c in "/hi".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        assert!(result.is_some());
        let res = result.unwrap();
        assert_eq!(res.delete_count, 3); // '/' + "hi" = 3
        assert_eq!(res.steps, vec![ExpansionStep::Text("hello".to_string())]);
    }

    #[test]
    fn test_emoji_word_boundary_activation() {
        let state = Arc::new(EngineState::new('>'));
        crate::settings::set_cached_inline_emoji_enabled(true);
        crate::settings::set_cached_inline_emoji_trigger_char(':');
        let mut eval = Evaluator::new(state);

        // Typing colon at start of buffer should activate completion
        assert_eq!(eval.process(EngineEvent::Char(':')), None);
        assert!(eval.is_completion_active());
        assert!(eval.completion.is_emoji);

        // Reset
        eval.cancel_completion();
        eval.buffer.clear();

        // Typing colon after a non-whitespace char should NOT activate
        eval.buffer.push('a');
        assert_eq!(eval.process(EngineEvent::Char(':')), None);
        assert!(!eval.is_completion_active());
    }

    #[test]
    fn test_emoji_expansion_on_action_key() {
        let state = Arc::new(EngineState::new('>'));
        crate::settings::set_cached_inline_emoji_enabled(true);
        crate::settings::set_cached_inline_emoji_trigger_char(':');
        let mut eval = Evaluator::new(state);

        for c in ":rocket".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        let exp = res.unwrap();
        assert_eq!(exp.steps, vec![ExpansionStep::Text("🚀".to_string())]);
        assert_eq!(exp.delete_count, 7);
    }

    #[test]
    fn test_emoji_tab_completion_cycles_labels() {
        let state = Arc::new(EngineState::new('>'));
        crate::settings::set_cached_inline_emoji_enabled(true);
        crate::settings::set_cached_inline_emoji_trigger_char(':');
        let mut eval = Evaluator::new(state);

        for c in ":rocke".chars() {
            eval.process(EngineEvent::Char(c));
        }

        // Cycling next should replace with `:rocket` (the label, not the emoji)
        assert_completion_rewrite(eval.cycle_completion_next(), 6, ":rocket");
        assert_eq!(eval.completion.current_text, ":rocket");

        // Finally, pressing space/action key expands
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        assert_eq!(
            res.unwrap().steps,
            vec![ExpansionStep::Text("🚀".to_string())]
        );
    }

    #[test]
    fn test_snippet_precedence_when_triggers_match() {
        let state = Arc::new(EngineState::new(':'));
        crate::settings::set_cached_inline_emoji_enabled(true);
        crate::settings::set_cached_inline_emoji_trigger_char(':');
        state.load_actions(vec![(
            "rocket".to_string(),
            crate::db::crud::TriggerAction::text("snippet_won"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ":rocket".chars() {
            eval.process(EngineEvent::Char(c));
        }

        // ActionKey should expand snippet instead of emoji rocket 🚀
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        let exp = res.unwrap();
        assert_eq!(
            exp.steps,
            vec![ExpansionStep::Text("snippet_won".to_string())]
        );
    }

    #[test]
    fn test_emoji_underscore_matching_and_expansion() {
        let state = Arc::new(EngineState::new('>'));
        crate::settings::set_cached_inline_emoji_enabled(true);
        crate::settings::set_cached_inline_emoji_trigger_char(':');
        let mut eval = Evaluator::new(state);

        // Typing with underscore should allow completion
        for c in ":heart_ey".chars() {
            eval.process(EngineEvent::Char(c));
        }
        assert!(eval.is_completion_active());

        // Cycling should suggest the hyphenated label `heart-eyes`
        assert_completion_rewrite(eval.cycle_completion_next(), 9, ":heart-eyes");

        // Typing and expanding directly with underscore should also work
        eval.cancel_completion();
        eval.buffer.clear();
        for c in ":heart_eyes".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        assert_eq!(
            res.unwrap().steps,
            vec![ExpansionStep::Text("😍".to_string())]
        );
    }

    #[test]
    fn test_spaces_in_arguments_with_enter_delimiter() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state.load_actions(vec![(
            "hi".to_string(),
            crate::db::crud::TriggerAction::text("Hello [0=default], [1=msg]!"),
        )]);

        let mut eval = Evaluator::new(state);
        for c in ">hi:erein aimer:how was your day".chars() {
            eval.process(EngineEvent::Char(c));
        }

        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        let exp = res.unwrap();
        assert_eq!(
            exp.steps,
            vec![ExpansionStep::Text(
                "Hello erein aimer, how was your day!".to_string()
            )]
        );
    }

    #[test]
    fn test_spaces_in_arguments_with_space_delimiter_fails() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Space);
        state.load_actions(vec![(
            "hi".to_string(),
            crate::db::crud::TriggerAction::text("Hello [0=default], [1=msg]!"),
        )]);

        let mut eval = Evaluator::new(state);
        for c in ">hi:erein".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_some());
        assert_eq!(
            res.unwrap().steps,
            vec![ExpansionStep::Text("Hello erein, msg!".to_string())]
        );
    }

    #[test]
    fn test_inline_unit_conversion_simple() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        let input = "100c=f ";
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionKey
            } else {
                EngineEvent::Char(c)
            }) {
                last_result = Some(res);
            }
        }

        let result = last_result.expect("Unit conversion should have triggered");
        assert_eq!(result.steps, vec![ExpansionStep::Text("212f".to_string())]);
        assert_eq!(result.trigger, "100c=f");
        assert_eq!(result.undo_trigger.as_deref(), Some("100c=f"));
    }

    #[test]
    fn test_inline_unit_conversion_instant_expand_disabled() {
        let state = Arc::new(EngineState::new('>'));
        state
            .instant_expand
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut eval = Evaluator::new(state);

        let input = "100c=f ";
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process(if c == ' ' {
                EngineEvent::ActionKey
            } else {
                EngineEvent::Char(c)
            }) {
                last_result = Some(res);
            }
        }

        assert!(
            last_result.is_none(),
            "Unit conversion must be skipped when instant_expand is active"
        );
    }

    #[test]
    fn test_triggerless_completion() {
        let state = Arc::new(EngineState::new('>'));
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        state.load_actions(vec![
            (
                "gpush".to_string(),
                crate::db::crud::TriggerAction::text("git push"),
            ),
            (
                "gs".to_string(),
                crate::db::crud::TriggerAction::text("git status"),
            ),
            (
                "gco".to_string(),
                crate::db::crud::TriggerAction::text("git checkout"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        // Type "gp"
        eval.process(EngineEvent::Char('g'));
        eval.process(EngineEvent::Char('p'));

        assert!(!eval.is_completion_active());

        // Activate completion
        let rewrite = eval.activate_triggerless_completion();
        assert_completion_rewrite(rewrite, 2, "gpush");

        assert!(eval.is_completion_active());
        assert_eq!(eval.completion.original_query, "gp");
        assert_eq!(eval.completion.current_text, "gpush");

        // Backspace should revert back to original query (which gets updated to "g")
        let rewrite = eval.rewrite_backspace_query();
        assert_completion_rewrite(rewrite, 5, "g");
        assert_eq!(eval.completion.original_query, "g");
        assert_eq!(eval.completion.current_text, "g");
    }

    #[test]
    fn test_triggered_shortcode_expansion() {
        crate::settings::set_cached_inline_emoji_enabled(true);
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        for c in ":heart".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey).unwrap();
        assert_eq!(res.steps[0], ExpansionStep::Text("❤️".to_string()));
        assert_eq!(res.delete_count, 6);
    }

    #[test]
    fn test_triggered_no_nl_fallback() {
        crate::settings::set_cached_inline_emoji_enabled(true);
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        // ":love" has no exact shortcode -> no expansion, no NL fallback
        for c in ":love".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_none());
    }

    #[test]
    fn test_triggerless_emoji_requires_emoji_suffix() {
        let state = Arc::new(EngineState::new('>'));
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crate::settings::set_cached_inline_emoji_enabled(true);
        let mut eval = Evaluator::new(state);

        // "heart" without "emoji" suffix -> no expansion
        for c in "heart".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey);
        assert!(res.is_none());
    }

    #[test]
    fn test_triggerless_emoji_with_emoji_suffix() {
        let state = Arc::new(EngineState::new('>'));
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crate::settings::set_cached_inline_emoji_enabled(true);
        let mut eval = Evaluator::new(state);

        for c in "heart emoji".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey).unwrap();
        assert_eq!(res.steps[0], ExpansionStep::Text("❤️".to_string()));
        assert_eq!(res.delete_count, 11);
    }

    #[test]
    fn test_triggerless_multi_word_with_emoji_suffix() {
        let state = Arc::new(EngineState::new('>'));
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crate::settings::set_cached_inline_emoji_enabled(true);
        let mut eval = Evaluator::new(state);

        for c in "happy face emoji".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey).unwrap();
        assert_eq!(res.steps[0], ExpansionStep::Text("😊".to_string()));
        assert_eq!(res.delete_count, 16);
    }

    #[test]
    fn test_triggerless_suffix_with_emoji_suffix() {
        let state = Arc::new(EngineState::new('>'));
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crate::settings::set_cached_inline_emoji_enabled(true);
        let mut eval = Evaluator::new(state);

        for c in "I love my cat emoji".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey).unwrap();
        assert_eq!(res.steps[0], ExpansionStep::Text("🐱".to_string()));
    }

    #[test]
    fn test_triggerless_emoji_with_punctuation() {
        let state = Arc::new(EngineState::new('>'));
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        crate::settings::set_cached_inline_emoji_enabled(true);
        let mut eval = Evaluator::new(state);

        for c in "heart emoji!".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let res = eval.process(EngineEvent::ActionKey).unwrap();
        assert_eq!(res.steps[0], ExpansionStep::Text("❤️".to_string()));
    }

    #[test]
    fn test_completion_suggestions_labels_only() {
        crate::settings::set_cached_inline_emoji_enabled(true);
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        eval.process(EngineEvent::Char(':'));
        assert!(eval.is_completion_active());

        let suggestions = eval.completion.suggestions.clone();
        for s in &suggestions {
            assert!(s.is_ascii(), "Suggestion must be a label: {}", s);
        }
    }

    #[test]
    fn test_completion_suggestions_contains_shortcodes() {
        crate::settings::set_cached_inline_emoji_enabled(true);
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        eval.process(EngineEvent::Char(':'));
        eval.process(EngineEvent::Char('f'));

        let suggestions = eval.completion.suggestions.clone();
        assert!(suggestions.contains(&":frog".to_string()));
    }

    #[test]
    fn test_multi_word_trigger_with_dot_expands_on_enter() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state.load_actions(vec![(
            "test.my email".to_string(),
            crate::db::crud::TriggerAction::text("erein"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">test.my email".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        assert!(
            result.is_some(),
            "multi-word trigger with dot should expand on Enter"
        );
        let val = result.unwrap();
        assert_eq!(val.trigger, "test.my email");
        assert_eq!(val.steps[0], ExpansionStep::Text("erein".to_string()));
    }

    #[test]
    fn test_space_at_various_positions_in_trigger() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state.load_actions(vec![
            (
                "my email".to_string(),
                crate::db::crud::TriggerAction::text("addr@x.com"),
            ),
            (
                "a b c".to_string(),
                crate::db::crud::TriggerAction::text("three-word"),
            ),
            (
                "  leading".to_string(),
                crate::db::crud::TriggerAction::text("spaces"),
            ),
        ]);
        let mut eval = Evaluator::new(state);

        // Standard two-word
        for c in ">my email".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        assert!(result.is_some(), "two-word trigger should expand");
        let val = result.unwrap();
        assert_eq!(val.trigger, "my email");
        assert_eq!(val.steps[0], ExpansionStep::Text("addr@x.com".to_string()));

        // Three-word trigger
        eval.buffer.clear();
        for c in ">a b c".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        assert!(result.is_some(), "three-word trigger should expand");
        assert_eq!(result.unwrap().trigger, "a b c");
    }

    #[test]
    fn test_trigger_expansion_with_leading_text_and_multi_word() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state.load_actions(vec![(
            "my email".to_string(),
            crate::db::crud::TriggerAction::text("addr@x.com"),
        )]);
        let mut eval = Evaluator::new(state);

        // Simulate typing normal text before the trigger
        for c in "hey there >my email".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        assert!(
            result.is_some(),
            "multi-word trigger with leading text should expand"
        );
        assert_eq!(result.unwrap().trigger, "my email");
    }

    #[test]
    fn test_multi_word_trigger_expands_on_enter() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state.load_actions(vec![(
            "my email address".to_string(),
            crate::db::crud::TriggerAction::text("user@example.com"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">my email address".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(val.trigger, "my email address");
        assert_eq!(
            val.steps[0],
            ExpansionStep::Text("user@example.com".to_string())
        );
    }

    #[test]
    fn test_multi_word_trigger_does_not_expand_on_space_when_action_key_is_space() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Space);
        state.load_actions(vec![(
            "my email address".to_string(),
            crate::db::crud::TriggerAction::text("user@example.com"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">my email address".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        assert!(result.is_none());
    }

    #[test]
    fn test_completion_stays_active_with_spaces_on_backspace_when_action_key_is_enter() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        let mut eval = Evaluator::new(state);

        // Activate completion by typing the trigger char
        eval.process(EngineEvent::Char('>'));
        assert!(eval.completion.active);

        // Type a multi-word with spaces
        for c in "my email ".chars() {
            eval.process(EngineEvent::Char(c));
        }
        assert!(
            eval.completion.active,
            "Completion should stay active with spaces when ActionKey=Enter"
        );

        // Simulate backspace - this calls sync_completion_from_buffer
        eval.process(EngineEvent::Backspace);
        assert!(
            eval.completion.active,
            "Completion should stay active after backspace with ActionKey=Enter"
        );
    }

    #[test]
    fn test_multi_word_trigger_preserves_completion_after_each_char() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state.load_actions(vec![(
            "test.my email".to_string(),
            crate::db::crud::TriggerAction::text("erein"),
        )]);
        let mut eval = Evaluator::new(state);

        eval.process(EngineEvent::Char('>'));
        assert!(eval.completion.active, "completion should activate on >");

        for c in "test.my email".chars() {
            eval.process(EngineEvent::Char(c));
            assert!(
                eval.completion.active,
                "completion should stay active after '{}'",
                c
            );
        }

        assert!(
            eval.completion.active,
            "completion should be active after full trigger"
        );
        assert_eq!(eval.completion.original_query, "test.my email");

        let result = eval.process(EngineEvent::ActionKey);
        assert!(
            result.is_some(),
            "multi-word trigger should expand on Enter"
        );
        let val = result.unwrap();
        assert_eq!(val.trigger, "test.my email");
        assert_eq!(val.steps[0], ExpansionStep::Text("erein".to_string()));
    }

    #[test]
    fn test_multi_word_trigger_preserves_completion_after_space_explicitly() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state.load_actions(vec![(
            "my email".to_string(),
            crate::db::crud::TriggerAction::text("addr@x.com"),
        )]);
        let mut eval = Evaluator::new(state);

        eval.process(EngineEvent::Char('>'));
        assert!(eval.completion.active, "completion should activate on >");

        for c in "my".chars() {
            eval.process(EngineEvent::Char(c));
        }
        assert!(eval.completion.active, "completion active before space");

        // The space character
        eval.process(EngineEvent::Char(' '));
        assert!(
            eval.completion.active,
            "completion should NOT deactivate after space"
        );
        assert_eq!(
            eval.completion.original_query, "my ",
            "query should include space"
        );

        for c in "email".chars() {
            eval.process(EngineEvent::Char(c));
        }
        assert!(
            eval.completion.active,
            "completion active after full trigger"
        );
        assert_eq!(eval.completion.original_query, "my email");

        let result = eval.process(EngineEvent::ActionKey);
        assert!(
            result.is_some(),
            "multi-word trigger should expand on Enter"
        );
    }

    #[test]
    fn test_triggerless_multi_word_expands_on_enter() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        state.load_actions(vec![(
            "my email".to_string(),
            crate::db::crud::TriggerAction::text("addr@x.com"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "my email".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        assert!(
            result.is_some(),
            "triggerless multi-word trigger should expand on Enter"
        );
        let val = result.unwrap();
        assert_eq!(val.trigger, "my email");
    }

    #[test]
    fn test_triggerless_multi_word_with_dot_expands_on_enter() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        state.load_actions(vec![(
            "test.my email".to_string(),
            crate::db::crud::TriggerAction::text("erein"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "test.my email".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        assert!(
            result.is_some(),
            "triggerless multi-word trigger with dot should expand on Enter"
        );
        let val = result.unwrap();
        assert_eq!(val.trigger, "test.my email");
        assert_eq!(val.steps[0], ExpansionStep::Text("erein".to_string()));
    }

    #[test]
    fn test_triggerless_multi_word_tab_does_not_cross_line() {
        let state = Arc::new(EngineState::new('>'));
        state.set_action_key(crate::settings::ActionKey::Enter);
        state
            .triggerless_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        state.load_actions(vec![(
            "my\temail".to_string(),
            crate::db::crud::TriggerAction::text("nope"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in "my\temail".chars() {
            eval.process(EngineEvent::Char(c));
        }
        let result = eval.process(EngineEvent::ActionKey);
        // Tab is not a space — multi-word should NOT cross tab boundaries
        assert!(result.is_none(), "should not expand across tab");
    }

    #[test]
    fn test_inline_ai_paste_appends_characters() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));
        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));

        assert_eq!(eval.process(EngineEvent::Paste("hello".to_string())), None);

        assert_eq!(state.ai_prompt_buffer(), "hello");
    }

    #[test]
    fn test_inline_ai_paste_does_not_auto_submit() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));
        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));

        assert_eq!(
            eval.process(EngineEvent::Paste("hello<<".to_string())),
            None
        );

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert_eq!(state.ai_prompt_buffer(), "hello<<");
    }

    #[test]
    fn test_inline_ai_paste_outside_ai_capture_is_nop() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(state.engine_mode(), EngineMode::Normal);

        assert_eq!(eval.process(EngineEvent::Paste("hello".to_string())), None);

        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert_eq!(state.ai_prompt_buffer(), "");
    }

    #[test]
    fn test_inline_ai_paste_respects_cap() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        assert_eq!(eval.process(EngineEvent::Char('>')), None);
        let _ = eval.process(EngineEvent::Char('>'));
        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));

        let large = "a".repeat(100 * 1024);
        assert_eq!(eval.process(EngineEvent::Paste(large)), None);

        let buf = state.ai_prompt_buffer();
        assert_eq!(buf.len(), 64 * 1024);
    }
}
