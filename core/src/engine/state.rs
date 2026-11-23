use crate::db::crud::TriggerAction;
use crate::engine::case_cycle::{CaseCycleSession, CycleDirection};
use crate::engine::catalog::{
    ActiveWindowInfo, ExpansionCatalog, HotkeyCatalog, RegexCatalog, expand_trigger_action,
};
use crate::engine::source::SnippetSource;
use crate::engine::variables::FinalExpansion;
use crate::keys::Hotkey;

use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::{Duration, Instant};

const UNDO_WINDOW: Duration = Duration::from_millis(5000);

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
    inline_date_format: parking_lot::RwLock<String>,
    inline_time_format: parking_lot::RwLock<String>,
    inline_datetime_format: parking_lot::RwLock<String>,
    pub instant_expand: AtomicBool,
    pub ignore_fullscreen_enabled: AtomicBool,
    pub is_os_fullscreen: AtomicBool,
    pub completion_active: AtomicBool,

    undo_state: RwLock<Option<UndoState>>,
    case_cycle: RwLock<Option<CaseCycleSession>>,
    word_catalog: ExpansionCatalog,
    hotkey_catalog: HotkeyCatalog,
    pub regex_catalog: RegexCatalog,
    /// Longest loaded word trigger (chars), clamped to
    /// `[DEFAULT_SUFFIX_CAP, MAX_SUFFIX_CAP]`: drives the suffix scan depth so
    /// long triggers stay matchable. Floor keeps the default depth for
    /// math/date/conversion fallbacks that are not catalog triggers.
    max_suffix_scan_len: AtomicUsize,
    pub source: Arc<dyn SnippetSource>,
}

impl Default for EngineState {
    fn default() -> Self {
        Self::new()
    }
}

impl EngineState {
    pub fn new() -> Self {
        let source: Arc<dyn SnippetSource> = Arc::new(crate::engine::source::MemorySource::new());
        Self {
            inline_date_format: parking_lot::RwLock::new("MMMM D, YYYY".to_string()),
            inline_time_format: parking_lot::RwLock::new("h:mm A".to_string()),
            inline_datetime_format: parking_lot::RwLock::new(
                "MMMM D, YYYY 'at' h:mm A".to_string(),
            ),
            instant_expand: AtomicBool::new(false),
            ignore_fullscreen_enabled: AtomicBool::new(true),
            is_os_fullscreen: AtomicBool::new(false),
            completion_active: AtomicBool::new(false),

            undo_state: RwLock::new(None),
            case_cycle: RwLock::new(None),
            word_catalog: ExpansionCatalog::with_source(source.clone()),
            hotkey_catalog: HotkeyCatalog::new(),
            regex_catalog: RegexCatalog::new(),
            max_suffix_scan_len: AtomicUsize::new(crate::engine::buffer::DEFAULT_SUFFIX_CAP),
            source,
        }
    }

    /// Creates an EngineState with a custom snippet source.
    pub fn with_source(source: Arc<dyn SnippetSource>) -> Self {
        Self {
            inline_date_format: parking_lot::RwLock::new("MMMM D, YYYY".to_string()),
            inline_time_format: parking_lot::RwLock::new("h:mm A".to_string()),
            inline_datetime_format: parking_lot::RwLock::new(
                "MMMM D, YYYY 'at' h:mm A".to_string(),
            ),
            instant_expand: AtomicBool::new(false),
            ignore_fullscreen_enabled: AtomicBool::new(true),
            is_os_fullscreen: AtomicBool::new(false),
            completion_active: AtomicBool::new(false),

            undo_state: RwLock::new(None),
            case_cycle: RwLock::new(None),
            word_catalog: ExpansionCatalog::with_source(source.clone()),
            hotkey_catalog: HotkeyCatalog::new(),
            regex_catalog: RegexCatalog::new(),
            max_suffix_scan_len: AtomicUsize::new(crate::engine::buffer::DEFAULT_SUFFIX_CAP),
            source,
        }
    }

    pub fn inline_ai_enabled(&self) -> bool {
        true
    }

    pub fn inline_tab_completion_enabled(&self) -> bool {
        true
    }

    pub fn inline_case_transform_enabled(&self) -> bool {
        true
    }

    /// True when expansions and injections must be suppressed: the user
    /// enabled "ignore fullscreen" and the OS reports a fullscreen
    /// foreground window. Voice, expansions, and hotkeys share this check.
    pub fn fullscreen_suppressed(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.ignore_fullscreen_enabled.load(Ordering::Relaxed)
            && self.is_os_fullscreen.load(Ordering::Relaxed)
    }

