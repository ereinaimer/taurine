use crate::db::crud::AutomationAction;
pub use crate::engine::ai_session::EngineMode;
use crate::engine::ai_session::InlineAiSession;
use crate::engine::catalog::{
    ExpansionCatalog, HotkeyCatalog, RegexCatalog, expand_automation_action,
};
use crate::engine::source::SnippetSource;
use crate::engine::variables::FinalExpansion;
use crate::keys::Hotkey;

use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
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
    inline_ai_trigger_mode: RwLock<crate::settings::InlineAiTriggerMode>,
    inline_ai_trigger: RwLock<String>,
    inline_ai_trigger_open: RwLock<String>,
    inline_ai_trigger_close: RwLock<String>,
    pub inline_tab_completion_enabled: AtomicBool,
    pub inline_history_enabled: AtomicBool,
    pub triggerless_mode: AtomicBool,
    pub instant_expand: AtomicBool,
    pub ignore_fullscreen_enabled: AtomicBool,
    pub is_os_fullscreen: AtomicBool,
    pub completion_active: AtomicBool,
    pub wpm: AtomicU32,
    pub clipboard_restore_delay_ms: AtomicU32,
    pub script_timeout: AtomicU32,

    pub spinner_style: RwLock<crate::settings::SpinnerStyle>,
    action_key: AtomicU8,
    undo_state: RwLock<Option<UndoState>>,
    ai_session: InlineAiSession,
    word_catalog: ExpansionCatalog,
    hotkey_catalog: HotkeyCatalog,
    pub regex_catalog: RegexCatalog,
}

impl EngineState {
    pub fn new(trigger_char: char) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            inline_ai_trigger_mode: RwLock::new(crate::settings::InlineAiTriggerMode::default()),
            inline_ai_trigger: RwLock::new("^".to_string()),
            inline_ai_trigger_open: RwLock::new(">>".to_string()),
            inline_ai_trigger_close: RwLock::new("<<".to_string()),
            inline_tab_completion_enabled: AtomicBool::new(true),
            inline_history_enabled: AtomicBool::new(true),
            triggerless_mode: AtomicBool::new(false),
            instant_expand: AtomicBool::new(false),
            ignore_fullscreen_enabled: AtomicBool::new(true),
            is_os_fullscreen: AtomicBool::new(false),
            completion_active: AtomicBool::new(false),
            wpm: AtomicU32::new(60),
            clipboard_restore_delay_ms: AtomicU32::new(250),
            script_timeout: AtomicU32::new(15),

