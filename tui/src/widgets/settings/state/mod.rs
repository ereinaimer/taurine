mod hotkey_capture;
mod keys;
mod modals;

pub(crate) use hotkey_capture::HotkeyCaptureModalState;
pub(crate) use keys::{EditorKind, SettingKey, SettingKeyMeta};
pub(crate) use modals::{ConfirmResetModalState, InputModalState, SelectModalState};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::{
    ai::supported_providers,
    settings::{Settings, apply_setting_input, reset_setting_to_default},
};

const SPINNER_STYLE_OPTIONS: [&str; 3] = ["classic", "braille", "arc"];
const AUDIO_THEME_OPTIONS: [&str; 7] = [
    "minimal",
    "arcade",
    "mechanical",
    "organic",
    "scifi",
    "rubber",
    "zen",
];

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SettingsPageState {
    pub(crate) settings: Settings,
    pub(crate) selected: usize,
    pub(crate) modal: Option<SettingsModal>,
    pub(crate) status_message: Option<String>,
    pub(crate) load_error: Option<String>,
    pub(crate) search_query: String,
    pub(crate) search_active: bool,
    pub(crate) window_anchor: Option<usize>,
}

impl SettingsPageState {
    pub(crate) const fn settings(&self) -> &Settings {
        &self.settings
    }

    pub(crate) const fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn visible_keys(&self) -> Vec<SettingKey> {
        let mut keys = SettingKey::ALL.to_vec();

        if !self.settings.inline_datetime_enabled {
            keys.retain(|k| {
                *k != SettingKey::InlineDatetimeDateFormat
                    && *k != SettingKey::InlineDatetimeTimeFormat
                    && *k != SettingKey::InlineDatetimeDatetimeFormat
                    && *k != SettingKey::InlineDatetimeDialect
            });
        }
        let needle = self.search_query.trim().to_ascii_lowercase();
        if !needle.is_empty() {
            keys.retain(|k| {
                k.display_name().to_ascii_lowercase().contains(&needle)
                    || k.description().to_ascii_lowercase().contains(&needle)
            });
        }
        keys
    }

    pub(crate) fn search_query(&self) -> &str {
        &self.search_query
    }

    pub(crate) const fn is_search_active(&self) -> bool {
        self.search_active
    }

    pub(crate) fn activate_search(&mut self) {
        self.search_active = true;
    }

    /// Click parity with Enter: first click selects the row, clicking the
    /// selected row toggles booleans or opens the editor. Selecting records
    /// the click-time window start so the list does not jump.
    pub(crate) fn click_setting(&mut self, key: SettingKey, anchor: usize) -> SettingsInteraction {
        let position = self
            .visible_keys()
            .iter()
            .position(|k| *k == key)
            .unwrap_or(self.selected);
        if self.selected_key() == key {
            if key.editor_kind() == EditorKind::Toggle {
                return self.toggle_selected_setting();
            }
            self.open_editor_for_selected();
            return SettingsInteraction::handled();
        }
        self.selected = position;
        self.window_anchor = Some(anchor);
        SettingsInteraction::handled()
    }

    pub(crate) fn selected_key(&self) -> SettingKey {
        let keys = self.visible_keys();
        keys[self.selected.min(keys.len().saturating_sub(1))]
    }

    pub(crate) const fn modal(&self) -> Option<&SettingsModal> {
        self.modal.as_ref()
    }

    pub(crate) fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    pub(crate) fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub(crate) const fn is_modal_open(&self) -> bool {
        self.modal.is_some()
    }

