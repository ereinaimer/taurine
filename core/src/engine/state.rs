use crate::db::crud::AutomationAction;
pub use crate::engine::ai_session::EngineMode;
use crate::engine::ai_session::InlineAiSession;
use crate::engine::catalog::{ExpansionCatalog, HotkeyCatalog, expand_automation_action};
use crate::engine::source::SnippetSource;
use crate::engine::variables::FinalExpansion;
use crate::keys::Hotkey;

use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicU32;
use std::time::{Duration, Instant};

const UNDO_WINDOW: Duration = Duration::from_millis(2500);

#[derive(Clone, Debug)]
pub struct UndoState {
    pub trigger_string: String,
    pub output_length: usize,
    pub timestamp: Instant,
}

impl UndoState {
    fn new(trigger_string: String, output_length: usize) -> Self {
        Self {
            trigger_string,
            output_length,
            timestamp: Instant::now(),
        }
    }

    fn is_active(&self) -> bool {
        self.timestamp.elapsed() < UNDO_WINDOW
    }
}

pub struct EngineState {
    pub trigger_char: AtomicU32,
    pub inline_ai_delimiter: AtomicU32,
    pub ai_presets: RwLock<std::collections::HashMap<String, String>>,
    pub spinner_style: RwLock<crate::settings::SpinnerStyle>,
    undo_state: RwLock<Option<UndoState>>,
    ai_session: InlineAiSession,
    word_catalog: ExpansionCatalog,
    hotkey_catalog: HotkeyCatalog,
}

