mod keys;
mod modals;

pub(crate) use keys::{EditorKind, SettingKey, SettingKeyMeta};
pub(crate) use modals::{ConfirmResetModalState, InputModalState, SelectModalState};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::{
    ai::supported_providers,
    settings::{Settings, apply_setting_input, reset_setting_to_default},
};

const SPINNER_STYLE_OPTIONS: [&str; 3] = ["classic", "braille", "arc"];
const AUDIO_THEME_OPTIONS: [&str; 12] = [
    "minimal",
    "soft",
    "glass",
    "arcade",
    "mechanical",
    "organic",
    "dreamy",
    "scifi",
    "rubber",
    "cinematic",
    "studio",
    "zen",
];

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SettingsPageState {
    pub(crate) settings: Settings,
    pub(crate) selected: usize,
    pub(crate) modal: Option<SettingsModal>,
    pub(crate) status_message: Option<String>,
    pub(crate) load_error: Option<String>,
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
        if self.settings.inline_ai_trigger_mode
            == taurine_core::settings::InlineAiTriggerMode::Symmetric
        {
            keys.retain(|k| {
                *k != SettingKey::InlineAiTriggerOpen && *k != SettingKey::InlineAiTriggerClose
            });
        } else {
            keys.retain(|k| *k != SettingKey::InlineAiTrigger);
        }

        if self.settings.rpc_mode == taurine_core::settings::RpcMode::Socket {
            keys.retain(|k| *k != SettingKey::RpcHost && *k != SettingKey::RpcPort);
        }

        if !self.settings.inline_datetime_enabled {
            keys.retain(|k| {
                *k != SettingKey::InlineDatetimeDateFormat
                    && *k != SettingKey::InlineDatetimeTimeFormat
                    && *k != SettingKey::InlineDatetimeDatetimeFormat
                    && *k != SettingKey::InlineDatetimeDialect
            });
        }
        keys
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

        self.status_message = None;

        match (key.code, key.modifiers) {
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_selection(1);
                SettingsInteraction::handled()
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_selection(-1);
                SettingsInteraction::handled()
            }
            (KeyCode::Char(' '), KeyModifiers::NONE) => self.toggle_selected_setting(),
            (KeyCode::Char('r'), KeyModifiers::NONE) => {
                self.modal = Some(SettingsModal::ConfirmReset(ConfirmResetModalState::new(
                    self.selected_key(),
                )));
                SettingsInteraction::handled()
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                self.open_editor_for_selected();
                SettingsInteraction::handled()
            }
            _ => SettingsInteraction::default(),
        }
    }

    pub(crate) fn footer_text(&self) -> &'static str {
        match self.modal.as_ref() {
            Some(SettingsModal::Select(_)) => "j/k Move   ↑/↓ Move   Enter Save   Esc Cancel",
            Some(SettingsModal::Input(_)) => "Type Edit   Enter Save   Esc Cancel",
            Some(SettingsModal::ConfirmReset(_)) => "←/h Yes   →/l No   y Confirm   n/Esc Cancel",
            None => "j/k Move   ↑/↓ Move   Space Toggle   Enter Edit   r Reset   q Quit",
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let max_index = self.visible_keys().len().saturating_sub(1) as isize;
        let next = (self.selected as isize + delta).clamp(0, max_index);
        self.selected = next as usize;
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
            EditorKind::InlineAiTriggerModeSelect => {
                Some(SettingsModal::Select(SelectModalState::new(
                    key,
                    vec!["symmetric".to_string(), "asymmetric".to_string()],
                    key.display_value(&self.settings),
                )))
            }
            EditorKind::InlineDictionaryModeSelect => {
                Some(SettingsModal::Select(SelectModalState::new(
                    key,
                    vec!["lite".to_string(), "full".to_string()],
                    key.display_value(&self.settings),
                )))
            }
            EditorKind::RpcModeSelect => Some(SettingsModal::Select(SelectModalState::new(
                key,
                vec!["socket".to_string(), "tcp".to_string()],
                key.display_value(&self.settings),
            ))),
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingsModal {
    Input(InputModalState),
    Select(SelectModalState),
    ConfirmReset(ConfirmResetModalState),
}

impl SettingsModal {
    fn set_error(&mut self, error: String) {
        match self {
            Self::Input(state) => state.error = Some(error),
            Self::Select(state) => state.error = Some(error),
            Self::ConfirmReset(state) => state.error = Some(error),
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
            assert_eq!(modal_state.options().len(), 12);
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
}
