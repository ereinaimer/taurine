use super::evaluator::ExpansionFollowUp;
use crate::engine::EngineMode;
use crate::engine::evaluator::EngineEvent;
use crate::engine::evaluator::ExpansionResult;
use crate::engine::variables::ExpansionStep;
use crate::stats::TriggerStatKind;

impl crate::engine::evaluator::Evaluator {
    pub(crate) fn process_ai_capture_event(
        &mut self,
        event: EngineEvent,
    ) -> Option<ExpansionResult> {
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
            EngineEvent::ActionKey => {
                if let Some(expansion) = self.finish_inline_ai_capture_if_ready() {
                    return Some(expansion);
                }
                self.state.append_ai_prompt_char('\n');
                None
            }
            EngineEvent::Paste(text) => {
                for c in text.chars() {
                    self.state.append_ai_prompt_char(c);
                }
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

    pub(crate) fn start_inline_ai_capture(&mut self, open_delim: &str) -> ExpansionResult {
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
            stat_kind: TriggerStatKind::InlineAi,
            track_usage: false,
            follow_up: None,
        }
    }

    pub(crate) fn finish_inline_ai_capture_if_ready(&mut self) -> Option<ExpansionResult> {
        let mode = self.state.get_inline_ai_trigger_mode();
        let close_delim = match mode {
            crate::settings::InlineAiTriggerMode::Symmetric => self.state.get_inline_ai_trigger(),
            crate::settings::InlineAiTriggerMode::Asymmetric => {
                self.state.get_inline_ai_trigger_close()
            }
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
            crate::settings::InlineAiTriggerMode::Symmetric => self.state.get_inline_ai_trigger(),
            crate::settings::InlineAiTriggerMode::Asymmetric => {
                self.state.get_inline_ai_trigger_open()
            }
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
            stat_kind: TriggerStatKind::InlineAi,
            track_usage: false,
            follow_up: Some(ExpansionFollowUp::InlineAi {
                prompt: prompt.to_string(),
                system_prompt_override,
            }),
        })
    }
}