impl EngineState {
    pub fn new(trigger_char: char) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            inline_ai_delimiter: AtomicU32::new('`' as u32),
            ai_presets: RwLock::new(std::collections::HashMap::new()),
            spinner_style: RwLock::new(crate::settings::SpinnerStyle::default()),
            undo_state: RwLock::new(None),
            ai_session: InlineAiSession::new(),
            word_catalog: ExpansionCatalog::new(),
            hotkey_catalog: HotkeyCatalog::new(),
        }
    }

    /// Creates an EngineState with a custom snippet source.
    pub fn with_source(trigger_char: char, source: Arc<dyn SnippetSource>) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            inline_ai_delimiter: AtomicU32::new('`' as u32),
            ai_presets: RwLock::new(std::collections::HashMap::new()),
            spinner_style: RwLock::new(crate::settings::SpinnerStyle::default()),
            undo_state: RwLock::new(None),
            ai_session: InlineAiSession::new(),
            word_catalog: ExpansionCatalog::with_source(source),
            hotkey_catalog: HotkeyCatalog::new(),
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
        self.word_catalog.load_actions(actions);
    }

    pub fn load_word_trigger_history(&self, triggers: impl IntoIterator<Item = String>) {
        self.word_catalog.load_history_triggers(triggers);
    }

    pub fn load_hotkey_actions(
        &self,
        actions: impl IntoIterator<Item = (String, AutomationAction)>,
    ) {
        self.hotkey_catalog.load_actions(actions);
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
        self.word_catalog.fetch_expansion(keyword)
    }

    pub fn matching_word_triggers(&self, prefix: &str) -> Vec<String> {
        self.word_catalog.matching_triggers(prefix)
    }

    pub fn matching_word_trigger_history(&self, prefix: &str) -> Vec<String> {
        self.word_catalog.matching_history_triggers(prefix)
    }

    pub fn record_word_trigger_usage(&self, trigger: &str) {
        self.word_catalog.promote_history_trigger(trigger);
    }

    pub fn get_hotkey_action(&self, trigger: &str) -> Option<AutomationAction> {
        self.hotkey_catalog.get_action(trigger)
    }

    pub fn fetch_hotkey_expansion(&self, hotkey: Hotkey) -> Option<(String, FinalExpansion)> {
        let (trigger, action) = self.hotkey_catalog.match_action(hotkey)?;
        let expansion = expand_automation_action(action, &trigger)?;
        Some((trigger, expansion))
    }

    pub fn set_undo_state(&self, trigger_string: String, output_length: usize) {
        if trigger_string.is_empty() || output_length == 0 {
            self.clear_undo_state();
            return;
        }

        if let Ok(mut guard) = self.undo_state.write() {
            *guard = Some(UndoState::new(trigger_string, output_length));
        }
    }

    pub fn clear_undo_state(&self) {
        if let Ok(mut guard) = self.undo_state.write() {
            *guard = None;
        }
    }

    pub fn take_active_undo_state(&self) -> Option<UndoState> {
        let mut guard = self.undo_state.write().ok()?;
        match guard.as_ref() {
            Some(state) if state.is_active() => guard.take(),
            Some(_) => {
                *guard = None;
                None
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{KeyPress, LogicalKey, Modifier, Modifiers};

    fn modifiers_with(modifiers: &[Modifier]) -> Modifiers {
        let mut bitset = Modifiers::new();
        for modifier in modifiers {
            let _ = bitset.insert(*modifier);
        }
        bitset
    }

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

    #[test]
    fn undo_state_round_trips_while_active() {
        let state = EngineState::new('>');

        state.set_undo_state(">gm".to_string(), 12);
        let undo = state
            .take_active_undo_state()
            .expect("undo state should exist");

        assert_eq!(undo.trigger_string, ">gm");
        assert_eq!(undo.output_length, 12);
        assert!(state.take_active_undo_state().is_none());
    }

    #[test]
    fn expired_undo_state_is_cleared_on_access() {
        let state = EngineState::new('>');
        let expired = UndoState {
            trigger_string: ">gm".to_string(),
            output_length: 12,
            timestamp: Instant::now() - Duration::from_millis(2600),
        };

        *state.undo_state.write().expect("undo lock") = Some(expired);

        assert!(state.take_active_undo_state().is_none());
        assert!(state.undo_state.read().expect("undo lock").is_none());
    }

    #[test]
    fn hotkey_actions_load_into_separate_catalog_from_word_actions() {
        let state = EngineState::new('>');

        state.load_actions(vec![(
            "gm".to_string(),
            AutomationAction::text("good morning"),
        )]);
        state.load_hotkey_actions(vec![(
            "ctrl+shift+g".to_string(),
            AutomationAction::text("git status"),
        )]);

        assert!(state.fetch_expansion("ctrl+shift+g").is_none());
        assert_eq!(
            state.get_hotkey_action("ctrl+shift+g").unwrap().output,
            "git status"
        );
        assert!(state.get_hotkey_action("gm").is_none());
    }

    #[test]
    fn fetch_hotkey_expansion_builds_steps_without_entering_word_catalog() {
        let state = EngineState::new('>');

        state.load_actions(vec![(
            "gm".to_string(),
            AutomationAction::text("good morning"),
        )]);
        state.load_hotkey_actions(vec![(
            "ctrl+shift+g".to_string(),
            AutomationAction::text("git [key.enter]status"),
        )]);

        let (trigger, expansion) = state
            .fetch_hotkey_expansion(KeyPress {
                modifiers: modifiers_with(&[Modifier::Ctrl, Modifier::Shift]),
                key: LogicalKey::Letter('g'),
            })
            .expect("hotkey expansion should resolve");

        assert_eq!(trigger, "ctrl+shift+g");
        assert!(state.fetch_expansion("ctrl+shift+g").is_none());
        assert!(
            expansion.steps.iter().any(
                |step| matches!(step, crate::engine::variables::ExpansionStep::KeyPress(alias) if alias == "enter")
            )
        );
    }

    #[test]
    fn fetch_hotkey_expansion_matches_generic_hotkeys_from_side_specific_runtime_state() {
        let state = EngineState::new('>');
        state.load_hotkey_actions(vec![(
            "alt+m".to_string(),
            AutomationAction::text("monkeytype"),
        )]);

        let (trigger, expansion) = state
            .fetch_hotkey_expansion(KeyPress {
                modifiers: modifiers_with(&[Modifier::RightAlt]),
                key: LogicalKey::Letter('m'),
            })
            .expect("generic alt hotkey should match right alt");

        assert_eq!(trigger, "alt+m");
        assert_eq!(
            expansion.steps[0],
            crate::engine::variables::ExpansionStep::Text("monkeytype".to_string())
        );
    }

    #[test]
    fn fetch_hotkey_expansion_prefers_exact_side_specific_trigger() {
        let state = EngineState::new('>');
        state.load_hotkey_actions(vec![
            ("alt+m".to_string(), AutomationAction::text("generic")),
            ("ralt+m".to_string(), AutomationAction::text("right")),
        ]);

        let (trigger, expansion) = state
            .fetch_hotkey_expansion(KeyPress {
                modifiers: modifiers_with(&[Modifier::RightAlt]),
                key: LogicalKey::Letter('m'),
            })
            .expect("side-specific hotkey should resolve");

        assert_eq!(trigger, "ralt+m");
        assert_eq!(
            expansion.steps[0],
            crate::engine::variables::ExpansionStep::Text("right".to_string())
        );
    }

    #[test]
    fn matching_word_triggers_reads_only_the_word_catalog() {
        let state = EngineState::new('>');
        state.load_actions(vec![
            ("gpush".to_string(), AutomationAction::text("git push")),
            ("gs".to_string(), AutomationAction::text("git status")),
        ]);
        state.load_hotkey_actions(vec![(
            "ctrl+g".to_string(),
            AutomationAction::text("hotkey"),
        )]);

        assert_eq!(
            state.matching_word_triggers("g"),
            vec!["gpush".to_string(), "gs".to_string()]
        );
    }

    #[test]
    fn matching_word_trigger_history_reads_only_the_word_catalog() {
        let state = EngineState::new('>');
        state.load_actions(vec![
            ("gpush".to_string(), AutomationAction::text("git push")),
            ("gs".to_string(), AutomationAction::text("git status")),
        ]);
        state.load_word_trigger_history(vec!["gs".to_string(), "gpush".to_string()]);
        state.load_hotkey_actions(vec![(
            "ctrl+g".to_string(),
            AutomationAction::text("hotkey"),
        )]);

        assert_eq!(
            state.matching_word_trigger_history("g"),
            vec!["gs".to_string(), "gpush".to_string()]
        );
    }

    #[test]
    fn record_word_trigger_usage_promotes_history_without_touching_hotkeys() {
        let state = EngineState::new('>');
        state.load_actions(vec![
            ("email".to_string(), AutomationAction::text("team update")),
            ("gs".to_string(), AutomationAction::text("git status")),
            ("uuid".to_string(), AutomationAction::text("1234")),
        ]);
        state.load_word_trigger_history(vec![
            "gs".to_string(),
            "email".to_string(),
            "uuid".to_string(),
        ]);
        state.load_hotkey_actions(vec![(
            "ctrl+shift+g".to_string(),
            AutomationAction::text("hotkey"),
        )]);

        state.record_word_trigger_usage("uuid");
        state.record_word_trigger_usage("ctrl+shift+g");

        assert_eq!(
            state.matching_word_trigger_history(""),
            vec!["uuid".to_string(), "gs".to_string(), "email".to_string()]
        );
    }
}
