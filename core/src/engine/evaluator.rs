use std::sync::Arc;

use crate::engine::variables::ExpansionStep;
use crate::engine::variables::system::clipboard::MAX_PAYLOAD_BYTES;

use crate::engine::buffer::FastBuffer;
use crate::engine::state::{EngineMode, EngineState};

const INLINE_AI_KEYWORD: &str = "ai";
const INLINE_AI_KEYWORD_PREFIX: &str = "ai:";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineEvent {
    Char(char),
    Backspace,
    WordBackspace,
    Interrupt, // Esc, Mouse clicks, or loss of focus
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExpansionFollowUp {
    InlineAi {
        prompt: String,
        system_prompt_override: Option<String>,
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
    /// Whether the daemon should record snippet/calculation usage for this expansion.
    pub track_usage: bool,
    /// Optional daemon-side follow-up that should run after expansion injection.
    pub follow_up: Option<ExpansionFollowUp>,
}

pub struct Evaluator {
    pub buffer: FastBuffer,
    pub state: Arc<EngineState>,
}

impl Evaluator {
    pub fn new(state: Arc<EngineState>) -> Self {
        Self {
            buffer: FastBuffer::new(),
            state,
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
            crate::settings::SpinnerStyle::Braille => "⠋ Thinking...".to_string(),
            crate::settings::SpinnerStyle::Arc => "◜ Thinking...".to_string(),
            crate::settings::SpinnerStyle::Classic => "| Thinking...".to_string(),
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
                ExpansionStep::KeyPress(_) | ExpansionStep::Delay(_) => return false,
                // Shell/script side effects are not reversible through text deletion alone.
                ExpansionStep::Script(_) | ExpansionStep::InlineRun(_) => return false,
            }
        }

        // Clipboard history can legally hold a full 64 KiB payload. Treat that ceiling as unsafe
        // for blind undo so Taurine never floods the OS with a huge backspace replay.
        text_bytes < MAX_PAYLOAD_BYTES
    }

    fn undo_trigger_for_steps(&self, keyword: &str, steps: &[ExpansionStep]) -> Option<String> {
        self.allows_blind_undo(steps)
            .then(|| self.full_trigger_text(keyword))
    }

    pub fn process_event(&mut self, event: EngineEvent) -> Option<ExpansionResult> {
        if let EngineMode::AiCapture { .. } = self.state.engine_mode() {
            return self.process_ai_capture_event(event);
        }

        match event {
            EngineEvent::Interrupt => {
                // Severe interrupts ruin active sequences
                self.buffer.clear();
                None
            }
            EngineEvent::Backspace => {
                // Backtrack buffer safely
                self.buffer.pop();
                None
            }
            EngineEvent::WordBackspace => {
                // Backtrack a whole word
                self.buffer.pop_word();
                None
            }
            EngineEvent::Char(' ') => {
                // Action character — evaluate trigger extraction
                let trigger_char = self.trigger_prefix();

                if let Some(keyword) = self.buffer.extract_trigger_word(trigger_char) {
                    if keyword == INLINE_AI_KEYWORD {
                        return Some(self.start_inline_ai_capture(&keyword, None));
                    }

                    if let Some(preset_name) = keyword.strip_prefix("ai.")
                        && let Some(prompt_override) = self.state.get_ai_preset(preset_name)
                    {
                        return Some(self.start_inline_ai_capture(&keyword, Some(prompt_override)));
                    }

                    if let Some(prompt) = parse_inline_ai_prompt(&keyword) {
                        return Some(self.expand_inline_ai_prompt(&keyword, prompt));
                    }

                    if let Some(expansion) = self.state.fetch_expansion(&keyword) {
                        // trigger_char + keyword + the space that fired the action
                        let delete_count = 1 + keyword.chars().count() + 1;
                        let undo_trigger = self.undo_trigger_for_steps(&keyword, &expansion.steps);
                        self.buffer.clear();
                        return Some(ExpansionResult {
                            delete_count,
                            steps: expansion.steps,
                            trigger: keyword,
                            undo_trigger,
                            is_calculation: expansion.is_calculation,
                            track_usage: true,
                            follow_up: None,
                        });
                    }
                }

                // Not a trigger — just record the space normally.
                self.buffer.push(' ');
                None
            }
            EngineEvent::Char(c) => {
                // Normal typing tracking
                self.buffer.push(c);
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

    fn start_inline_ai_capture(
        &mut self,
        keyword: &str,
        prompt_override: Option<String>,
    ) -> ExpansionResult {
        use std::sync::atomic::Ordering;
        let delimiter_u32 = self.state.inline_ai_delimiter.load(Ordering::Relaxed);
        let delimiter = std::char::from_u32(delimiter_u32).unwrap_or('`');

        self.buffer.clear();
        self.state.clear_ai_prompt_buffer();
        self.state.set_engine_mode(EngineMode::AiCapture {
            system_prompt_override: prompt_override.clone(),
        });

        ExpansionResult {
            delete_count: 1 + keyword.chars().count() + 1,
            steps: vec![ExpansionStep::Text(delimiter.to_string())],
            trigger: keyword.to_string(),
            undo_trigger: None,
            is_calculation: false,
            track_usage: false,
            follow_up: None,
        }
    }

    fn expand_inline_ai_prompt(&mut self, keyword: &str, prompt: String) -> ExpansionResult {
        self.buffer.clear();
        let delete_count = 1 + keyword.chars().count() + 1;

        ExpansionResult {
            delete_count,
            steps: vec![ExpansionStep::Text(self.get_thinking_text())],
            trigger: INLINE_AI_KEYWORD.to_string(),
            undo_trigger: None,
            is_calculation: false,
            track_usage: false,
            follow_up: Some(ExpansionFollowUp::InlineAi {
                prompt,
                system_prompt_override: None,
            }),
        }
    }

    fn finish_inline_ai_capture_if_ready(&mut self) -> Option<ExpansionResult> {
        use std::sync::atomic::Ordering;
        let delimiter_u32 = self.state.inline_ai_delimiter.load(Ordering::Relaxed);
        let delimiter = std::char::from_u32(delimiter_u32).unwrap_or('`');

        let captured = self.state.ai_prompt_buffer();
        if !captured.ends_with(delimiter) {
            return None;
        }

        let prompt = captured.strip_suffix(delimiter)?;
        if prompt.is_empty() {
            return None;
        }

        let delete_count = captured.chars().count() + 2;
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
            trigger: INLINE_AI_KEYWORD.to_string(),
            undo_trigger: None,
            is_calculation: false,
            track_usage: false,
            follow_up: Some(ExpansionFollowUp::InlineAi {
                prompt: prompt.to_string(),
                system_prompt_override,
            }),
        })
    }
}

fn parse_inline_ai_prompt(keyword: &str) -> Option<String> {
    let raw_prompt = keyword.strip_prefix(INLINE_AI_KEYWORD_PREFIX)?;
    parse_quoted_inline_ai_prompt(raw_prompt)
}

fn parse_quoted_inline_ai_prompt(raw_prompt: &str) -> Option<String> {
    let mut chars = raw_prompt.chars();
    let quote = chars.next()?;
    if !matches!(quote, '"' | '\'') || raw_prompt.chars().count() < 2 {
        return None;
    }

    if raw_prompt.chars().last()? != quote {
        return None;
    }

    let inner = &raw_prompt[quote.len_utf8()..raw_prompt.len() - quote.len_utf8()];
    let mut parsed = String::new();
    let mut escaping = false;

    for ch in inner.chars() {
        if escaping {
            let resolved = match ch {
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                _ => ch,
            };
            parsed.push(resolved);
            escaping = false;
            continue;
        }

        match ch {
            '\\' => escaping = true,
            current if current == quote => return None,
            other => parsed.push(other),
        }
    }

    if escaping {
        return None;
    }

    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn test_standard_typing_no_trigger() {
        let mut eval = setup();
        for c in "hello world".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }
        // Buffer should have successfully recorded the string
        assert_eq!(eval.buffer.len, 11);
    }

    #[test]
    fn test_successful_trigger_requires_space() {
        let mut eval = setup();
        // Type standard string leading to a trigger
        for c in "Hello /gm".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        // Exact sequence matching should occur when space fires
        let result = eval.process_event(EngineEvent::Char(' ')).unwrap();
        // delete_count = '/' (1) + "gm" (2) + ' ' (1) = 4
        assert_eq!(result.delete_count, 4);
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
            eval.process_event(EngineEvent::Char(c));
        }

        // An interrupt (e.g. mouse click) happens
        eval.process_event(EngineEvent::Interrupt);

        // The space no longer expands because the buffer was wiped
        assert_eq!(eval.process_event(EngineEvent::Char(' ')), None);
    }

    #[test]
    fn test_backspace_supports_typo_correction() {
        let mut eval = setup();
        // Type string with typo: /gn
        for c in "/gn".chars() {
            eval.process_event(EngineEvent::Char(c));
        }

        // Delete 'n'
        eval.process_event(EngineEvent::Backspace);

        // Retype 'm'
        eval.process_event(EngineEvent::Char('m'));

        // Fire expansion
        let result = eval.process_event(EngineEvent::Char(' ')).unwrap();
        assert_eq!(result.delete_count, 4);
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
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        let result = eval
            .process_event(EngineEvent::Char(' '))
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
            crate::db::crud::AutomationAction::text("before [run.bash(echo hi)] after"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">runme".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("run template should expand");
        assert_eq!(result.undo_trigger, None);
        assert!(
            result
                .steps
                .iter()
                .any(|step| matches!(step, ExpansionStep::InlineRun(_)))
        );
    }

    #[test]
    fn clipboard_payload_at_history_ceiling_skips_blind_undo_registration() {
        crate::engine::variables::system::clipboard::set_mock_clipboard(Some(
            "x".repeat(MAX_PAYLOAD_BYTES),
        ));

        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "clip".to_string(),
            crate::db::crud::AutomationAction::text("[clipboard]"),
        )]);
        let mut eval = Evaluator::new(state);

        for c in ">clip".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("clipboard template should expand");
        assert_eq!(result.undo_trigger, None);

        crate::engine::variables::system::clipboard::set_mock_clipboard(None);
    }

    #[test]
    fn test_longer_keyword_has_correct_delete_count() {
        let mut eval = setup();
        // "/shrug" = 1 trigger + 5 keyword + 1 space = 7
        for c in "/shrug".chars() {
            eval.process_event(EngineEvent::Char(c));
        }
        let result = eval.process_event(EngineEvent::Char(' ')).unwrap();
        assert_eq!(result.delete_count, 7);
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
            eval.process_event(EngineEvent::Char(c));
        }
        assert_eq!(eval.process_event(EngineEvent::Char(' ')), None);
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
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }
        // Ambiguous: two `>` in one span — do not expand with a partial delete.
        assert_eq!(eval.process_event(EngineEvent::Char(' ')), None);
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
                let r = eval.process_event(EngineEvent::Char(' ')).unwrap();
                assert_eq!(
                    r.steps,
                    vec![ExpansionStep::Text("Be right back!".to_string())]
                );
                assert_eq!(r.delete_count, 1 + "brb".len() + 1);
            } else {
                assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
            }
        }
        assert_eq!(eval.buffer.len, 0);

        for c in ">gm ".chars() {
            if c == ' ' {
                let r = eval.process_event(EngineEvent::Char(' ')).unwrap();
                assert_eq!(
                    r.steps,
                    vec![ExpansionStep::Text("Good morning!".to_string())]
                );
                assert_eq!(r.delete_count, 1 + "gm".len() + 1);
            } else {
                assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
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
                    let r = eval.process_event(EngineEvent::Char(' ')).unwrap();
                    assert_eq!(
                        r.steps,
                        vec![ExpansionStep::Text("Good morning!".to_string())]
                    );
                    assert_eq!(r.delete_count, 1 + 2 + 1);
                } else {
                    assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
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
                assert_eq!(eval.process_event(EngineEvent::Char(' ')), None);
            } else {
                assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
            }
        }
        assert!(eval.buffer.len > 0);

        eval.process_event(EngineEvent::Interrupt);
        for c in ">gm ".chars() {
            if c == ' ' {
                let r = eval.process_event(EngineEvent::Char(' ')).unwrap();
                assert_eq!(
                    r.steps,
                    vec![ExpansionStep::Text("Good morning!".to_string())]
                );
            } else {
                assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
            }
        }
    }

    #[test]
    fn test_end_to_end_dynamic_variable_expansion() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "repo".to_string(),
            crate::db::crud::AutomationAction::text("https://github.com/[0]/[1]"),
        )]);
        let mut eval = Evaluator::new(state);

        let input = r#"Hello >repo:"ereinaimer":"taurine" "#;
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process_event(EngineEvent::Char(c)) {
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
        assert_eq!(result.delete_count, 1 + result.trigger.len() + 1);
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
            if let Some(res) = eval.process_event(EngineEvent::Char(c)) {
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
            eval.process_event(EngineEvent::Char(c));
        }

        // Backspace blah (WordBackspace)
        eval.process_event(EngineEvent::WordBackspace);

        let input2 = "irrelevant";
        for c in input2.chars() {
            eval.process_event(EngineEvent::Char(c));
        }

        let result = eval.process_event(EngineEvent::Char(' '));
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
            if let Some(res) = eval.process_event(EngineEvent::Char(c)) {
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
            if let Some(res) = eval.process_event(EngineEvent::Char(c)) {
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
            if let Some(res) = eval.process_event(EngineEvent::Char(c)) {
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
                if let Some(res) = eval.process_event(EngineEvent::Char(c)) {
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
    fn inline_ai_quoted_trigger_expands_into_thinking_spinner_with_prompt_payload() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        let trigger = r#">ai:"What is the deadliest microbe?""#;
        for c in trigger.chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("inline ai should trigger on the trailing space");

        assert_eq!(eval.buffer.len, 0);
        assert_eq!(result.delete_count, trigger.chars().count() + 1);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(eval.get_thinking_text())]
        );
        assert_eq!(result.trigger, INLINE_AI_KEYWORD);
        assert_eq!(result.undo_trigger, None);
        assert!(!result.track_usage);
        assert_inline_ai_follow_up(&result, "What is the deadliest microbe?", None);
    }

    #[test]
    fn inline_ai_prompt_parser_decodes_json_escapes() {
        assert_eq!(
            parse_inline_ai_prompt(r#"ai:"Line one\n\"Rust\"""#),
            Some("Line one\n\"Rust\"".to_string())
        );
    }

    #[test]
    fn inline_ai_single_quoted_trigger_expands_and_extracts_prompt() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);

        let trigger = ">ai:'What is the deadliest microbe?'";
        for c in trigger.chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("single-quoted inline ai should trigger on the trailing space");

        assert_eq!(result.delete_count, trigger.chars().count() + 1);
        assert_inline_ai_follow_up(&result, "What is the deadliest microbe?", None);
    }

    #[test]
    fn inline_ai_requires_matching_quotes() {
        assert_eq!(
            parse_inline_ai_prompt("ai:'hello'"),
            Some("hello".to_string())
        );
        assert_eq!(parse_inline_ai_prompt(r#"ai:"unterminated"#), None);
        assert_eq!(parse_inline_ai_prompt(r#"ai:"prompt'"#), None);
        assert_eq!(parse_inline_ai_prompt("ai:'prompt\""), None);

        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state);
        let invalid_trigger = r#">ai:"hello'"#;
        for c in invalid_trigger.chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        assert_eq!(eval.process_event(EngineEvent::Char(' ')), None);
        assert_eq!(eval.buffer.len, invalid_trigger.chars().count() + 1);
    }

    #[test]
    fn inline_ai_single_quote_parser_supports_escaped_quotes() {
        assert_eq!(
            parse_inline_ai_prompt(r#"ai:'It\'s still stateless'"#),
            Some("It's still stateless".to_string())
        );
    }

    #[test]
    fn inline_ai_capture_trigger_enters_micro_state_and_paints_opening_delimiter() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        for c in ">ai".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("inline ai capture should start on >ai<space>");

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert_eq!(state.ai_prompt_buffer(), "");
        assert_eq!(result.delete_count, 4);
        assert_eq!(result.steps, vec![ExpansionStep::Text("`".to_string())]);
        assert_eq!(result.undo_trigger, None);
        assert_no_follow_up(&result);
    }

    #[test]
    fn inline_ai_capture_exits_on_backtick_then_space_and_hands_prompt_to_stream() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        for c in ">ai ".chars() {
            let _ = eval.process_event(EngineEvent::Char(c));
        }

        for c in "What is Rust?`".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("closing backtick plus space should submit captured prompt");

        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert_eq!(state.ai_prompt_buffer(), "");
        assert_eq!(result.delete_count, "What is Rust?`".chars().count() + 2);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(eval.get_thinking_text())]
        );
        assert_eq!(result.undo_trigger, None);
        assert_inline_ai_follow_up(&result, "What is Rust?", None);
    }

    #[test]
    fn test_ai_capture_interrupted_by_esc_reverts_to_normal() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::AutomationAction::text("Good morning!"),
        )]);
        let mut eval = Evaluator::new(state.clone());

        for c in ">ai ".chars() {
            let _ = eval.process_event(EngineEvent::Char(c));
        }
        for c in "draft".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        assert_eq!(eval.process_event(EngineEvent::Interrupt), None);
        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert!(state.is_ai_prompt_empty());

        for c in ">gm".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }
        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("normal trigger should work after interrupt exits capture");
        assert_eq!(
            result.steps,
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

        for c in ">ai ".chars() {
            let _ = eval.process_event(EngineEvent::Char(c));
        }

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert!(state.is_ai_prompt_empty());
        assert_eq!(eval.process_event(EngineEvent::Backspace), None);
        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert!(state.is_ai_prompt_empty());

        for c in ">gm".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }
        let result = eval
            .process_event(EngineEvent::Char(' '))
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

        for c in ">ai ".chars() {
            let _ = eval.process_event(EngineEvent::Char(c));
        }

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert!(state.is_ai_prompt_empty());
        assert_eq!(eval.process_event(EngineEvent::WordBackspace), None);
        assert_eq!(state.engine_mode(), EngineMode::Normal);

        for c in ">gm".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }
        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("normal trigger should work after empty-buffer word-backspace exits capture");
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text("Good morning!".to_string())]
        );
    }

    #[test]
    fn test_ai_capture_finish_with_backtick_and_space() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        for c in ">ai ".chars() {
            let _ = eval.process_event(EngineEvent::Char(c));
        }
        for c in "prompt`".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("closing backtick plus space should submit captured prompt");

        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert!(state.is_ai_prompt_empty());
        assert_eq!(result.delete_count, "prompt`".chars().count() + 2);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(eval.get_thinking_text())]
        );
        assert_inline_ai_follow_up(&result, "prompt", None);
    }

    #[test]
    fn inline_ai_capture_keeps_collecting_without_closing_backtick_space() {
        let state = Arc::new(EngineState::new('>'));
        let mut eval = Evaluator::new(state.clone());

        for c in ">ai ".chars() {
            let _ = eval.process_event(EngineEvent::Char(c));
        }

        for c in "draft prompt ".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert_eq!(state.ai_prompt_buffer(), "draft prompt ");
    }

    #[test]
    fn inline_ai_thinking_text_matches_spec() {
        let state = Arc::new(EngineState::new('>'));
        let eval = Evaluator::new(state);
        assert_eq!(eval.get_thinking_text(), "⠋ Thinking...");
    }

    #[test]
    fn inline_ai_capture_works_with_custom_delimiter() {
        use std::sync::atomic::Ordering;
        let state = Arc::new(EngineState::new('>'));
        state
            .inline_ai_delimiter
            .store('~' as u32, Ordering::Relaxed);
        let mut eval = Evaluator::new(state.clone());

        // 1. Enter capture
        eval.process_event(EngineEvent::Char('>'));
        eval.process_event(EngineEvent::Char('a'));
        eval.process_event(EngineEvent::Char('i'));
        let start_res = eval
            .process_event(EngineEvent::Char(' '))
            .expect("Should enter capture");

        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert_eq!(start_res.steps, vec![ExpansionStep::Text("~".to_string())]);

        // 2. Type prompt
        for c in "Hello AI~".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        // 3. Finish capture
        let finish_res = eval
            .process_event(EngineEvent::Char(' '))
            .expect("Should finish capture");
        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert_inline_ai_follow_up(&finish_res, "Hello AI", None);
    }

    #[test]
    fn test_ai_preset_trigger_enters_capture_mode_with_override() {
        let state = Arc::new(EngineState::new('>'));
        state.load_ai_presets(vec![("re".to_string(), "expert editor".to_string())]);
        let mut eval = Evaluator::new(state);

        let input = ">ai.re ";
        let mut result = None;
        for c in input.chars() {
            if let Some(res) = eval.process_event(EngineEvent::Char(c)) {
                result = Some(res);
            }
        }

        let res = result.expect("AI preset should trigger");
        assert_no_follow_up(&res);
        assert!(matches!(
            eval.state.engine_mode(),
            EngineMode::AiCapture {
                system_prompt_override: Some(_)
            }
        ));
    }

    #[test]
    fn test_finishing_ai_preset_capture_preserves_override() {
        let state = Arc::new(EngineState::new('>'));
        state.load_ai_presets(vec![("re".to_string(), "expert editor".to_string())]);
        let mut eval = Evaluator::new(state);

        // Start capture
        for c in ">ai.re ".chars() {
            eval.process_event(EngineEvent::Char(c));
        }

        // Type prompt + delimiter
        for c in "fix grammar`".chars() {
            eval.process_event(EngineEvent::Char(c));
        }

        // Finish capture with space
        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("Should finish prompt");
        assert_inline_ai_follow_up(&result, "fix grammar", Some("expert editor"));
    }
}
