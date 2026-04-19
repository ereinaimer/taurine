use crate::db::crud::AutomationAction;
pub use crate::engine::ai_session::EngineMode;
use crate::engine::ai_session::InlineAiSession;
use crate::engine::catalog::ExpansionCatalog;
use crate::engine::source::SnippetSource;
use crate::engine::variables::FinalExpansion;

use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicU32;

pub struct EngineState {
    pub trigger_char: AtomicU32,
    pub inline_ai_delimiter: AtomicU32,
    pub ai_presets: RwLock<std::collections::HashMap<String, String>>,
    ai_session: InlineAiSession,
    catalog: ExpansionCatalog,
}

impl EngineState {
    pub fn new(trigger_char: char) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            inline_ai_delimiter: AtomicU32::new('`' as u32),
            ai_presets: RwLock::new(std::collections::HashMap::new()),
            ai_session: InlineAiSession::new(),
            catalog: ExpansionCatalog::new(),
        }
    }

    /// Creates an EngineState with a custom snippet source.
    pub fn with_source(trigger_char: char, source: Arc<dyn SnippetSource>) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            inline_ai_delimiter: AtomicU32::new('`' as u32),
            ai_presets: RwLock::new(std::collections::HashMap::new()),
            ai_session: InlineAiSession::new(),
            catalog: ExpansionCatalog::with_source(source),
        }
    }

    pub fn engine_mode(&self) -> EngineMode {
        self.ai_session.engine_mode()
    }

    pub fn set_engine_mode(&self, mode: EngineMode) {
        self.ai_session.set_engine_mode(mode);
    }

    pub fn append_ai_prompt_char(&self, c: char) {
        self.ai_session.append_prompt_char(c);
    }

    pub fn pop_ai_prompt_char(&self) {
        self.ai_session.pop_prompt_char();
    }

    pub fn pop_ai_prompt_word(&self) {
        self.ai_session.pop_prompt_word();
    }

    pub fn clear_ai_prompt_buffer(&self) {
        self.ai_session.clear_prompt_buffer();
    }

    pub fn ai_prompt_buffer(&self) -> String {
        self.ai_session.prompt_buffer()
    }

    pub fn is_ai_prompt_empty(&self) -> bool {
        self.ai_session.is_prompt_empty()
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