    pub(crate) fn replace_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.modal = None;
        self.load_error = None;
        self.status_message = None;
        self.window_anchor = None;
    }

    pub(crate) fn set_load_error(&mut self, error: String) {
        self.load_error = Some(error);
    }

    pub(crate) fn set_save_error(&mut self, error: String) {
        if let Some(modal) = self.modal.as_mut() {
            modal.set_error(error);
        } else {
            self.status_message = Some(error);
        }
    }

    pub(crate) fn clear_modal(&mut self) {
        self.modal = None;
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> SettingsInteraction {
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }

        if self.search_active {
            self.handle_search_key(key);
            return SettingsInteraction::handled();
        }

        self.status_message = None;

        match (key.code, key.modifiers) {
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.search_active = true;
                SettingsInteraction::handled()
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_selection(1);
                SettingsInteraction::handled()
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_selection(-1);
                SettingsInteraction::handled()
            }
            (KeyCode::Char('r'), KeyModifiers::NONE) => {
                if self.visible_keys().is_empty() {
                    return SettingsInteraction::handled();
                }
                self.modal = Some(SettingsModal::ConfirmReset(ConfirmResetModalState::new(
                    self.selected_key(),
                )));
                SettingsInteraction::handled()
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if self.visible_keys().is_empty() {
                    return SettingsInteraction::handled();
                }
                if self.selected_key().editor_kind() == EditorKind::Toggle {
                    self.toggle_selected_setting()
                } else {
                    self.open_editor_for_selected();
                    SettingsInteraction::handled()
                }
            }
            // honey: navigation and quit keys never start a search; any other
            // bare character filters the list immediately (type-to-search).
            (KeyCode::Char('1' | '2' | '3' | 'q'), KeyModifiers::NONE) => {
                SettingsInteraction::default()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_active = true;
                self.search_query.push(ch);
                self.selected = 0;
                self.window_anchor = None;
                SettingsInteraction::handled()
            }
            _ => SettingsInteraction::default(),
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                self.search_active = false;
            }
            (KeyCode::Up, KeyModifiers::NONE) => self.move_selection(-1),
            (KeyCode::Down, KeyModifiers::NONE) => self.move_selection(1),
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.search_query.pop();
                self.selected = 0;
                self.window_anchor = None;
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_query.push(ch);
                self.selected = 0;
                self.window_anchor = None;
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let max_index = self.visible_keys().len().saturating_sub(1) as isize;
        let next = (self.selected as isize + delta).clamp(0, max_index);
        self.selected = next as usize;
        self.window_anchor = None;
    }

    fn toggle_selected_setting(&mut self) -> SettingsInteraction {
        let key = self.selected_key();
        let next_value = match key {
            SettingKey::PauseNotificationsEnabled => {
                (!self.settings.pause_notifications_enabled).to_string()
            }
            SettingKey::PauseAudioEnabled => (!self.settings.pause_audio_enabled).to_string(),
            SettingKey::StartOnBoot => (!self.settings.start_on_boot).to_string(),
            SettingKey::AutoUpdate => (!self.settings.auto_update).to_string(),
            SettingKey::NotifyOnUpdate => (!self.settings.notify_on_update).to_string(),
            SettingKey::InlineTabCompletionEnabled => {
                (!self.settings.inline_tab_completion_enabled).to_string()
            }
            SettingKey::InlineCaseTransformEnabled => {
                (!self.settings.inline_case_transform_enabled).to_string()
            }
            SettingKey::InstantExpand => (!self.settings.instant_expand).to_string(),
            SettingKey::IgnoreFullscreen => (!self.settings.ignore_fullscreen).to_string(),
            SettingKey::ScriptsEnabled => (!self.settings.scripts_enabled).to_string(),
            SettingKey::ClipboardHistoryEnabled => {
                (!self.settings.clipboard_history_enabled).to_string()
            }
            SettingKey::InlineEmojiEnabled => (!self.settings.inline_emoji_enabled).to_string(),
            SettingKey::SystemTrayEnabled => (!self.settings.system_tray_enabled).to_string(),
            SettingKey::InlineDatetimeEnabled => {
                (!self.settings.inline_datetime_enabled).to_string()
            }
            SettingKey::InlineCurrencyToWordsEnabled => {
                (!self.settings.inline_currency_to_words_enabled).to_string()
            }
            SettingKey::InlineDictionaryEnabled => {
                (!self.settings.inline_dictionary_enabled).to_string()
            }
            SettingKey::VoiceKeepLoaded => (!self.settings.voice_keep_loaded).to_string(),
            SettingKey::PauseMediaWhileDictating => {
                (!self.settings.pause_media_while_dictating).to_string()
            }
            _ => return SettingsInteraction::handled(),
        };

        SettingsInteraction::save(key, Some(next_value))
    }

    fn open_editor_for_selected(&mut self) {
        let key = self.selected_key();
        self.modal = match key.editor_kind() {
            EditorKind::Toggle => None,
            EditorKind::SpinnerSelect => Some(SettingsModal::Select(SelectModalState::new(
                key,
                SPINNER_STYLE_OPTIONS
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                key.display_value(&self.settings),
            ))),
            EditorKind::AudioThemeSelect => Some(SettingsModal::Select(SelectModalState::new(
                key,
                AUDIO_THEME_OPTIONS
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                key.display_value(&self.settings),
            ))),
            EditorKind::AiProviderSelect => Some(SettingsModal::Select(SelectModalState::new(
                key,
                supported_providers()
                    .iter()
                    .map(|provider| provider.as_str().to_string())
                    .collect(),
                self.settings.ai_provider.clone().unwrap_or_default(),
            ))),
            EditorKind::InlineDictionaryModeSelect => {
                Some(SettingsModal::Select(SelectModalState::new(
                    key,
                    vec!["lite".to_string(), "full".to_string()],
                    key.display_value(&self.settings),
                )))
            }
            EditorKind::HotkeyCapture => Some(SettingsModal::HotkeyCapture(
                HotkeyCaptureModalState::new(key, key.edit_value(&self.settings)),
            )),
            EditorKind::SingleCharInput
            | EditorKind::TextInput
            | EditorKind::OptionalTextInput
            | EditorKind::NumberInput => Some(SettingsModal::Input(InputModalState::new(
                key,
                key.edit_value(&self.settings),
            ))),
        };
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> SettingsInteraction {
        let Some(modal) = self.modal.as_mut() else {
            return SettingsInteraction::default();
        };

        match modal {
            SettingsModal::Input(state) => state.handle_key(key),
            SettingsModal::Select(state) => state.handle_key(key),
            SettingsModal::ConfirmReset(state) => state.handle_key(key),
            SettingsModal::HotkeyCapture(state) => state.handle_key(key),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingsModal {
    Input(InputModalState),
    Select(SelectModalState),
    ConfirmReset(ConfirmResetModalState),
    HotkeyCapture(HotkeyCaptureModalState),
}

impl SettingsModal {
    fn set_error(&mut self, error: String) {
        match self {
            Self::Input(state) => state.error = Some(error),
            Self::Select(state) => state.error = Some(error),
            Self::ConfirmReset(state) => state.error = Some(error),
            Self::HotkeyCapture(state) => state.error = Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSettingSave {
    pub(crate) key: SettingKey,
    pub(crate) value: Option<String>,
}

impl PendingSettingSave {
    pub(crate) fn apply(&self) -> taurine_core::Result<()> {
        apply_setting_input(self.key.storage_key(), self.value.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSettingReset {
    pub(crate) key: SettingKey,
}

impl PendingSettingReset {
    pub(crate) fn apply(&self) -> taurine_core::Result<()> {
        reset_setting_to_default(self.key.storage_key())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SettingsInteraction {
    pending_save: Option<PendingSettingSave>,
    pending_reset: Option<PendingSettingReset>,
    close_modal: bool,
}

impl SettingsInteraction {
    pub(crate) const fn pending_save(&self) -> Option<&PendingSettingSave> {
        self.pending_save.as_ref()
    }

    pub(crate) const fn pending_reset(&self) -> Option<&PendingSettingReset> {
        self.pending_reset.as_ref()
    }

    pub(crate) const fn should_close_modal(&self) -> bool {
        self.close_modal
    }

    fn handled() -> Self {
        Self {
            pending_save: None,
            pending_reset: None,
            close_modal: false,
        }
    }

    fn cancel() -> Self {
        Self {
            pending_save: None,
            pending_reset: None,
            close_modal: true,
        }
    }

    fn save(key: SettingKey, value: Option<String>) -> Self {
        Self {
            pending_save: Some(PendingSettingSave { key, value }),
            pending_reset: None,
            close_modal: false,
        }
    }

    fn reset(key: SettingKey) -> Self {
        Self {
            pending_save: None,
            pending_reset: Some(PendingSettingReset { key }),
            close_modal: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_theme_selection_modal_opens() {
        let mut state = SettingsPageState::default();
        let audio_theme_idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::AudioTheme)
            .expect("AudioTheme should be visible");
        state.selected = audio_theme_idx;
        state.open_editor_for_selected();

        assert!(matches!(state.modal, Some(SettingsModal::Select(_))));
        if let Some(SettingsModal::Select(modal_state)) = state.modal {
            assert_eq!(modal_state.options().len(), 7);
            assert_eq!(
                modal_state.options()[modal_state.selected_index()],
                "minimal"
            );
        }
    }

    #[test]
    fn test_audio_theme_is_always_visible() {
        let mut state = SettingsPageState::default();
        state.settings.pause_audio_enabled = true;
        assert!(state.visible_keys().contains(&SettingKey::AudioTheme));

        state.settings.pause_audio_enabled = false;
        assert!(state.visible_keys().contains(&SettingKey::AudioTheme));
    }

    #[test]
    fn test_audio_volume_is_always_visible() {
        let mut state = SettingsPageState::default();
        state.settings.pause_audio_enabled = true;
        assert!(state.visible_keys().contains(&SettingKey::AudioVolume));

        state.settings.pause_audio_enabled = false;
        assert!(state.visible_keys().contains(&SettingKey::AudioVolume));
    }

    #[test]
    fn test_voice_keep_loaded_toggles_and_needs_no_modal() {
        let mut state = SettingsPageState::default();
        let idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::VoiceKeepLoaded)
            .expect("VoiceKeepLoaded should be visible");
        state.selected = idx;

        assert_eq!(
            SettingKey::VoiceKeepLoaded.editor_kind(),
            EditorKind::Toggle
        );
        let interaction = state.toggle_selected_setting();
        let pending = interaction.pending_save().expect("toggle saves");
        assert_eq!(pending.key, SettingKey::VoiceKeepLoaded);
        assert_eq!(pending.value.as_deref(), Some("true"));

        state.open_editor_for_selected();
        assert!(state.modal.is_none());
    }

    #[test]
    fn test_pause_media_while_dictating_toggles_and_needs_no_modal() {
        let mut state = SettingsPageState::default();
        let idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::PauseMediaWhileDictating)
            .expect("PauseMediaWhileDictating should be visible");
        state.selected = idx;

        assert_eq!(
            SettingKey::PauseMediaWhileDictating.editor_kind(),
            EditorKind::Toggle
        );
        let interaction = state.toggle_selected_setting();
        let pending = interaction.pending_save().expect("toggle saves");
        assert_eq!(pending.key, SettingKey::PauseMediaWhileDictating);
        assert_eq!(pending.value.as_deref(), Some("false"));

        state.open_editor_for_selected();
        assert!(state.modal.is_none());
    }

    #[test]
    fn test_enter_toggles_boolean_setting() {
        let mut state = SettingsPageState::default();
        let idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::VoiceKeepLoaded)
            .expect("VoiceKeepLoaded should be visible");
        state.selected = idx;

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let pending = interaction.pending_save().expect("enter toggles");
        assert_eq!(pending.key, SettingKey::VoiceKeepLoaded);
        assert!(state.modal.is_none());
    }

    #[test]
    fn test_enter_opens_editor_for_non_toggle_setting() {
        let mut state = SettingsPageState::default();
        let idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::AudioTheme)
            .expect("AudioTheme should be visible");
        state.selected = idx;

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(interaction.pending_save().is_none());
        assert!(matches!(state.modal, Some(SettingsModal::Select(_))));
    }

    #[test]
    fn test_space_no_longer_toggles() {
        let mut state = SettingsPageState::default();
        let idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::VoiceKeepLoaded)
            .expect("VoiceKeepLoaded should be visible");
        state.selected = idx;

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(interaction.pending_save().is_none());
        assert!(state.modal.is_none());
    }

    fn type_query(state: &mut SettingsPageState, query: &str) {
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(state.is_search_active());
        for ch in query.chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
    }

    #[test]
    fn test_search_filters_by_display_name() {
        let mut state = SettingsPageState::default();
        type_query(&mut state, "audio");
        let keys = state.visible_keys();
        assert!(!keys.is_empty());
        assert!(
            keys.iter()
                .all(|k| k.display_name().to_ascii_lowercase().contains("audio"))
        );
        assert!(keys.contains(&SettingKey::AudioTheme));
    }

    #[test]
    fn test_search_matches_descriptions() {
        let mut state = SettingsPageState::default();
        type_query(&mut state, "extra ram");
        assert!(state.visible_keys().contains(&SettingKey::VoiceKeepLoaded));
    }

    #[test]
    fn test_search_keystroke_resets_selection() {
        let mut state = SettingsPageState {
            selected: 10,
            ..SettingsPageState::default()
        };
        type_query(&mut state, "audio");
        assert_eq!(state.selected_index(), 0);
    }

    #[test]
    fn test_search_enter_keeps_query_and_exits() {
        let mut state = SettingsPageState::default();
        type_query(&mut state, "audio");
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!state.is_search_active());
        assert_eq!(state.search_query(), "audio");
        assert!(
            state
                .visible_keys()
                .iter()
                .all(|k| k.display_name().to_ascii_lowercase().contains("audio"))
        );
    }

    #[test]
    fn test_search_esc_exits_and_keeps_query() {
        let mut state = SettingsPageState::default();
        type_query(&mut state, "audio");
        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!state.is_search_active());
        assert_eq!(state.search_query(), "audio");
    }

    #[test]
    fn test_empty_search_result_ignores_action_keys() {
        let mut state = SettingsPageState::default();
        type_query(&mut state, "zzz-no-such-setting");
        assert!(state.visible_keys().is_empty());
        for code in [KeyCode::Char('r'), KeyCode::Char('j')] {
            let interaction = state.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
            assert!(interaction.pending_save().is_none());
            assert!(state.modal.is_none());
        }
        assert!(state.is_search_active());
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!state.is_search_active());
        assert!(state.modal.is_none());
    }

    #[test]
    fn test_arrows_move_selection_while_searching() {
        let mut state = SettingsPageState::default();
        type_query(&mut state, "audio");
        assert!(state.visible_keys().len() > 1);

        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.selected_index(), 1);
        assert!(state.is_search_active());

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.selected_index(), 0);
        assert!(state.is_search_active());
    }

    #[test]
    fn test_click_selects_then_toggles() {
        let mut state = SettingsPageState::default();
        let idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::VoiceKeepLoaded)
            .expect("VoiceKeepLoaded should be visible");

        let interaction = state.click_setting(SettingKey::VoiceKeepLoaded, 0);
        assert!(interaction.pending_save().is_none());
        assert_eq!(state.selected_index(), idx);

        let interaction = state.click_setting(SettingKey::VoiceKeepLoaded, 0);
        let pending = interaction.pending_save().expect("second click toggles");
        assert_eq!(pending.key, SettingKey::VoiceKeepLoaded);
    }

    #[test]
    fn test_click_selected_non_toggle_opens_editor() {
        let mut state = SettingsPageState::default();
        let idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::AudioTheme)
            .expect("AudioTheme should be visible");
        state.selected = idx;

        let interaction = state.click_setting(SettingKey::AudioTheme, idx);
        assert!(interaction.pending_save().is_none());
        assert!(matches!(state.modal, Some(SettingsModal::Select(_))));
    }

    #[test]
    fn test_unbound_character_starts_search_immediately() {
        let mut state = SettingsPageState::default();
        assert!(!state.is_search_active());

        state.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

        assert!(state.is_search_active());
        assert_eq!(state.search_query(), "a");
    }

    #[test]
    fn test_reserved_keys_never_start_search() {
        for ch in ['1', '2', '3', 'q'] {
            let mut state = SettingsPageState::default();
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            assert!(!state.is_search_active());
            assert_eq!(state.search_query(), "");
        }
    }
}