    pub fn inline_emoji_enabled(&self) -> bool {
        crate::settings::get_cached_inline_emoji_enabled()
    }

    pub fn inline_emoji_trigger_char(&self) -> char {
        crate::settings::get_cached_inline_emoji_trigger_char()
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, TriggerAction)>) {
        let actions: Vec<_> = actions.into_iter().collect();
        let longest = actions
            .iter()
            .map(|(trigger, _)| trigger.chars().count())
            .max()
            .unwrap_or(0);
        self.max_suffix_scan_len.store(
            longest.clamp(
                crate::engine::buffer::DEFAULT_SUFFIX_CAP,
                crate::engine::buffer::MAX_SUFFIX_CAP,
            ),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.word_catalog.load_actions(actions);
    }

    /// Suffix scan depth for trigger matching: covers the longest loaded
    /// trigger so triggers beyond the default depth can still expand.
    pub fn max_suffix_scan_len(&self) -> usize {
        self.max_suffix_scan_len
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn load_hotkey_actions(&self, actions: impl IntoIterator<Item = (String, TriggerAction)>) {
        self.hotkey_catalog.load_actions(actions);
    }

    pub fn load_regex_actions(&self, actions: impl IntoIterator<Item = (String, TriggerAction)>) {
        self.regex_catalog.load_actions(actions);
    }

    pub fn match_regex_action(
        &self,
        buffer_string: &str,
        active_window: Option<&str>,
    ) -> Option<(String, TriggerAction, Vec<String>)> {
        self.regex_catalog
            .match_action(buffer_string, active_window)
    }

    pub fn match_regex_action_lazy(
        &self,
        buffer_string: &str,
        window: &crate::engine::catalog::WindowResolver,
        fetch_window: Option<impl FnOnce() -> Option<ActiveWindowInfo>>,
    ) -> Option<(String, TriggerAction, Vec<String>)> {
        self.regex_catalog
            .match_action_lazy(buffer_string, window, fetch_window)
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

    pub fn fetch_expansion_lazy(
        &self,
        keyword: &str,
        window: &crate::engine::catalog::WindowResolver,
        fetch_window: Option<impl FnOnce() -> Option<ActiveWindowInfo>>,
    ) -> Option<FinalExpansion> {
        let instant = self
            .instant_expand
            .load(std::sync::atomic::Ordering::Relaxed);
        self.word_catalog
            .fetch_expansion_lazy(keyword, instant, window, fetch_window)
    }

    pub fn fetch_expansion_no_date_fallback(
        &self,
        keyword: &str,
        active_window: Option<&str>,
    ) -> Option<FinalExpansion> {
        let instant = self
            .instant_expand
            .load(std::sync::atomic::Ordering::Relaxed);
        self.word_catalog
            .fetch_expansion_no_date_fallback(keyword, instant, active_window)
    }

    pub fn fetch_expansion_no_date_fallback_lazy(
        &self,
        keyword: &str,
        window: &crate::engine::catalog::WindowResolver,
        fetch_window: Option<impl FnOnce() -> Option<ActiveWindowInfo>>,
    ) -> Option<FinalExpansion> {
        let instant = self
            .instant_expand
            .load(std::sync::atomic::Ordering::Relaxed);
        self.word_catalog.fetch_expansion_no_date_fallback_lazy(
            keyword,
            instant,
            window,
            fetch_window,
        )
    }

    pub fn matching_word_triggers(&self, prefix: &str) -> Vec<String> {
        self.word_catalog.matching_triggers(prefix)
    }

    pub fn has_hotkey_entry_for(&self, key: crate::keys::LogicalKey) -> bool {
        self.hotkey_catalog.has_entry_for(key)
    }

    pub fn get_hotkey_action(&self, trigger: &str) -> Option<TriggerAction> {
        self.hotkey_catalog.get_action(trigger)
    }

    pub fn fetch_hotkey_expansion(
        &self,
        hotkey: Hotkey,
        active_window: Option<&str>,
    ) -> Option<(String, FinalExpansion)> {
        let (trigger, action) = self.hotkey_catalog.match_action(hotkey, active_window)?;
        let expansion = expand_trigger_action(action, &trigger)?;
        Some((trigger, expansion))
    }

    pub fn fetch_hotkey_expansion_info(
        &self,
        hotkey: Hotkey,
        active_window: Option<&ActiveWindowInfo>,
    ) -> Option<(String, FinalExpansion)> {
        let (trigger, action) = self
            .hotkey_catalog
            .match_action_info(hotkey, active_window)?;
        let expansion = expand_trigger_action(action, &trigger)?;
        Some((trigger, expansion))
    }

    pub fn fetch_hotkey_expansion_lazy(
        &self,
        hotkey: Hotkey,
        fetch_window: impl FnOnce() -> Option<ActiveWindowInfo>,
    ) -> Option<(String, FinalExpansion)> {
        let (trigger, action) = self
            .hotkey_catalog
            .match_action_lazy(hotkey, fetch_window)?;
        let expansion = expand_trigger_action(action, &trigger)?;
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

    pub fn refresh_undo_state(&self, trigger_string: &str, output_length: usize) {
        if let Ok(mut guard) = self.undo_state.write()
            && let Some(state) = guard.as_mut()
            && state.trigger_string == trigger_string
        {
            state.output_length = output_length;
            state.timestamp = Instant::now();
        }
    }

    pub fn inline_datetime_enabled(&self) -> bool {
        true
    }
    pub fn inline_currency_to_words_enabled(&self) -> bool {
        true
    }
    pub fn inline_dictionary_enabled(&self) -> bool {
        true
    }
    pub fn get_inline_date_format(&self) -> String {
        self.inline_date_format.read().clone()
    }
    pub fn set_inline_date_format(&self, f: String) {
        *self.inline_date_format.write() = f;
    }
    pub fn get_inline_time_format(&self) -> String {
        self.inline_time_format.read().clone()
    }
    pub fn set_inline_time_format(&self, f: String) {
        *self.inline_time_format.write() = f;
    }
    pub fn get_inline_datetime_format(&self) -> String {
        self.inline_datetime_format.read().clone()
    }
    pub fn set_inline_datetime_format(&self, f: String) {
        *self.inline_datetime_format.write() = f;
    }
    pub fn get_inline_datetime_date_format(&self) -> String {
        self.get_inline_date_format()
    }
    pub fn set_inline_datetime_date_format(&self, f: String) {
        self.set_inline_date_format(f);
    }
    pub fn get_inline_datetime_time_format(&self) -> String {
        self.get_inline_time_format()
    }
    pub fn set_inline_datetime_time_format(&self, f: String) {
        self.set_inline_time_format(f);
    }
    pub fn get_inline_datetime_datetime_format(&self) -> String {
        self.get_inline_datetime_format()
    }
    pub fn set_inline_datetime_datetime_format(&self, f: String) {
        self.set_inline_datetime_format(f);
    }
    pub fn get_inline_datetime_dialect(&self) -> String {
        crate::engine::dates::detect_locale_dialect().to_string()
    }
    pub fn set_inline_datetime_dialect(&self, _d: String) {}

    pub fn register_case_cycle(&self, original_text: String) {
        if let Ok(mut guard) = self.case_cycle.write() {
            *guard = Some(CaseCycleSession::new(original_text));
        }
    }

    pub fn advance_case_variant(
        &self,
        dir: CycleDirection,
    ) -> Option<crate::engine::CompletionRewrite> {
        let mut case_guard = self.case_cycle.write().ok()?;
        let session = case_guard.as_mut()?;

        let now = std::time::Instant::now();
        if !session.is_ready(now) {
            return None;
        }

        let previous_len = session.current_text.chars().count();
        let new_text = session.advance(dir);

        if let Ok(mut undo_guard) = self.undo_state.write()
            && let Some(undo) = undo_guard.as_mut()
        {
            undo.output_length = new_text.chars().count();
        }

        Some(crate::engine::CompletionRewrite {
            delete_count: previous_len,
            replacement: new_text,
        })
    }

    fn clear_case_cycle(&self) {
        if let Ok(mut guard) = self.case_cycle.write() {
            *guard = None;
        }
    }

    pub fn clear_undo_state(&self) {
        self.clear_case_cycle();
        if let Ok(mut guard) = self.undo_state.write() {
            *guard = None;
        }
    }

    pub fn take_active_undo_state(&self) -> Option<UndoState> {
        self.clear_case_cycle();
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
    fn engine_state_defaults() {
        let state = EngineState::new();

        assert!(state.inline_ai_enabled());
        assert!(state.inline_tab_completion_enabled());
    }

    #[test]
    fn max_suffix_scan_len_tracks_longest_loaded_trigger() {
        let state = EngineState::new();
        assert_eq!(
            state.max_suffix_scan_len(),
            crate::engine::buffer::DEFAULT_SUFFIX_CAP
        );

        state.load_actions(vec![("x".repeat(45), TriggerAction::text("long"))]);
        assert_eq!(state.max_suffix_scan_len(), 45);

        state.load_actions(vec![("short".to_string(), TriggerAction::text("s"))]);
        assert_eq!(
            state.max_suffix_scan_len(),
            crate::engine::buffer::DEFAULT_SUFFIX_CAP
        );
    }

    #[test]
    fn fullscreen_suppressed_requires_both_flags() {
        use std::sync::atomic::Ordering;
        let state = EngineState::new();
        assert!(!state.fullscreen_suppressed());
        state.is_os_fullscreen.store(true, Ordering::Relaxed);
        assert!(state.fullscreen_suppressed());
        state
            .ignore_fullscreen_enabled
            .store(false, Ordering::Relaxed);
        assert!(!state.fullscreen_suppressed());
        state.is_os_fullscreen.store(false, Ordering::Relaxed);
        assert!(!state.fullscreen_suppressed());
    }

    #[test]
    fn undo_state_round_trips_while_active() {
        let state = EngineState::new();

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
        let state = EngineState::new();
        let expired = UndoState {
            trigger_string: ">gm".to_string(),
            output_length: 12,
            timestamp: Instant::now() - (UNDO_WINDOW + Duration::from_millis(100)),
        };

        *state.undo_state.write().expect("undo lock") = Some(expired);

        assert!(state.take_active_undo_state().is_none());
        assert!(state.undo_state.read().expect("undo lock").is_none());
    }

    #[test]
    fn hotkey_actions_load_into_separate_catalog_from_word_actions() {
        let state = EngineState::new();

        state.load_actions(vec![(
            "gm".to_string(),
            TriggerAction::text("good morning"),
        )]);
        state.load_hotkey_actions(vec![(
            "ctrl+shift+g".to_string(),
            TriggerAction::text("git status"),
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
        let state = EngineState::new();

        state.load_actions(vec![(
            "gm".to_string(),
            TriggerAction::text("good morning"),
        )]);
        state.load_hotkey_actions(vec![(
            "ctrl+shift+g".to_string(),
            TriggerAction::text("git [enter]status"),
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
        let state = EngineState::new();
        state.load_hotkey_actions(vec![(
            "alt+m".to_string(),
            TriggerAction::text("monkeytype"),
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
        let state = EngineState::new();
        state.load_hotkey_actions(vec![
            ("alt+m".to_string(), TriggerAction::text("generic")),
            ("ralt+m".to_string(), TriggerAction::text("right")),
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
        let state = EngineState::new();
        state.load_actions(vec![
            ("gpush".to_string(), TriggerAction::text("git push")),
            ("gs".to_string(), TriggerAction::text("git status")),
        ]);
        state.load_hotkey_actions(vec![("ctrl+g".to_string(), TriggerAction::text("hotkey"))]);

        assert_eq!(
            state.matching_word_triggers("g"),
            vec!["gpush".to_string(), "gs".to_string()]
        );
    }

    #[test]
    fn case_cycle_advances_and_syncs_undo_length() {
        let state = EngineState::new();
        state.set_undo_state(">gm".to_string(), 12);
        state.register_case_cycle("good morning".to_string());

        let rewrite = state
            .advance_case_variant(CycleDirection::Next)
            .expect("ready");
        assert_eq!(rewrite.delete_count, 12);
        assert_eq!(rewrite.replacement, "Good morning");

        let after = state.take_active_undo_state().expect("undo survives");
        assert_eq!(after.output_length, rewrite.replacement.chars().count());
    }

    #[test]
    fn expired_case_state_returns_none_without_clearing_other() {
        let state = EngineState::new();
        state.set_undo_state(">gm".to_string(), 12);
        // Not registered case cycle -> advance_case_variant returns None
        assert!(state.advance_case_variant(CycleDirection::Next).is_none());

        // Undo state still survives
        assert!(state.take_active_undo_state().is_some());
    }

    #[test]
    fn clear_undo_state_clears_case_cycle() {
        let state = EngineState::new();
        state.set_undo_state(">gm".to_string(), 12);
        state.register_case_cycle("good morning".to_string());

        state.clear_undo_state();
        assert!(state.advance_case_variant(CycleDirection::Next).is_none());
    }
}
