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
    /// Number of characters to delete (the keyword).
    pub delete_count: usize,
    /// Ordered sequence of actions (text pastes, key presses, delays).
    pub steps: Vec<ExpansionStep>,
    /// The trigger keyword that was matched.
    pub trigger: String,
    /// Exact trigger text to restore during Backspace Undo.
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
    pub(crate) original_query: String,
    pub(crate) current_text: String,
    pub(crate) suggestions: Vec<String>,
    pub(crate) selected_index: Option<usize>,
    pub(crate) selection_mode: Option<TriggerAssistSelectionMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TriggerAssistSelectionMode {
    Completion,
}

impl TriggerCompletionState {
    pub(crate) fn activate(
        &mut self,
        active_atomic: &std::sync::atomic::AtomicBool,
        is_emoji: bool,
    ) {
        self.active = true;
        self.is_emoji = is_emoji;
        active_atomic.store(true, std::sync::atomic::Ordering::Relaxed);
        self.original_query.clear();
        self.current_text.clear();
        self.suggestions.clear();
        self.selected_index = None;
        self.selection_mode = None;
    }

    pub(crate) fn deactivate(&mut self, active_atomic: &std::sync::atomic::AtomicBool) {
        self.active = false;
        self.is_emoji = false;
        active_atomic.store(false, std::sync::atomic::Ordering::Relaxed);
        self.original_query.clear();
        self.current_text.clear();
        self.suggestions.clear();
        self.selected_index = None;
        self.selection_mode = None;
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selected_index = None;
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
