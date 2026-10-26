mod hotkey_capture;
mod keys;
mod modals;

pub(crate) use hotkey_capture::HotkeyCaptureModalState;
#[cfg(test)]
pub(crate) use keys::cursor_style_label;
pub(crate) use keys::{
    EditorKind, SettingKey, SettingKeyMeta, cursor_set_cursor_style, voice_model_tier,
};
pub(crate) use modals::{ConfirmResetModalState, InputModalState, SelectChoice, SelectModalState};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::{
    ai::supported_providers,
    settings::{Settings, apply_setting_input, reset_setting_to_default},
};

const SPINNER_STYLE_OPTIONS: [&str; 3] = ["classic", "braille", "arc"];
const CURSOR_STYLE_OPTIONS: [&str; 3] = ["block", "bar", "underscore"];
const AUDIO_THEME_OPTIONS: [&str; 7] = [
    "minimal",
    "arcade",
    "mechanical",
    "organic",
    "scifi",
    "rubber",
    "zen",
];

/// Voice model tiers, ordered best quality to lightest footprint.
const VOICE_MODEL_OPTIONS: [(&str, &str, &str); 3] = [
    (
        "Best",
        "Highest accuracy; slower and uses the most RAM",
        "best",
    ),
    (
        "Balanced",
        "Great accuracy with faster dictation and moderate RAM",
        "balanced",
    ),
    (
        "Fast",
        "Quickest and lightest; best for low-RAM machines",
        "fast",
    ),
];

fn voice_model_choices() -> Vec<SelectChoice> {
    VOICE_MODEL_OPTIONS
        .iter()
        .map(|(label, detail, value)| SelectChoice {
            label: (*label).to_string(),
            detail: (*detail).to_string(),
            value: (*value).to_string(),
        })
        .collect()
}

