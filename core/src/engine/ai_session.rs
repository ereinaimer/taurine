use std::sync::{Mutex, RwLock};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EngineMode {
    #[default]
    Normal,
    AiCapture {
        system_prompt_override: Option<String>,
    },
}

pub struct InlineAiSession {
    mode: RwLock<EngineMode>,
    prompt_buffer: Mutex<String>,
}

impl InlineAiSession {
    pub fn new() -> Self {
        Self {
            mode: RwLock::new(EngineMode::Normal),
            prompt_buffer: Mutex::new(String::new()),
        }
    }

    pub fn engine_mode(&self) -> EngineMode {
        self.mode
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn set_engine_mode(&self, mode: EngineMode) {
        if let Ok(mut guard) = self.mode.write() {
            *guard = mode;
        }
    }

    pub fn append_prompt_char(&self, c: char) {
        if let Ok(mut prompt) = self.prompt_buffer.lock() {
            prompt.push(c);
        }
    }

    pub fn pop_prompt_char(&self) {
        if let Ok(mut prompt) = self.prompt_buffer.lock() {
            prompt.pop();
        }
    }

    pub fn pop_prompt_word(&self) {
        if let Ok(mut prompt) = self.prompt_buffer.lock() {
            pop_last_word_from_prompt(&mut prompt);
        }
    }

    pub fn clear_prompt_buffer(&self) {
        if let Ok(mut prompt) = self.prompt_buffer.lock() {
            prompt.clear();
        }
    }

    pub fn prompt_buffer(&self) -> String {
        self.prompt_buffer
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn is_prompt_empty(&self) -> bool {
        self.prompt_buffer
            .lock()
            .map(|guard| guard.is_empty())
            .unwrap_or(true)
    }
}

impl Default for InlineAiSession {
    fn default() -> Self {
        Self::new()
    }
}

fn pop_last_word_from_prompt(prompt: &mut String) {
    if prompt.is_empty() {
        return;
    }

    let mut chars: Vec<char> = prompt.chars().collect();

    while let Some(last) = chars.last() {
        if last.is_whitespace() {
            chars.pop();
        } else {
            break;
        }
    }

    let Some(last) = chars.last().copied() else {
        prompt.clear();
        return;
    };

    let is_alphanumeric = last.is_alphanumeric();
    while let Some(current) = chars.last().copied() {
        if current.is_whitespace() || current.is_alphanumeric() != is_alphanumeric {
            break;
        }
        chars.pop();
    }

    *prompt = chars.into_iter().collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_ai_session_defaults_to_normal_mode_with_empty_prompt() {
        let session = InlineAiSession::new();

        assert_eq!(session.engine_mode(), EngineMode::Normal);
        assert_eq!(session.prompt_buffer(), "");
        assert!(session.is_prompt_empty());
    }

    #[test]
    fn inline_ai_session_append_and_pop_behave_like_previous_engine_state_helpers() {
        let session = InlineAiSession::new();

        session.append_prompt_char('h');
        session.append_prompt_char('i');
        session.append_prompt_char(' ');
        session.append_prompt_char('世');
        session.append_prompt_char('界');
        assert_eq!(session.prompt_buffer(), "hi 世界");

        session.pop_prompt_char();
        assert_eq!(session.prompt_buffer(), "hi 世");
    }

    #[test]
    fn inline_ai_session_pop_last_word_matches_previous_behavior() {
        let session = InlineAiSession::new();

        for c in "hi 世界".chars() {
            session.append_prompt_char(c);
        }

        session.pop_prompt_word();
        assert_eq!(session.prompt_buffer(), "hi ");
    }

    #[test]
    fn inline_ai_session_clear_resets_buffer_without_touching_mode() {
        let session = InlineAiSession::new();

        session.set_engine_mode(EngineMode::AiCapture {
            system_prompt_override: Some("expert editor".to_string()),
        });
        for c in "draft".chars() {
            session.append_prompt_char(c);
        }

        session.clear_prompt_buffer();

        assert_eq!(session.prompt_buffer(), "");
        assert!(session.is_prompt_empty());
        assert_eq!(
            session.engine_mode(),
            EngineMode::AiCapture {
                system_prompt_override: Some("expert editor".to_string())
            }
        );
    }
}
