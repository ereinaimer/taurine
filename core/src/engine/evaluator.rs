use std::sync::Arc;

use crate::engine::buffer::FastBuffer;
use crate::engine::state::EngineState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EngineEvent {
    Char(char),
    Backspace,
    WordBackspace,
    Interrupt, // Esc, Mouse clicks, or loss of focus
}

/// Instructions the daemon must execute to perform a text expansion.
///
/// The daemon's only job is to relay these instructions to the OS:
/// 1. Send `delete_count` backspaces to erase the trigger sequence.
/// 2. Type out the `output` string.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpansionResult {
    /// Number of characters to delete (the trigger char + keyword + the trailing space).
    pub delete_count: usize,
    /// The replacement text to type out.
    pub output: String,
    /// The trigger keyword that was matched.
    pub trigger: String,
    /// The number of left arrow presses to execute after pasting.
    pub left_arrow_count: usize,
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
                let trigger_char = self.state.trigger_char;
                if let Some(keyword) = self.buffer.extract_trigger_word(trigger_char)
                    && let Some(expansion) = self.state.fetch_expansion(&keyword)
                {
                    // trigger_char + keyword + the space that fired the action
                    let delete_count = 1 + keyword.len() + 1;
                    self.buffer.clear();
                    return Some(ExpansionResult {
                        delete_count,
                        output: expansion.text,
                        trigger: keyword,
                        left_arrow_count: expansion.left_arrow_count,
                    });
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Evaluator {
        let state = Arc::new(EngineState::new('/'));
        state.load_snippets(vec![
            ("gm".to_string(), "Good morning!".to_string()),
            ("shrug".to_string(), r#"¯\_(ツ)_/¯"#.to_string()),
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
        assert_eq!(result.output, "Good morning!");

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
        assert_eq!(result.output, "Good morning!");
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
        assert_eq!(result.output, r#"¯\_(ツ)_/¯"#);
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
        state.load_snippets(vec![
            ("brb".to_string(), "Be right back!".to_string()),
            ("gm".to_string(), "Good morning!".to_string()),
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
        state.load_snippets(vec![
            ("brb".to_string(), "Be right back!".to_string()),
            ("gm".to_string(), "Good morning!".to_string()),
        ]);
        let mut eval = Evaluator::new(state);

        for c in ">brb ".chars() {
            if c == ' ' {
                let r = eval.process_event(EngineEvent::Char(' ')).unwrap();
                assert_eq!(r.output, "Be right back!");
                assert_eq!(r.delete_count, 1 + "brb".len() + 1);
            } else {
                assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
            }
        }
        assert_eq!(eval.buffer.len, 0);

        for c in ">gm ".chars() {
            if c == ' ' {
                let r = eval.process_event(EngineEvent::Char(' ')).unwrap();
                assert_eq!(r.output, "Good morning!");
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
        state.load_snippets(vec![("gm".to_string(), "Good morning!".to_string())]);
        let mut eval = Evaluator::new(state);

        for _ in 0..2 {
            for c in ">gm ".chars() {
                if c == ' ' {
                    let r = eval.process_event(EngineEvent::Char(' ')).unwrap();
                    assert_eq!(r.output, "Good morning!");
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
        state.load_snippets(vec![("gm".to_string(), "Good morning!".to_string())]);
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
                assert_eq!(r.output, "Good morning!");
            } else {
                assert_eq!(eval.process_event(EngineEvent::Char(c)), None);
            }
        }
    }

    #[test]
    fn test_end_to_end_dynamic_variable_expansion() {
        let state = Arc::new(EngineState::new('>'));
        state.load_snippets(vec![(
            "repo".to_string(),
            "https://github.com/{0}/{1}".to_string(),
        )]);
        let mut eval = Evaluator::new(state);

        let input = r#"Hello >repo-"ereinaimer, taurine" "#;
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process_event(EngineEvent::Char(c)) {
                last_result = Some(res);
            }
        }

        let result = last_result.expect("Expansion should have triggered on the space");
        assert_eq!(result.output, "https://github.com/ereinaimer/taurine");
        assert_eq!(result.trigger, r#"repo-"ereinaimer, taurine""#);
        // trigger_char + keyword + space
        assert_eq!(result.delete_count, 1 + result.trigger.len() + 1);
    }

    #[test]
    fn test_end_to_end_dynamic_variable_named_args_and_defaults() {
        let state = Arc::new(EngineState::new('>'));
        state.load_snippets(vec![(
            "gh".to_string(),
            "https://github.com/{username}/{repo=taurine}".to_string(),
        )]);
        let mut eval = Evaluator::new(state);

        let input = r#">gh-"username=ereinaimer" "#;
        let mut last_result = None;

        for c in input.chars() {
            if let Some(res) = eval.process_event(EngineEvent::Char(c)) {
                last_result = Some(res);
            }
        }

        let result = last_result.expect("Expansion should have triggered");
        assert_eq!(result.output, "https://github.com/ereinaimer/taurine");
        assert_eq!(result.trigger, r#"gh-"username=ereinaimer""#);
    }
    #[test]
    fn test_backspace_with_args_bug() {
        let state = Arc::new(EngineState::new('>'));
        state.load_snippets(vec![(
            "gh".to_string(),
            "https://github.com/{username}/{repo=taurine}".to_string(),
        )]);
        let mut eval = Evaluator::new(state);

        let input = ">gh-blah";
        for c in input.chars() {
            eval.process_event(EngineEvent::Char(c));
        }

        // Backspace blah (WordBackspace)
        eval.process_event(EngineEvent::WordBackspace);

        let input2 = "randomguy,randomrepo";
        for c in input2.chars() {
            eval.process_event(EngineEvent::Char(c));
        }

        let result = eval.process_event(EngineEvent::Char(' '));
        let result = result.expect("Expansion should have triggered");
        assert_eq!(result.output, "https://github.com/randomguy/randomrepo");
    }
}
