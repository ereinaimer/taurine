use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::{
    ai::supported_providers,
    settings::{Settings, SpinnerStyle, apply_setting_input, reset_setting_to_default},
};

const SPINNER_STYLE_OPTIONS: [&str; 3] = ["classic", "braille", "arc"];
const ACTION_DELIMITER_OPTIONS: [&str; 2] = ["space", "enter"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingKey {
    TriggerChar,
    PauseHotkey,
    PauseNotificationsEnabled,
    PauseAudioEnabled,
    StartOnBoot,
    InlineTabCompletionEnabled,
    InlineHistoryEnabled,
    Wpm,
    SpinnerStyle,
    AiProvider,
    AiModel,
    AiCustomEndpoint,
    InlineAiDelimiter,
    ClipboardRestoreDelayMs,
    ActionDelimiter,
    TriggerlessMode,
    IgnoreFullscreen,
}

impl SettingKey {
    pub(crate) const ALL: [Self; 17] = [
        Self::TriggerChar,
        Self::PauseHotkey,
        Self::PauseNotificationsEnabled,
        Self::PauseAudioEnabled,
        Self::StartOnBoot,
        Self::InlineTabCompletionEnabled,
        Self::InlineHistoryEnabled,
        Self::Wpm,
        Self::SpinnerStyle,
        Self::AiProvider,
        Self::AiModel,
        Self::AiCustomEndpoint,
        Self::InlineAiDelimiter,
        Self::ClipboardRestoreDelayMs,
        Self::ActionDelimiter,
        Self::TriggerlessMode,
        Self::IgnoreFullscreen,
    ];

    pub(crate) const fn storage_key(self) -> &'static str {
        match self {
            Self::TriggerChar => "trigger_char",
            Self::PauseHotkey => "pause_hotkey",
            Self::PauseNotificationsEnabled => "pause_notifications_enabled",
            Self::PauseAudioEnabled => "pause_audio_enabled",
            Self::StartOnBoot => "start_on_boot",
            Self::InlineTabCompletionEnabled => "inline_tab_completion_enabled",
            Self::InlineHistoryEnabled => "inline_history_enabled",
            Self::Wpm => "wpm",
            Self::SpinnerStyle => "spinner_style",
            Self::AiProvider => "ai_provider",
            Self::AiModel => "ai_model",
            Self::AiCustomEndpoint => "ai_custom_endpoint",
            Self::InlineAiDelimiter => "inline_ai_delimiter",
            Self::ClipboardRestoreDelayMs => "clipboard_restore_delay_ms",
            Self::ActionDelimiter => "action_delimiter",
            Self::TriggerlessMode => "triggerless_mode",
            Self::IgnoreFullscreen => "ignore_fullscreen",
        }
    }

    pub(crate) const fn display_name(self) -> &'static str {
        match self {
            Self::TriggerChar => "Trigger Character",
            Self::PauseHotkey => "Pause Hotkey",
            Self::PauseNotificationsEnabled => "Pause Notifications",
            Self::PauseAudioEnabled => "Pause Audio",
            Self::StartOnBoot => "Start on Boot",
            Self::InlineTabCompletionEnabled => "Inline Tab Completion",
            Self::InlineHistoryEnabled => "Inline History",
            Self::Wpm => "Words Per Minute",
            Self::SpinnerStyle => "Spinner Style",
            Self::AiProvider => "AI Provider",
            Self::AiModel => "AI Model",
            Self::AiCustomEndpoint => "AI Custom Endpoint",
            Self::InlineAiDelimiter => "Inline AI Delimiter",
            Self::ClipboardRestoreDelayMs => "Clipboard Restore Delay (ms)",
            Self::ActionDelimiter => "Action Delimiter",
            Self::TriggerlessMode => "Triggerless Mode",
            Self::IgnoreFullscreen => "Ignore Fullscreen Apps",
        }
    }

    pub(crate) const fn description(self) -> &'static str {
        match self {
            Self::TriggerChar => "The character Taurine uses to start listening for trigger words",
            Self::PauseHotkey => "The keyboard shortcut used to pause Taurine globally",
            Self::PauseNotificationsEnabled => {
                "Show a notification when Taurine is paused or resumed"
            }
            Self::PauseAudioEnabled => "Play an audio cue when Taurine is paused or resumed",
            Self::StartOnBoot => "Start Taurine automatically when the system starts",
            Self::InlineTabCompletionEnabled => {
                "Use Tab and Shift+Tab to cycle trigger completions after the trigger character"
            }
            Self::InlineHistoryEnabled => {
                "Use Up and Down to navigate recently used triggers after the trigger character"
            }
            Self::Wpm => "Used to estimate time saved from keystrokes saved",
            Self::SpinnerStyle => "Animation style used while Taurine is processing",
            Self::AiProvider => "Default AI provider used for inline AI",
            Self::AiModel => "Default AI model used for inline AI",
            Self::AiCustomEndpoint => "Optional custom API endpoint for AI requests",
            Self::InlineAiDelimiter => "Delimiter used by inline AI capture mode",
            Self::ClipboardRestoreDelayMs => {
                "The delay in milliseconds between pasting and restoring the clipboard"
            }
            Self::ActionDelimiter => {
                "The keystroke used to trigger a text expansion after the trigger character"
            }
            Self::TriggerlessMode => {
                "Expand trigger words automatically when typing without requiring the trigger character"
            }
            Self::IgnoreFullscreen => {
                "Pause macro evaluation when running a full-screen application (e.g. games)"
            }
        }
    }

    pub(crate) const fn editor_kind(self) -> EditorKind {
        match self {
            Self::PauseNotificationsEnabled
            | Self::PauseAudioEnabled
            | Self::StartOnBoot
            | Self::InlineTabCompletionEnabled
            | Self::InlineHistoryEnabled
            | Self::TriggerlessMode
            | Self::IgnoreFullscreen => EditorKind::Toggle,
            Self::Wpm | Self::ClipboardRestoreDelayMs => EditorKind::NumberInput,
            Self::SpinnerStyle => EditorKind::SpinnerSelect,
            Self::ActionDelimiter => EditorKind::ActionDelimiterSelect,
            Self::AiProvider => EditorKind::AiProviderSelect,
            Self::AiCustomEndpoint => EditorKind::OptionalTextInput,
            Self::TriggerChar | Self::InlineAiDelimiter => EditorKind::SingleCharInput,
            Self::PauseHotkey | Self::AiModel => EditorKind::TextInput,
        }
    }

    pub(crate) fn display_value(self, settings: &Settings) -> String {
        match self {
            Self::TriggerChar => settings.trigger_char.to_string(),
            Self::PauseHotkey => settings.pause_hotkey.clone(),
            Self::PauseNotificationsEnabled => settings.pause_notifications_enabled.to_string(),
            Self::PauseAudioEnabled => settings.pause_audio_enabled.to_string(),
            Self::StartOnBoot => settings.start_on_boot.to_string(),
            Self::InlineTabCompletionEnabled => settings.inline_tab_completion_enabled.to_string(),
            Self::InlineHistoryEnabled => settings.inline_history_enabled.to_string(),
            Self::Wpm => settings.wpm.to_string(),
            Self::SpinnerStyle => spinner_style_label(settings.spinner_style).to_string(),
            Self::AiProvider => optional_value_label(settings.ai_provider.as_deref()).to_string(),
            Self::AiModel => optional_value_label(settings.ai_model.as_deref()).to_string(),
            Self::AiCustomEndpoint => {
                optional_value_label(settings.ai_custom_endpoint.as_deref()).to_string()
            }
            Self::InlineAiDelimiter => settings.inline_ai_delimiter.to_string(),
            Self::ClipboardRestoreDelayMs => settings.clipboard_restore_delay_ms.to_string(),
            Self::ActionDelimiter => format!("{:?}", settings.action_delimiter).to_lowercase(),
            Self::TriggerlessMode => settings.triggerless_mode.to_string(),
            Self::IgnoreFullscreen => settings.ignore_fullscreen.to_string(),
        }
    }

    fn edit_value(self, settings: &Settings) -> String {
        match self {
            Self::TriggerChar => settings.trigger_char.to_string(),
            Self::PauseHotkey => settings.pause_hotkey.clone(),
            Self::Wpm => settings.wpm.to_string(),
            Self::AiProvider => settings.ai_provider.clone().unwrap_or_default(),
            Self::AiModel => settings.ai_model.clone().unwrap_or_default(),
            Self::AiCustomEndpoint => settings.ai_custom_endpoint.clone().unwrap_or_default(),
            Self::InlineAiDelimiter => settings.inline_ai_delimiter.to_string(),
            Self::PauseNotificationsEnabled
            | Self::PauseAudioEnabled
            | Self::StartOnBoot
            | Self::InlineTabCompletionEnabled
            | Self::InlineHistoryEnabled
            | Self::TriggerlessMode
            | Self::IgnoreFullscreen
            | Self::SpinnerStyle
            | Self::ActionDelimiter
            | Self::ClipboardRestoreDelayMs => self.display_value(settings),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorKind {
    Toggle,
    SingleCharInput,
    TextInput,
    OptionalTextInput,
    NumberInput,
    SpinnerSelect,
    ActionDelimiterSelect,
    AiProviderSelect,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SettingsPageState {
    settings: Settings,
    selected: usize,
    modal: Option<SettingsModal>,
    status_message: Option<String>,
    load_error: Option<String>,
}

impl SettingsPageState {
    pub(crate) const fn settings(&self) -> &Settings {
        &self.settings
    }

    pub(crate) const fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn selected_key(&self) -> SettingKey {
        SettingKey::ALL[self.selected]
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
        let max_index = SettingKey::ALL.len().saturating_sub(1) as isize;
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
            SettingKey::InlineTabCompletionEnabled => {
                (!self.settings.inline_tab_completion_enabled).to_string()
            }
            SettingKey::InlineHistoryEnabled => (!self.settings.inline_history_enabled).to_string(),
            SettingKey::TriggerlessMode => (!self.settings.triggerless_mode).to_string(),
            SettingKey::IgnoreFullscreen => (!self.settings.ignore_fullscreen).to_string(),
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
            EditorKind::ActionDelimiterSelect => {
                Some(SettingsModal::Select(SelectModalState::new(
                    key,
                    ACTION_DELIMITER_OPTIONS
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                    key.display_value(&self.settings),
                )))
            }
            EditorKind::AiProviderSelect => Some(SettingsModal::Select(SelectModalState::new(
                key,
                supported_providers()
                    .iter()
                    .map(|provider| provider.as_str().to_string())
                    .collect(),
                self.settings.ai_provider.clone().unwrap_or_default(),
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
pub(crate) struct ConfirmResetModalState {
    key: SettingKey,
    default_display_value: String,
    selected_yes: bool,
    error: Option<String>,
}

impl ConfirmResetModalState {
    fn new(key: SettingKey) -> Self {
        let defaults = Settings::default();
        Self {
            key,
            default_display_value: key.display_value(&defaults),
            selected_yes: true,
            error: None,
        }
    }

    pub(crate) const fn key(&self) -> SettingKey {
        self.key
    }

    pub(crate) fn default_display_value(&self) -> &str {
        &self.default_display_value
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) const fn selected_yes(&self) -> bool {
        self.selected_yes
    }

    fn handle_key(&mut self, key: KeyEvent) -> SettingsInteraction {
        self.error = None;

        match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                self.selected_yes = true;
                SettingsInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
                self.selected_yes = false;
                SettingsInteraction::handled()
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if self.selected_yes {
                    SettingsInteraction::reset(self.key)
                } else {
                    SettingsInteraction::cancel()
                }
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) | (KeyCode::Char('Y'), KeyModifiers::NONE) => {
                SettingsInteraction::reset(self.key)
            }
            (KeyCode::Char('n'), KeyModifiers::NONE)
            | (KeyCode::Char('N'), KeyModifiers::NONE)
            | (KeyCode::Esc, KeyModifiers::NONE) => SettingsInteraction::cancel(),
            _ => SettingsInteraction::handled(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputModalState {
    key: SettingKey,
    value: String,
    cursor: usize,
    error: Option<String>,
}

impl InputModalState {
    fn new(key: SettingKey, value: String) -> Self {
        let cursor = value.chars().count();
        Self {
            key,
            value,
            cursor,
            error: None,
        }
    }

    pub(crate) const fn key(&self) -> SettingKey {
        self.key
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn handle_key(&mut self, key: KeyEvent) -> SettingsInteraction {
        self.error = None;

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => SettingsInteraction::cancel(),
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if self.key.editor_kind() == EditorKind::SingleCharInput
                    && self.value.chars().count() != 1
                {
                    self.error = Some(format!(
                        "{} must be exactly one character.",
                        self.key.display_name()
                    ));
                    return SettingsInteraction::handled();
                }
                let value = match self.key.editor_kind() {
                    EditorKind::OptionalTextInput if self.value.trim().is_empty() => None,
                    _ => Some(self.value.clone()),
                };
                SettingsInteraction::save(self.key, value)
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.backspace();
                SettingsInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.cursor = self.cursor.saturating_sub(1);
                SettingsInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.cursor = (self.cursor + 1).min(self.value.chars().count());
                SettingsInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.cursor = 0;
                SettingsInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.cursor = self.value.chars().count();
                SettingsInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_char(ch);
                SettingsInteraction::handled()
            }
            _ => SettingsInteraction::handled(),
        }
    }

    fn insert_char(&mut self, ch: char) {
        let byte_index = char_index_to_byte_index(&self.value, self.cursor);
        self.value.insert(byte_index, ch);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let end = char_index_to_byte_index(&self.value, self.cursor);
        let start = char_index_to_byte_index(&self.value, self.cursor - 1);
        self.value.drain(start..end);
        self.cursor -= 1;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectModalState {
    key: SettingKey,
    options: Vec<String>,
    selected: usize,
    error: Option<String>,
}

impl SelectModalState {
    fn new(key: SettingKey, options: Vec<String>, current_value: String) -> Self {
        let selected = options
            .iter()
            .position(|option| option.eq_ignore_ascii_case(&current_value))
            .unwrap_or_default();

        Self {
            key,
            options,
            selected,
            error: None,
        }
    }

    pub(crate) const fn key(&self) -> SettingKey {
        self.key
    }

    pub(crate) fn options(&self) -> &[String] {
        &self.options
    }

    pub(crate) const fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn handle_key(&mut self, key: KeyEvent) -> SettingsInteraction {
        self.error = None;

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => SettingsInteraction::cancel(),
            (KeyCode::Enter, KeyModifiers::NONE) => {
                SettingsInteraction::save(self.key, Some(self.options[self.selected].clone()))
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                self.selected = (self.selected + 1).min(self.options.len().saturating_sub(1));
                SettingsInteraction::handled()
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                self.selected = self.selected.saturating_sub(1);
                SettingsInteraction::handled()
            }
            _ => SettingsInteraction::handled(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSettingSave {
    key: SettingKey,
    value: Option<String>,
}

impl PendingSettingSave {
    pub(crate) fn apply(&self) -> taurine_core::Result<()> {
        apply_setting_input(self.key.storage_key(), self.value.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSettingReset {
    key: SettingKey,
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
pub(crate) fn spinner_style_options() -> &'static [&'static str; 3] {
    &SPINNER_STYLE_OPTIONS
}

pub(crate) const fn spinner_style_label(style: SpinnerStyle) -> &'static str {
    match style {
        SpinnerStyle::Classic => "classic",
        SpinnerStyle::Braille => "braille",
        SpinnerStyle::Arc => "arc",
    }
}

pub(crate) fn optional_value_label(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("<unset>")
}

fn char_index_to_byte_index(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(value.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_has_a_descriptor() {
        assert_eq!(SettingKey::ALL.len(), 17);
    }

    #[test]
    fn descriptor_names_are_human_readable() {
        for key in SettingKey::ALL {
            assert_ne!(key.display_name(), key.storage_key());
        }
    }

    #[test]
    fn spinner_style_options_are_exact() {
        assert_eq!(spinner_style_options(), &["classic", "braille", "arc"]);
    }

    #[test]
    fn boolean_settings_are_toggles() {
        assert_eq!(
            SettingKey::PauseNotificationsEnabled.editor_kind(),
            EditorKind::Toggle
        );
        assert_eq!(
            SettingKey::PauseAudioEnabled.editor_kind(),
            EditorKind::Toggle
        );
        assert_eq!(SettingKey::StartOnBoot.editor_kind(), EditorKind::Toggle);
        assert_eq!(
            SettingKey::InlineTabCompletionEnabled.editor_kind(),
            EditorKind::Toggle
        );
        assert_eq!(
            SettingKey::InlineHistoryEnabled.editor_kind(),
            EditorKind::Toggle
        );
    }

    #[test]
    fn wpm_is_number_input_and_ai_model_is_text_input() {
        assert_eq!(SettingKey::Wpm.editor_kind(), EditorKind::NumberInput);
        assert_eq!(
            SettingKey::ClipboardRestoreDelayMs.editor_kind(),
            EditorKind::NumberInput
        );
        assert_eq!(SettingKey::AiModel.editor_kind(), EditorKind::TextInput);
    }

    #[test]
    fn trigger_char_and_inline_ai_delimiter_are_single_char_inputs() {
        assert_eq!(
            SettingKey::TriggerChar.editor_kind(),
            EditorKind::SingleCharInput
        );
        assert_eq!(
            SettingKey::InlineAiDelimiter.editor_kind(),
            EditorKind::SingleCharInput
        );
    }

    #[test]
    fn unset_custom_endpoint_uses_placeholder() {
        assert_eq!(
            SettingKey::AiCustomEndpoint.display_value(&Settings::default()),
            "<unset>"
        );
    }

    #[test]
    fn pressing_j_moves_selection_down() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(state.selected_index(), 1);
    }

    #[test]
    fn pressing_down_moves_selection_down() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.selected_index(), 1);
    }

    #[test]
    fn pressing_k_moves_selection_up_without_underflow() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(state.selected_index(), 0);
    }

    #[test]
    fn pressing_up_moves_selection_up() {
        let mut state = SettingsPageState {
            selected: 2,
            ..Default::default()
        };
        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.selected_index(), 1);
    }

    #[test]
    fn space_toggles_boolean_setting() {
        let selected = SettingKey::ALL
            .iter()
            .position(|&k| k == SettingKey::PauseNotificationsEnabled)
            .unwrap();
        let mut state = SettingsPageState {
            selected,
            ..Default::default()
        };
        let interaction = state.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        let pending = interaction.pending_save().unwrap();
        assert_eq!(pending.key, SettingKey::PauseNotificationsEnabled);
        assert_eq!(pending.value.as_deref(), Some("false"));
    }

    #[test]
    fn space_on_non_toggle_does_not_mutate_setting() {
        let mut state = SettingsPageState::default();
        let interaction = state.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(interaction.pending_save().is_none());
        assert!(!state.is_modal_open());
    }

    #[test]
    fn enter_on_spinner_style_opens_select_modal() {
        let selected = SettingKey::ALL
            .iter()
            .position(|&k| k == SettingKey::SpinnerStyle)
            .unwrap();
        let mut state = SettingsPageState {
            selected,
            ..Default::default()
        };
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(state.modal(), Some(SettingsModal::Select(_))));
    }

    #[test]
    fn enter_on_text_setting_opens_input_modal() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(state.modal(), Some(SettingsModal::Input(_))));
    }

    #[test]
    fn escape_cancels_modal_without_save() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(interaction.should_close_modal());
        assert!(interaction.pending_save().is_none());
    }

    #[test]
    fn single_char_input_rejects_multi_character_values() {
        let mut modal = InputModalState::new(SettingKey::TriggerChar, "ab".to_string());

        let interaction = modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(interaction.pending_save().is_none());
        assert_eq!(
            modal.error(),
            Some("Trigger Character must be exactly one character.")
        );
    }

    #[test]
    fn pressing_r_creates_reset_action_for_selected_setting() {
        let selected = SettingKey::ALL
            .iter()
            .position(|&k| k == SettingKey::InlineTabCompletionEnabled)
            .unwrap();
        let mut state = SettingsPageState {
            selected,
            ..Default::default()
        };
        let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        assert!(interaction.pending_save().is_none());
        assert!(interaction.pending_reset().is_none());
        assert!(matches!(
            state.modal(),
            Some(SettingsModal::ConfirmReset(_))
        ));
    }

    #[test]
    fn settings_footer_includes_reset_hint() {
        assert!(
            SettingsPageState::default()
                .footer_text()
                .contains("r Reset")
        );
    }

    #[test]
    fn reset_confirmation_footer_matches_confirmation_keys() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        assert_eq!(
            state.footer_text(),
            "←/h Yes   →/l No   y Confirm   n/Esc Cancel"
        );
    }

    #[test]
    fn pressing_n_cancels_reset_confirmation() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

        assert!(interaction.should_close_modal());
        assert!(interaction.pending_reset().is_none());
    }

    #[test]
    fn pressing_escape_cancels_reset_confirmation() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(interaction.should_close_modal());
        assert!(interaction.pending_reset().is_none());
    }

    #[test]
    fn pressing_y_confirms_reset_after_modal_opens() {
        let selected = SettingKey::ALL
            .iter()
            .position(|&k| k == SettingKey::InlineTabCompletionEnabled)
            .unwrap();
        let mut state = SettingsPageState {
            selected,
            ..Default::default()
        };
        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));

        assert_eq!(
            interaction.pending_reset().map(|reset| reset.key),
            Some(SettingKey::InlineTabCompletionEnabled)
        );
    }

    #[test]
    fn unrelated_keys_are_ignored_while_reset_modal_is_open() {
        let selected = SettingKey::ALL
            .iter()
            .position(|&k| k == SettingKey::PauseAudioEnabled)
            .unwrap();
        let mut state = SettingsPageState {
            selected,
            ..Default::default()
        };
        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

        assert_eq!(state.selected_index(), selected);
        assert!(interaction.pending_reset().is_none());
        assert!(!interaction.should_close_modal());
        assert!(matches!(
            state.modal(),
            Some(SettingsModal::ConfirmReset(_))
        ));
    }

    #[test]
    fn reset_modal_uses_selected_setting_and_default_value() {
        let selected = SettingKey::ALL
            .iter()
            .position(|&k| k == SettingKey::AiCustomEndpoint)
            .unwrap();
        let mut state = SettingsPageState {
            selected,
            ..Default::default()
        };
        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        let Some(SettingsModal::ConfirmReset(modal)) = state.modal() else {
            panic!("expected reset confirmation modal");
        };

        assert_eq!(modal.key(), SettingKey::AiCustomEndpoint);
        assert_eq!(modal.default_display_value(), "<unset>");
        assert!(modal.selected_yes());
    }

    #[test]
    fn pressing_right_selects_no_in_reset_modal() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

        state.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

        let Some(SettingsModal::ConfirmReset(modal)) = state.modal() else {
            panic!("expected reset confirmation modal");
        };
        assert!(!modal.selected_yes());
    }

    #[test]
    fn pressing_enter_on_no_cancels_reset() {
        let mut state = SettingsPageState::default();
        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)); // Select No

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(interaction.should_close_modal());
        assert!(interaction.pending_reset().is_none());
    }
}
