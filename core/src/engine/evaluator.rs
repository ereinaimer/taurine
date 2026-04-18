use std::sync::Arc;

use crate::engine::variables::ExpansionStep;

use crate::engine::buffer::FastBuffer;
use crate::engine::state::{EngineMode, EngineState};

const INLINE_AI_KEYWORD: &str = "ai";
const INLINE_AI_TRIGGER_DELETE_COUNT: usize = 4;
const INLINE_AI_THINKING_HEADER: &str = "\n\ntau:\n⠋ Thinking";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineEvent {
    Char(char),
    Backspace,
    WordBackspace,
    SubmitAiPrompt,
    Interrupt, // Esc, Mouse clicks, or loss of focus
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
    /// Whether this expansion was a mathematical calculation.
    pub is_calculation: bool,
    /// Whether the daemon should record snippet/calculation usage for this expansion.
    pub track_usage: bool,
    /// Whether the daemon should start the async AI spinner after injection.
    pub start_ai_spinner: bool,
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

    pub fn process_event(&mut self, event: EngineEvent) -> Option<ExpansionResult> {
        match self.state.engine_mode() {
            EngineMode::AiCapture => return self.process_ai_capture_event(event),
            EngineMode::AiGenerating => return self.process_ai_generating_event(event),
            EngineMode::Normal => {}
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
                use std::sync::atomic::Ordering;
                let trigger_char_u32 = self.state.trigger_char.load(Ordering::Relaxed);
                let trigger_char = std::char::from_u32(trigger_char_u32).unwrap_or('>');

                if let Some(keyword) = self.buffer.extract_trigger_word(trigger_char) {
                    if keyword == INLINE_AI_KEYWORD {
                        return Some(self.start_inline_ai_capture());
                    }

                    if let Some(expansion) = self.state.fetch_expansion(&keyword) {
                        // trigger_char + keyword + the space that fired the action
                        let delete_count = 1 + keyword.chars().count() + 1;
                        self.buffer.clear();
                        return Some(ExpansionResult {
                            delete_count,
                            steps: expansion.steps,
                            trigger: keyword,
                            is_calculation: expansion.is_calculation,
                            track_usage: true,
                            start_ai_spinner: false,
                        });
                    }
                }

                // Not a trigger — just record the space normally.
                self.buffer.push(' ');
                None
            }
            EngineEvent::SubmitAiPrompt => None,
            EngineEvent::Char(c) => {
                // Normal typing tracking
                self.buffer.push(c);
                None
            }
        }
    }

    fn process_ai_capture_event(&mut self, event: EngineEvent) -> Option<ExpansionResult> {
        // AI capture bypasses the trigger matcher entirely and should not retain
        // any stale snippet-matching history while the prompt is being typed.
        self.buffer.clear();

        match event {
            EngineEvent::Char(c) => self.state.append_ai_prompt_char(c),
            EngineEvent::Backspace => self.state.pop_ai_prompt_char(),
            EngineEvent::WordBackspace => self.state.pop_ai_prompt_word(),
            EngineEvent::SubmitAiPrompt => return Some(self.start_inline_ai_generation()),
            EngineEvent::Interrupt => {}
        }

        None
    }

    fn process_ai_generating_event(&mut self, event: EngineEvent) -> Option<ExpansionResult> {
        self.buffer.clear();

        match event {
            EngineEvent::Interrupt
            | EngineEvent::Backspace
            | EngineEvent::WordBackspace
            | EngineEvent::SubmitAiPrompt
            | EngineEvent::Char(_) => None,
        }
    }

    fn start_inline_ai_capture(&mut self) -> ExpansionResult {
        self.state.clear_ai_prompt_buffer();
        self.state.set_engine_mode(EngineMode::AiCapture);
        self.buffer.clear();

        ExpansionResult {
            delete_count: INLINE_AI_TRIGGER_DELETE_COUNT,
            steps: vec![ExpansionStep::Text(build_inline_ai_header())],
            trigger: INLINE_AI_KEYWORD.to_string(),
            is_calculation: false,
            track_usage: false,
            start_ai_spinner: false,
        }
    }

    fn start_inline_ai_generation(&mut self) -> ExpansionResult {
        self.buffer.clear();
        self.state.set_engine_mode(EngineMode::AiGenerating);

        ExpansionResult {
            delete_count: 0,
            steps: vec![ExpansionStep::Text(build_inline_ai_thinking_header())],
            trigger: INLINE_AI_KEYWORD.to_string(),
            is_calculation: false,
            track_usage: false,
            start_ai_spinner: true,
        }
    }
}

fn build_inline_ai_header() -> String {
    build_inline_ai_header_for_username(&resolve_inline_ai_username())
}

fn build_inline_ai_header_for_username(username: &str) -> String {
    format!("// alt+enter send\n// alt+esc exit\n\n{}:\n\n", username)
}

fn build_inline_ai_thinking_header() -> String {
    INLINE_AI_THINKING_HEADER.to_string()
}

fn resolve_inline_ai_username() -> String {
    resolve_inline_ai_username_from_values(
        std::env::var("USERNAME").ok(),
        std::env::var("USER").ok(),
    )
}