/// Choices for the microphone picker: the system default plus every live
/// input device reported by the shared tray enumeration.
fn voice_device_choices(devices: Vec<String>) -> Vec<SelectChoice> {
    let mut choices = vec![SelectChoice {
        label: "System Default".to_string(),
        detail: "Use the microphone the system selects".to_string(),
        value: String::new(),
    }];
    choices.extend(devices.into_iter().map(|name| SelectChoice {
        label: name.clone(),
        detail: String::new(),
        value: name,
    }));
    choices
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SettingsPageState {
    pub(crate) settings: Settings,
    pub(crate) selected: usize,
    pub(crate) modal: Option<SettingsModal>,
    pub(crate) status_message: Option<String>,
    pub(crate) load_error: Option<String>,
    pub(crate) search: crate::widgets::field::TextField,
    pub(crate) search_active: bool,
    pub(crate) window_anchor: Option<usize>,
    pub(crate) inline: Option<InputModalState>,
}

impl SettingsPageState {
    pub(crate) const fn settings(&self) -> &Settings {
        &self.settings
    }

    pub(crate) fn setting_toggle_value(&self, key: SettingKey) -> Option<bool> {
        key.toggle_value(&self.settings)
    }

    pub(crate) const fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn select_last(&mut self) {
        self.selected = self.visible_keys().len().saturating_sub(1);
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
        let needle = self.search_query().trim().to_ascii_lowercase();
        if !needle.is_empty() {
            keys.retain(|k| {
                k.display_name().to_ascii_lowercase().contains(&needle)
                    || k.description().to_ascii_lowercase().contains(&needle)
            });
        }
        keys
    }

    pub(crate) fn search_query(&self) -> &str {
        self.search.text()
    }

    pub(crate) fn search_field(&self) -> &crate::widgets::field::TextField {
        &self.search
    }

    pub(crate) const fn is_search_active(&self) -> bool {
        self.search_active
    }

    pub(crate) fn activate_search_at(&mut self, cursor: usize) {
        self.search_active = true;
        self.search.place(cursor);
    }

    /// Clicking a boolean row or its switch flips immediately; other
    /// rows select first and open their editor on the next click. The
    /// click-time window start keeps the list from jumping.
    pub(crate) fn click_setting(&mut self, key: SettingKey, anchor: usize) -> SettingsInteraction {
        let position = self
            .visible_keys()
            .iter()
            .position(|k| *k == key)
            .unwrap_or(self.selected);
        let was_selected = self.selected_key() == key;
        self.selected = position;
        self.window_anchor = Some(anchor);
        if key.editor_kind() == EditorKind::Toggle {
            return self.toggle_selected_setting();
        }
        if was_selected {
            if key.inline_edit() {
                self.begin_inline_edit(key);
            } else {
                self.open_editor_for_selected();
            }
        }
        SettingsInteraction::handled()
    }

    /// Start editing a short value in the row itself. The click-time
    /// anchor keeps the clicked row in place; keyboard entry passes
    /// `None` so the window follows the selection instead.
    pub(crate) fn start_inline_edit(&mut self, key: SettingKey, anchor: Option<usize>) {
        if !key.inline_edit() {
            return;
        }
        if let Some(anchor) = anchor {
            self.select_key(key, anchor);
        } else {
            if let Some(position) = self.visible_keys().iter().position(|k| *k == key) {
                self.selected = position;
            }
            self.window_anchor = None;
        }
        self.begin_inline_edit(key);
    }

    /// Select a value row and open its picker from the same click.
    pub(crate) fn open_editor_for_key(&mut self, key: SettingKey, anchor: usize) {
        self.select_key(key, anchor);
        self.open_editor_for_selected();
    }

    pub(crate) fn clear_inline_edit(&mut self) {
        self.inline = None;
        self.status_message = None;
    }

    pub(crate) const fn inline_edit(&self) -> Option<&InputModalState> {
        self.inline.as_ref()
    }

    pub(crate) fn inline_key(&self) -> Option<SettingKey> {
        self.inline.as_ref().map(InputModalState::key)
    }

    fn begin_inline_edit(&mut self, key: SettingKey) {
        self.search_active = false;
        self.status_message = None;
        self.inline = Some(InputModalState::new(key, key.edit_value(&self.settings)));
    }

    pub(crate) fn selected_key(&self) -> SettingKey {
        let keys = self.visible_keys();
        keys[self.selected.min(keys.len().saturating_sub(1))]
    }

    /// Select `key` without opening an editor, anchoring the visible
    /// window so the clicked row keeps its place.
    pub(crate) fn select_key(&mut self, key: SettingKey, anchor: usize) {
        if let Some(position) = self.visible_keys().iter().position(|k| *k == key) {
            self.selected = position;
        }
        self.window_anchor = Some(anchor);
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
        self.inline = None;
        self.load_error = None;
        self.status_message = None;
        // honey: a save refreshes settings from disk; keep the click-time
        // window anchor so toggling a row cannot re-anchor the list on
        // the search bar and move the next click to another row.
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

        if let Some(inline) = self.inline.as_mut() {
            let interaction = inline.handle_key(key);
            let error = inline.error().map(str::to_string);
            self.status_message = error;
            return interaction;
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
                let key = self.selected_key();
                if key.editor_kind() == EditorKind::Toggle {
                    self.toggle_selected_setting()
                } else if key.inline_edit() {
                    self.begin_inline_edit(key);
                    SettingsInteraction::handled()
                } else {
                    self.open_editor_for_selected();
                    SettingsInteraction::handled()
                }
            }
            // honey: navigation and quit keys never start a search; any other
            // bare character filters the list immediately (type-to-search).
            (KeyCode::Char('1' | '2' | 'q'), KeyModifiers::NONE) => SettingsInteraction::default(),
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_active = true;
                self.search.insert(ch);
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
            (KeyCode::Left, KeyModifiers::NONE) => self.search.move_left(),
            (KeyCode::Right, KeyModifiers::NONE) => self.search.move_right(),
            (KeyCode::Home, KeyModifiers::NONE) => self.search.move_home(),
            (KeyCode::End, KeyModifiers::NONE) => self.search.move_end(),
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                if self.search.backspace() {
                    self.selected = 0;
                    self.window_anchor = None;
                }
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                if self.search.delete_at() {
                    self.selected = 0;
                    self.window_anchor = None;
                }
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search.insert(ch);
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
        let Some(value) = key.toggle_value(&self.settings) else {
            return SettingsInteraction::handled();
        };
        SettingsInteraction::save(key, Some((!value).to_string()))
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
            EditorKind::CursorStyleSelect => Some(SettingsModal::Select(SelectModalState::new(
                key,
                CURSOR_STYLE_OPTIONS
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
            EditorKind::VoiceModelSelect => {
                Some(SettingsModal::Select(SelectModalState::new_choices(
                    key,
                    voice_model_choices(),
                    voice_model_tier(&self.settings.voice_model),
                )))
            }
            EditorKind::VoiceInputDeviceSelect => {
                let current = self
                    .settings
                    .voice_input_device
                    .as_deref()
                    .filter(|device| !device.trim().is_empty())
                    .unwrap_or("");
                // honey: query on open, not from a cache, so newly plugged
                // microphones appear exactly like the tray menu.
                let devices = taurine_core::voice::devices::list_input_devices();
                Some(SettingsModal::Select(SelectModalState::new_choices(
                    key,
                    voice_device_choices(devices),
                    current,
                )))
            }
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

/// Discriminant for `SettingsModal`; see `SettingsModal::kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsModalKind {
    Input,
    Select,
    ConfirmReset,
    HotkeyCapture,
}

impl SettingsModal {
    /// Discriminant for overlay bookkeeping (layer tracking, open
    /// transitions): cursor moves and typed text never change it.
    pub(crate) const fn kind(&self) -> SettingsModalKind {
        match self {
            Self::Input(_) => SettingsModalKind::Input,
            Self::Select(_) => SettingsModalKind::Select,
            Self::ConfirmReset(_) => SettingsModalKind::ConfirmReset,
            Self::HotkeyCapture(_) => SettingsModalKind::HotkeyCapture,
        }
    }

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
    fn replacing_settings_keeps_the_click_anchor() {
        let mut state = SettingsPageState {
            window_anchor: Some(4),
            ..SettingsPageState::default()
        };

        state.replace_settings(Settings::default());

        assert_eq!(state.window_anchor, Some(4));
    }

    #[test]
    fn enter_starts_editing_a_short_value_in_place() {
        let mut state = SettingsPageState::default();
        state.selected = state
            .visible_keys()
            .iter()
            .position(|key| *key == SettingKey::ClipboardHistoryRetentionSecs)
            .expect("visible inline key");

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(state.modal().is_none());
        assert_eq!(
            state.inline_key(),
            Some(SettingKey::ClipboardHistoryRetentionSecs)
        );
        assert!(!interaction.should_close_modal());
        assert!(interaction.pending_save().is_none());
    }

    #[test]
    fn inline_single_char_validation_reports_in_the_status_line() {
        let mut state = SettingsPageState::default();
        state.selected = state
            .visible_keys()
            .iter()
            .position(|key| *key == SettingKey::InlineEmojiTriggerChar)
            .expect("visible inline key");
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        for ch in ['a', 'b'] {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(state.inline_edit().is_some());
        assert!(
            state
                .status_message()
                .is_some_and(|message| message.contains("exactly one character")),
            "{:?}",
            state.status_message()
        );
    }

    #[test]
    fn voice_model_picker_offers_tiers_with_descriptions() {
        let choices = voice_model_choices();
        let labels: Vec<&str> = choices.iter().map(|choice| choice.label.as_str()).collect();
        let values: Vec<&str> = choices.iter().map(|choice| choice.value.as_str()).collect();
        assert_eq!(labels, ["Best", "Balanced", "Fast"]);
        assert_eq!(values, ["best", "balanced", "fast"]);
        assert!(choices.iter().all(|choice| !choice.detail.is_empty()));
    }

    #[test]
    fn voice_device_picker_keeps_system_default_first() {
        let choices = voice_device_choices(vec!["Mic A".to_string(), "Mic B".to_string()]);
        let labels: Vec<&str> = choices.iter().map(|choice| choice.label.as_str()).collect();
        let values: Vec<&str> = choices.iter().map(|choice| choice.value.as_str()).collect();
        assert_eq!(labels, ["System Default", "Mic A", "Mic B"]);
        assert_eq!(values, ["", "Mic A", "Mic B"]);
    }

    #[test]
    fn voice_model_picker_saves_the_tier_value_not_the_label() {
        let mut picker = SelectModalState::new_choices(
            SettingKey::VoiceModel,
            voice_model_choices(),
            "balanced",
        );
        let interaction = picker.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            interaction
                .pending_save()
                .map(|pending| pending.value.clone()),
            Some(Some("balanced".to_string()))
        );
    }

    #[test]
    fn voice_model_opens_the_tier_picker() {
        let mut state = SettingsPageState::default();
        state.selected = state
            .visible_keys()
            .iter()
            .position(|key| *key == SettingKey::VoiceModel)
            .expect("visible voice model");
        state.open_editor_for_selected();

        let Some(SettingsModal::Select(modal)) = state.modal else {
            panic!("voice model should open a select modal");
        };
        assert_eq!(modal.key(), SettingKey::VoiceModel);
        assert_eq!(modal.options(), ["Best", "Balanced", "Fast"]);
    }

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
    fn test_cursor_style_select_modal_opens_with_bar_default() {
        let mut state = SettingsPageState::default();
        let idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::TuiCursorStyle)
            .expect("TuiCursorStyle should be visible");
        state.selected = idx;

        assert_eq!(
            SettingKey::TuiCursorStyle.editor_kind(),
            EditorKind::CursorStyleSelect
        );
        state.open_editor_for_selected();

        assert!(matches!(state.modal, Some(SettingsModal::Select(_))));
        if let Some(SettingsModal::Select(modal_state)) = state.modal {
            assert_eq!(modal_state.options().len(), 3);
            assert_eq!(modal_state.options()[modal_state.selected_index()], "bar");
        }
    }

    #[test]
    fn test_cursor_style_labels_and_terminal_mapping() {
        use taurine_core::settings::TuiCursorStyle;

        assert_eq!(cursor_style_label(TuiCursorStyle::Block), "block");
        assert_eq!(cursor_style_label(TuiCursorStyle::Bar), "bar");
        assert_eq!(cursor_style_label(TuiCursorStyle::Underscore), "underscore");
        assert_eq!(
            cursor_set_cursor_style(TuiCursorStyle::Block),
            crossterm::cursor::SetCursorStyle::SteadyBlock
        );
        assert_eq!(
            cursor_set_cursor_style(TuiCursorStyle::Bar),
            crossterm::cursor::SetCursorStyle::SteadyBar
        );
        assert_eq!(
            cursor_set_cursor_style(TuiCursorStyle::Underscore),
            crossterm::cursor::SetCursorStyle::SteadyUnderScore
        );
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
    fn test_search_caret_edits_mid_text() {
        let mut state = SettingsPageState::default();
        type_query(&mut state, "audoi");
        state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(state.search_query(), "audi");
        state.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(state.search_query(), "udi");
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
    fn every_toggle_editor_has_a_boolean_value() {
        let settings = Settings::default();
        for key in SettingKey::ALL {
            assert_eq!(
                key.editor_kind() == EditorKind::Toggle,
                key.toggle_value(&settings).is_some(),
                "{}",
                key.display_name()
            );
        }
    }

    #[test]
    fn test_click_toggles_boolean_immediately() {
        let mut state = SettingsPageState::default();
        let idx = state
            .visible_keys()
            .iter()
            .position(|k| *k == SettingKey::VoiceKeepLoaded)
            .expect("VoiceKeepLoaded should be visible");

        let interaction = state.click_setting(SettingKey::VoiceKeepLoaded, 0);
        let pending = interaction.pending_save().expect("first click toggles");
        assert_eq!(pending.key, SettingKey::VoiceKeepLoaded);
        assert_eq!(state.selected_index(), idx);

        let interaction = state.click_setting(SettingKey::VoiceKeepLoaded, 0);
        let pending = interaction
            .pending_save()
            .expect("second click toggles back");
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
        for ch in ['1', '2', 'q'] {
            let mut state = SettingsPageState::default();
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            assert!(!state.is_search_active());
            assert_eq!(state.search_query(), "");
        }
    }

    #[test]
    fn test_freed_digit_starts_search() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert!(state.is_search_active());
        assert_eq!(state.search_query(), "3");
    }

    #[test]
    fn test_select_last_targets_final_key() {
        let mut state = SettingsPageState::default();
        state.select_last();
        assert_eq!(
            state.selected_index(),
            state.visible_keys().len().saturating_sub(1)
        );
        assert_eq!(state.selected_key(), SettingKey::TuiCursorStyle);
    }

    #[test]
    fn test_select_last_on_empty_results_stays_zero() {
        let mut state = SettingsPageState {
            search: crate::widgets::field::TextField::new("zzz-no-such-setting"),
            ..SettingsPageState::default()
        };
        assert!(state.visible_keys().is_empty());
        state.select_last();
        assert_eq!(state.selected_index(), 0);
    }

    #[test]
    fn test_up_from_last_moves_up_the_stack() {
        let mut state = SettingsPageState::default();
        state.select_last();
        let last = state.selected_index();
        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.selected_index(), last.saturating_sub(1));
    }
}