            spinner_style: RwLock::new(crate::settings::SpinnerStyle::default()),
            action_key: AtomicU8::new(1),
            undo_state: RwLock::new(None),
            ai_session: InlineAiSession::new(),
            word_catalog: ExpansionCatalog::new(),
            hotkey_catalog: HotkeyCatalog::new(),
            regex_catalog: RegexCatalog::new(),
        }
    }

    /// Creates an EngineState with a custom snippet source.
    pub fn with_source(trigger_char: char, source: Arc<dyn SnippetSource>) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            inline_ai_trigger_mode: RwLock::new(crate::settings::InlineAiTriggerMode::default()),
            inline_ai_trigger: RwLock::new("^".to_string()),
            inline_ai_trigger_open: RwLock::new(">>".to_string()),
            inline_ai_trigger_close: RwLock::new("<<".to_string()),
            inline_tab_completion_enabled: AtomicBool::new(true),
            inline_history_enabled: AtomicBool::new(true),
            triggerless_mode: AtomicBool::new(false),
            instant_expand: AtomicBool::new(false),
            ignore_fullscreen_enabled: AtomicBool::new(true),
            is_os_fullscreen: AtomicBool::new(false),
            completion_active: AtomicBool::new(false),
            wpm: AtomicU32::new(60),
            clipboard_restore_delay_ms: AtomicU32::new(250),
            script_timeout: AtomicU32::new(15),

            spinner_style: RwLock::new(crate::settings::SpinnerStyle::default()),
            action_key: AtomicU8::new(1),
            undo_state: RwLock::new(None),
            ai_session: InlineAiSession::new(),
            word_catalog: ExpansionCatalog::with_source(source),
            hotkey_catalog: HotkeyCatalog::new(),
            regex_catalog: RegexCatalog::new(),
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

    pub fn inline_tab_completion_enabled(&self) -> bool {
        self.inline_tab_completion_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn inline_history_enabled(&self) -> bool {
        self.inline_history_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn inline_emoji_enabled(&self) -> bool {
        crate::settings::get_cached_inline_emoji_enabled()
    }

    pub fn inline_emoji_trigger_char(&self) -> char {
        crate::settings::get_cached_inline_emoji_trigger_char()
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

    pub fn load_regex_actions(
        &self,
        actions: impl IntoIterator<Item = (String, AutomationAction)>,
    ) {
        self.regex_catalog.load_actions(actions);
    }

    pub fn match_regex_action(
        &self,
        buffer_string: &str,
        active_window: Option<&str>,
    ) -> Option<(String, AutomationAction, Vec<String>)> {
        self.regex_catalog
            .match_action(buffer_string, active_window)
    }

    pub fn fetch_expansion(
        &self,
        keyword: &str,
        active_window: Option<&str>,
    ) -> Option<FinalExpansion> {
        let instant = self
            .instant_expand
            .load(std::sync::atomic::Ordering::Relaxed);
        self.word_catalog
            .fetch_expansion(keyword, instant, active_window)
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

    pub fn has_hotkey_entry_for(&self, key: crate::keys::LogicalKey) -> bool {
        self.hotkey_catalog.has_entry_for(key)
    }

    pub fn get_hotkey_action(&self, trigger: &str) -> Option<AutomationAction> {
        self.hotkey_catalog.get_action(trigger)
    }

    pub fn fetch_hotkey_expansion(
        &self,
        hotkey: Hotkey,
        active_window: Option<&str>,
    ) -> Option<(String, FinalExpansion)> {
        let (trigger, action) = self.hotkey_catalog.match_action(hotkey, active_window)?;
        let expansion = expand_automation_action(action, &trigger)?;
        Some((trigger, expansion))
    }

    pub fn fetch_hotkey_expansion_lazy(
        &self,
        hotkey: Hotkey,
        fetch_window: impl FnOnce() -> Option<String>,
    ) -> Option<(String, FinalExpansion)> {
        let (trigger, action) = self
            .hotkey_catalog
            .match_action_lazy(hotkey, fetch_window)?;
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

    pub fn action_key(&self) -> crate::settings::ActionKey {
        match self.action_key.load(std::sync::atomic::Ordering::Relaxed) {
            1 => crate::settings::ActionKey::Enter,
            _ => crate::settings::ActionKey::Space,
        }
    }

    pub fn set_action_key(&self, key: crate::settings::ActionKey) {
        self.action_key.store(
            match key {
                crate::settings::ActionKey::Space => 0,
                crate::settings::ActionKey::Enter => 1,
            },
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub fn set_inline_ai_trigger_mode(&self, mode: crate::settings::InlineAiTriggerMode) {
        if let Ok(mut guard) = self.inline_ai_trigger_mode.write() {
            *guard = mode;
        }
    }

    pub fn set_inline_ai_trigger_open(&self, open: String) {
        if let Ok(mut guard) = self.inline_ai_trigger_open.write() {
            *guard = open;
        }
    }

    pub fn set_inline_ai_trigger(&self, delim: String) {
        if let Ok(mut guard) = self.inline_ai_trigger.write() {
            *guard = delim;
        }
    }

    pub fn set_inline_ai_trigger_close(&self, close: String) {
        if let Ok(mut guard) = self.inline_ai_trigger_close.write() {
            *guard = close;
        }
    }

    pub fn get_inline_ai_trigger_mode(&self) -> crate::settings::InlineAiTriggerMode {
        self.inline_ai_trigger_mode
            .read()
            .map(|guard| *guard)
            .unwrap_or_default()
    }

    pub fn get_inline_ai_trigger_open(&self) -> String {
        self.inline_ai_trigger_open
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| ">>".to_string())
    }

    pub fn get_inline_ai_trigger(&self) -> String {
        self.inline_ai_trigger
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| "^".to_string())
    }

    pub fn get_inline_ai_trigger_close(&self) -> String {
        self.inline_ai_trigger_close
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| "<<".to_string())
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
        assert!(state.inline_tab_completion_enabled());
        assert!(state.inline_history_enabled());
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

        assert!(state.fetch_expansion("ctrl+shift+g", None).is_none());
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
            AutomationAction::text("git [key(enter)]status"),
        )]);

        let (trigger, expansion) = state
            .fetch_hotkey_expansion(
                KeyPress {
                    modifiers: modifiers_with(&[Modifier::Ctrl, Modifier::Shift]),
                    key: LogicalKey::Letter('g'),
                },
                None,
            )
            .expect("hotkey expansion should resolve");

        assert_eq!(trigger, "ctrl+shift+g");
        assert!(state.fetch_expansion("ctrl+shift+g", None).is_none());
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
            .fetch_hotkey_expansion(
                KeyPress {
                    modifiers: modifiers_with(&[Modifier::RightAlt]),
                    key: LogicalKey::Letter('m'),
                },
                None,
            )
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
            .fetch_hotkey_expansion(
                KeyPress {
                    modifiers: modifiers_with(&[Modifier::RightAlt]),
                    key: LogicalKey::Letter('m'),
                },
                None,
            )
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