fn resolve_inline_ai_username_from_values(
    username: Option<String>,
    user: Option<String>,
) -> String {
    username
        .or(user)
        .unwrap_or_else(|| "user".to_string())
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::error;

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
        assert!(!result.start_ai_spinner);

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
        assert!(!result.is_calculation);
        assert!(result.track_usage);
        assert!(!result.start_ai_spinner);
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
        assert!(!result.start_ai_spinner);
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
        assert!(result.is_calculation);
        assert!(result.track_usage);
        assert!(!result.start_ai_spinner);
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
        assert!(!result.start_ai_spinner);
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
        assert!(!result.start_ai_spinner);
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
    fn ai_capture_mode_bypasses_trigger_matching_and_records_prompt_text() {
        let state = Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gm".to_string(),
            crate::db::crud::AutomationAction::text("Good morning!"),
        )]);
        state.set_engine_mode(EngineMode::AiCapture);
        let mut eval = Evaluator::new(state.clone());

        for c in ">gm hello ".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        assert_eq!(state.ai_prompt_buffer(), ">gm hello ");
        assert_eq!(eval.buffer.len, 0);
    }

    #[test]
    fn ai_capture_mode_tracks_backspace_without_using_fast_buffer_matching() {
        let state = Arc::new(EngineState::new('>'));
        state.set_engine_mode(EngineMode::AiCapture);
        let mut eval = Evaluator::new(state.clone());

        for c in "draft".chars() {
            eval.process_event(EngineEvent::Char(c));
        }
        eval.process_event(EngineEvent::Backspace);
        eval.process_event(EngineEvent::Char('!'));

        assert_eq!(state.ai_prompt_buffer(), "draf!");
        assert_eq!(eval.buffer.len, 0);
    }

    #[test]
    fn ai_capture_mode_tracks_word_backspace_and_ignores_interrupts() {
        let state = Arc::new(EngineState::new('>'));
        state.set_engine_mode(EngineMode::AiCapture);
        let mut eval = Evaluator::new(state.clone());

        for c in "hello world".chars() {
            eval.process_event(EngineEvent::Char(c));
        }
        eval.process_event(EngineEvent::WordBackspace);
        eval.process_event(EngineEvent::Interrupt);

        assert_eq!(state.ai_prompt_buffer(), "hello ");
        assert_eq!(eval.buffer.len, 0);
    }

    #[test]
    fn inline_ai_trigger_enters_capture_mode_and_uses_header_text_step() {
        let state = Arc::new(EngineState::new('/'));
        let mut eval = Evaluator::new(state.clone());

        for c in "/ai".chars() {
            assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
        }

        let result = eval
            .process_event(EngineEvent::Char(' '))
            .expect("inline ai should trigger on space");

        assert_eq!(state.engine_mode(), EngineMode::AiCapture);
        assert_eq!(state.ai_prompt_buffer(), "");
        assert_eq!(eval.buffer.len, 0);
        assert_eq!(result.delete_count, INLINE_AI_TRIGGER_DELETE_COUNT);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(build_inline_ai_header())]
        );
        assert_eq!(result.trigger, INLINE_AI_KEYWORD);
        assert!(!result.track_usage);
        assert!(!result.start_ai_spinner);
    }

    #[test]
    fn inline_ai_header_format_matches_spec() {
        assert_eq!(
            build_inline_ai_header_for_username("codex"),
            "// alt+enter send\n// alt+esc exit\n\ncodex:\n\n"
        );
    }

    #[test]
    fn inline_ai_submit_enters_generating_mode_and_preserves_prompt_buffer() {
        let state = Arc::new(EngineState::new('>'));
        state.set_engine_mode(EngineMode::AiCapture);
        state.append_ai_prompt_char('h');
        state.append_ai_prompt_char('i');
        let mut eval = Evaluator::new(state.clone());

        let result = eval
            .process_event(EngineEvent::SubmitAiPrompt)
            .expect("submit should start AI generation");

        assert_eq!(state.engine_mode(), EngineMode::AiGenerating);
        assert_eq!(state.ai_prompt_buffer(), "hi");
        assert_eq!(result.delete_count, 0);
        assert_eq!(
            result.steps,
            vec![ExpansionStep::Text(build_inline_ai_thinking_header())]
        );
        assert!(result.start_ai_spinner);
        assert!(!result.track_usage);
    }

    #[test]
    fn ai_generating_mode_ignores_further_input_into_prompt_buffer() {
        let state = Arc::new(EngineState::new('>'));
        state.set_engine_mode(EngineMode::AiGenerating);
        state.append_ai_prompt_char('x');
        let mut eval = Evaluator::new(state.clone());

        eval.process_event(EngineEvent::Char('y'));
        eval.process_event(EngineEvent::Backspace);
        eval.process_event(EngineEvent::WordBackspace);
        eval.process_event(EngineEvent::SubmitAiPrompt);

        assert_eq!(state.ai_prompt_buffer(), "x");
        assert_eq!(eval.buffer.len, 0);
    }

    #[test]
    fn inline_ai_thinking_header_matches_spec() {
        assert_eq!(build_inline_ai_thinking_header(), "\n\ntau:\n⠋ Thinking");
    }

    #[test]
    fn inline_ai_username_resolution_prefers_username_then_user_then_default() {
        assert_eq!(
            resolve_inline_ai_username_from_values(
                Some("DeskUser".to_string()),
                Some("ShellUser".to_string())
            ),
            "deskuser"
        );
        assert_eq!(
            resolve_inline_ai_username_from_values(None, Some("ShellUser".to_string())),
            "shelluser"
        );
        assert_eq!(resolve_inline_ai_username_from_values(None, None), "user");
    }
}
