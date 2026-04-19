use crate::db::crud::AutomationAction;
use crate::engine::catalog::ExpansionCatalog;
use crate::engine::source::SnippetSource;
use crate::engine::variables::FinalExpansion;

use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::{Mutex, RwLock};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EngineMode {
    #[default]
    Normal,
    AiCapture {
        system_prompt_override: Option<String>,
    },
}

pub struct EngineState {
    pub trigger_char: AtomicU32,
    pub inline_ai_delimiter: AtomicU32,
    pub mode: RwLock<EngineMode>,
    pub ai_prompt_buffer: Mutex<String>,
    pub ai_presets: RwLock<std::collections::HashMap<String, String>>,
    catalog: ExpansionCatalog,
}

impl EngineState {
    pub fn new(trigger_char: char) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            inline_ai_delimiter: AtomicU32::new('`' as u32),
            mode: RwLock::new(EngineMode::Normal),
            ai_prompt_buffer: Mutex::new(String::new()),
            ai_presets: RwLock::new(std::collections::HashMap::new()),
            catalog: ExpansionCatalog::new(),
        }
    }

    /// Creates an EngineState with a custom snippet source.
    pub fn with_source(trigger_char: char, source: Arc<dyn SnippetSource>) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            inline_ai_delimiter: AtomicU32::new('`' as u32),
            mode: RwLock::new(EngineMode::Normal),
            ai_prompt_buffer: Mutex::new(String::new()),
            ai_presets: RwLock::new(std::collections::HashMap::new()),
            catalog: ExpansionCatalog::with_source(source),
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

    pub fn append_ai_prompt_char(&self, c: char) {
        if let Ok(mut prompt) = self.ai_prompt_buffer.lock() {
            prompt.push(c);
        }
    }

    pub fn pop_ai_prompt_char(&self) {
        if let Ok(mut prompt) = self.ai_prompt_buffer.lock() {
            prompt.pop();
        }
    }

    pub fn pop_ai_prompt_word(&self) {
        if let Ok(mut prompt) = self.ai_prompt_buffer.lock() {
            pop_last_word_from_prompt(&mut prompt);
        }
    }

    pub fn clear_ai_prompt_buffer(&self) {
        if let Ok(mut prompt) = self.ai_prompt_buffer.lock() {
            prompt.clear();
        }
    }

    pub fn ai_prompt_buffer(&self) -> String {
        self.ai_prompt_buffer
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn is_ai_prompt_empty(&self) -> bool {
        self.ai_prompt_buffer
            .lock()
            .map(|guard| guard.is_empty())
            .unwrap_or(true)
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, AutomationAction)>) {
        self.catalog.load_actions(actions);
    }

    pub fn load_ai_presets(&self, presets: impl IntoIterator<Item = (String, String)>) {
        if let Ok(mut guard) = self.ai_presets.write() {
            *guard = presets.into_iter().collect();
        }
    }

    pub fn get_ai_preset(&self, name: &str) -> Option<String> {
        self.ai_presets
            .read()
            .ok()
            .and_then(|guard| guard.get(name).cloned())
    }

    pub fn fetch_expansion(&self, keyword: &str) -> Option<FinalExpansion> {
        self.catalog.fetch_expansion(keyword)
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
    fn engine_state_defaults_to_normal_mode_with_empty_ai_prompt() {
        let state = EngineState::new('>');

        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert_eq!(state.ai_prompt_buffer(), "");
    }

    #[test]
    fn engine_state_ai_prompt_helpers_track_chars_and_words() {
        let state = EngineState::new('>');

        state.set_engine_mode(EngineMode::AiCapture {
            system_prompt_override: None,
        });
        state.append_ai_prompt_char('h');
        state.append_ai_prompt_char('i');
        state.append_ai_prompt_char(' ');
        state.append_ai_prompt_char('世');
        state.append_ai_prompt_char('界');
        assert_eq!(state.ai_prompt_buffer(), "hi 世界");

        state.pop_ai_prompt_char();
        assert_eq!(state.ai_prompt_buffer(), "hi 世");

        state.pop_ai_prompt_word();
        assert_eq!(state.ai_prompt_buffer(), "hi ");

        state.clear_ai_prompt_buffer();
        assert_eq!(state.ai_prompt_buffer(), "");
        assert!(matches!(state.engine_mode(), EngineMode::AiCapture { .. }));
        assert!(state.is_ai_prompt_empty());
    }
}
