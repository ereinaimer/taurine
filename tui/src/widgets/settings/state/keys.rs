use taurine_core::settings::{Settings, TuiCursorStyle};

pub(crate) use taurine_core::settings::SettingKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingCategory {
    General,
    Formatting,
    AudioAndFeedback,
    AiCopilot,
    VoiceDictation,
    Appearance,
}

impl SettingCategory {
    pub const ALL: [Self; 6] = [
        Self::General,
        Self::Formatting,
        Self::AudioAndFeedback,
        Self::AiCopilot,
        Self::VoiceDictation,
        Self::Appearance,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Formatting => "Formatting",
            Self::AudioAndFeedback => "Audio & Feedback",
            Self::AiCopilot => "AI Copilot",
            Self::VoiceDictation => "Voice Dictation",
            Self::Appearance => "Appearance",
        }
    }

    pub const fn keys(self) -> &'static [SettingKey] {
        match self {
            Self::General => &[
                SettingKey::PauseHotkey,
                SettingKey::StartOnBoot,
                SettingKey::InstantExpand,
                SettingKey::IgnoreFullscreen,
                SettingKey::SystemTrayEnabled,
                SettingKey::AutoUpdate,
            ],
            Self::Formatting => &[
                SettingKey::InlineDateFormat,
                SettingKey::InlineTimeFormat,
                SettingKey::InlineDatetimeFormat,
            ],
            Self::AudioAndFeedback => &[SettingKey::AudioTheme, SettingKey::AudioVolume],
            Self::AiCopilot => &[
                SettingKey::AiProvider,
                SettingKey::AiModel,
                SettingKey::AiCustomEndpoint,
            ],
            Self::VoiceDictation => &[
                SettingKey::VoicePttHotkey,
                SettingKey::VoiceHandsfreeHotkey,
                SettingKey::VoicePreferredLanguages,
                SettingKey::VoiceDictionary,
                SettingKey::VoiceInputDevice,
                SettingKey::PauseMediaWhileDictating,
            ],
            Self::Appearance => &[SettingKey::TuiCursorStyle],
        }
    }
}

pub(crate) trait SettingKeyMeta {
    fn display_name(self) -> &'static str;
    fn description(self) -> &'static str;
    fn editor_kind(self) -> EditorKind;
    fn category(self) -> SettingCategory;
    fn display_value(self, settings: &Settings) -> String;
    fn edit_value(self, settings: &Settings) -> String;
    fn toggle_value(self, settings: &Settings) -> Option<bool>;
    /// Short values edit in the row itself; overlays stay for pickers,
    /// long text, and the hotkey recorder.
    fn inline_edit(self) -> bool;
}

impl SettingKeyMeta for SettingKey {
    fn display_name(self) -> &'static str {
        match self {
            Self::PauseHotkey => "Pause Hotkey",
            Self::AudioTheme => "Audio Theme",
            Self::AudioVolume => "Audio Volume",
            Self::StartOnBoot => "Start on Boot",
            Self::AutoUpdate => "Auto Update",
            Self::AiProvider => "AI Provider",
            Self::AiModel => "AI Model",
            Self::AiCustomEndpoint => "AI Custom Endpoint",
            Self::InstantExpand => "Instant Expand",
            Self::IgnoreFullscreen => "Ignore Fullscreen on Windows",
            Self::SystemTrayEnabled => "System Tray Icon",
            Self::InlineDateFormat => "Inline Date Format",
            Self::InlineTimeFormat => "Inline Time Format",
            Self::InlineDatetimeFormat => "Inline Date & Time Format",
            Self::VoicePttHotkey => "Voice Push-to-Talk Hotkey",
            Self::VoiceHandsfreeHotkey => "Voice Hands-free Hotkey",
            Self::VoicePreferredLanguages => "Voice Preferred Languages",
            Self::VoiceDictionary => "Voice Personal Dictionary",
            Self::VoiceInputDevice => "Voice Input Device",
            Self::PauseMediaWhileDictating => "Pause Media While Dictating",
            Self::TuiCursorStyle => "Cursor Style",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::PauseHotkey => "The keyboard shortcut used to pause Taurine globally",
            Self::AudioTheme => "Sound pack theme used for pause and resume audio cues",
            Self::AudioVolume => "Playback volume for pause and resume audio cues (0-100%)",
            Self::StartOnBoot => "Start Taurine automatically when the system starts",
            Self::AutoUpdate => {
                "Automatically check for and install updates when the service starts"
            }
            Self::AiProvider => "Default AI provider used for inline AI",
            Self::AiModel => "Default AI model used for inline AI",
            Self::AiCustomEndpoint => "Optional custom API endpoint for AI requests",
            Self::InstantExpand => {
                "Expand snippets instantly when typed, without needing a Space or Enter key"
            }
            Self::IgnoreFullscreen => {
                "Pause macro evaluation when running a full-screen application (e.g. games)"
            }
            Self::SystemTrayEnabled => "Show a system tray icon when the service is running",
            Self::InlineDateFormat => "Format template for natural language date expansions",
            Self::InlineTimeFormat => "Format template for natural language time expansions",
            Self::InlineDatetimeFormat => "Format template for combined date and time expansions",
            Self::VoicePttHotkey => "Hold down to record speech, release to transcribe and type",
            Self::VoiceHandsfreeHotkey => {
                "Press once to start dictation mode, press again or Escape to stop and type"
            }
            Self::VoicePreferredLanguages => {
                "Comma-separated language codes for speech recognition candidates (e.g. en, ml, or auto)"
            }
            Self::VoiceDictionary => {
                "Comma-separated custom vocabulary words for phonetic and fuzzy correction"
            }
            Self::VoiceInputDevice => {
                "Microphone device name for voice dictation (leave empty for system default)"
            }
            Self::PauseMediaWhileDictating => {
                "Pause music and video while dictating, then resume afterwards (on by default)"
            }
            Self::TuiCursorStyle => "Caret shape used for text input in the TUI",
        }
    }

    fn category(self) -> SettingCategory {
        match self {
            Self::PauseHotkey
            | Self::StartOnBoot
            | Self::InstantExpand
            | Self::IgnoreFullscreen
            | Self::SystemTrayEnabled
            | Self::AutoUpdate => SettingCategory::General,
            Self::InlineDateFormat | Self::InlineTimeFormat | Self::InlineDatetimeFormat => {
                SettingCategory::Formatting
            }
            Self::AudioTheme | Self::AudioVolume => SettingCategory::AudioAndFeedback,
            Self::AiProvider | Self::AiModel | Self::AiCustomEndpoint => SettingCategory::AiCopilot,
            Self::VoicePttHotkey
            | Self::VoiceHandsfreeHotkey
            | Self::VoicePreferredLanguages
            | Self::VoiceDictionary
            | Self::VoiceInputDevice
            | Self::PauseMediaWhileDictating => SettingCategory::VoiceDictation,
            Self::TuiCursorStyle => SettingCategory::Appearance,
        }
    }

    fn editor_kind(self) -> EditorKind {
        match self {
            Self::StartOnBoot
            | Self::AutoUpdate
            | Self::InstantExpand
            | Self::IgnoreFullscreen
            | Self::SystemTrayEnabled
            | Self::PauseMediaWhileDictating => EditorKind::Toggle,
            Self::AudioVolume => EditorKind::VolumeSlider,
            Self::TuiCursorStyle => EditorKind::CursorStyleSelect,
            Self::AudioTheme => EditorKind::AudioThemeSelect,
            Self::AiProvider => EditorKind::AiProviderSelect,
            Self::AiCustomEndpoint => EditorKind::OptionalTextInput,
            Self::VoiceInputDevice => EditorKind::VoiceInputDeviceSelect,
            Self::PauseHotkey | Self::VoicePttHotkey | Self::VoiceHandsfreeHotkey => {
                EditorKind::HotkeyCapture
            }
            Self::InlineDateFormat | Self::InlineTimeFormat | Self::InlineDatetimeFormat => {
                EditorKind::InlineText
            }
            Self::AiModel | Self::VoiceDictionary | Self::VoicePreferredLanguages => {
                EditorKind::TextInput
            }
        }
    }

    fn toggle_value(self, settings: &Settings) -> Option<bool> {
        match self {
            Self::StartOnBoot => Some(settings.start_on_boot),
            Self::AutoUpdate => Some(settings.auto_update),
            Self::InstantExpand => Some(settings.instant_expand),
            Self::IgnoreFullscreen => Some(settings.ignore_fullscreen),
            Self::SystemTrayEnabled => Some(settings.system_tray_enabled),
            Self::PauseMediaWhileDictating => Some(settings.pause_media_while_dictating),
            _ => None,
        }
    }

    fn inline_edit(self) -> bool {
        match self.editor_kind() {
            EditorKind::InlineText => true,
            EditorKind::TextInput => matches!(self, Self::AiModel),
            EditorKind::OptionalTextInput
            | EditorKind::Toggle
            | EditorKind::VolumeSlider
            | EditorKind::HotkeyCapture
            | EditorKind::CursorStyleSelect
            | EditorKind::AudioThemeSelect
            | EditorKind::AiProviderSelect
            | EditorKind::VoiceInputDeviceSelect => false,
        }
    }

    fn display_value(self, settings: &Settings) -> String {
        match self {
            Self::PauseHotkey => settings.pause_hotkey.clone(),
            Self::AudioTheme => settings.audio_theme.as_str().to_string(),
            Self::AudioVolume => settings.audio_volume.to_string(),
            Self::StartOnBoot => settings.start_on_boot.to_string(),
            Self::AutoUpdate => settings.auto_update.to_string(),
            Self::TuiCursorStyle => cursor_style_label(settings.tui_cursor_style).to_string(),
            Self::AiProvider => optional_value_label(settings.ai_provider.as_deref()).to_string(),
            Self::AiModel => optional_value_label(settings.ai_model.as_deref()).to_string(),
            Self::AiCustomEndpoint => {
                optional_value_label(settings.ai_custom_endpoint.as_deref()).to_string()
            }
            Self::InstantExpand => settings.instant_expand.to_string(),
            Self::IgnoreFullscreen => settings.ignore_fullscreen.to_string(),
            Self::SystemTrayEnabled => settings.system_tray_enabled.to_string(),
            Self::InlineDateFormat => settings.inline_date_format.clone(),
            Self::InlineTimeFormat => settings.inline_time_format.clone(),
            Self::InlineDatetimeFormat => settings.inline_datetime_format.clone(),
            Self::PauseMediaWhileDictating => settings.pause_media_while_dictating.to_string(),
            Self::VoicePttHotkey => settings.voice_ptt_hotkey.clone(),
            Self::VoiceHandsfreeHotkey => settings.voice_handsfree_hotkey.clone(),
            Self::VoicePreferredLanguages => {
                if settings.voice_preferred_languages.is_empty() {
                    "en".to_string()
                } else {
                    settings.voice_preferred_languages.clone()
                }
            }
            Self::VoiceDictionary => {
                if settings.voice_dictionary.is_empty() {
                    "<empty>".to_string()
                } else {
                    settings.voice_dictionary.clone()
                }
            }
            Self::VoiceInputDevice => settings
                .voice_input_device
                .as_deref()
                .filter(|device| !device.trim().is_empty())
                .unwrap_or("System Default")
                .to_string(),
        }
    }

    fn edit_value(self, settings: &Settings) -> String {
        match self {
            Self::PauseHotkey => settings.pause_hotkey.clone(),
            Self::InlineDateFormat => settings.inline_date_format.clone(),
            Self::InlineTimeFormat => settings.inline_time_format.clone(),
            Self::InlineDatetimeFormat => settings.inline_datetime_format.clone(),
            Self::AiProvider => settings.ai_provider.clone().unwrap_or_default(),
            Self::AiModel => settings.ai_model.clone().unwrap_or_default(),
            Self::AiCustomEndpoint => settings.ai_custom_endpoint.clone().unwrap_or_default(),
            Self::VoicePttHotkey => settings.voice_ptt_hotkey.clone(),
            Self::VoiceHandsfreeHotkey => settings.voice_handsfree_hotkey.clone(),
            Self::VoicePreferredLanguages => settings.voice_preferred_languages.clone(),
            Self::VoiceDictionary => settings.voice_dictionary.clone(),
            Self::StartOnBoot
            | Self::AutoUpdate
            | Self::InstantExpand
            | Self::IgnoreFullscreen
            | Self::SystemTrayEnabled
            | Self::AudioTheme
            | Self::AudioVolume
            | Self::PauseMediaWhileDictating
            | Self::VoiceInputDevice
            | Self::TuiCursorStyle => self.display_value(settings),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorKind {
    Toggle,
    VolumeSlider,
    HotkeyCapture,
    InlineText,
    TextInput,
    OptionalTextInput,
    CursorStyleSelect,
    AudioThemeSelect,
    AiProviderSelect,
    VoiceInputDeviceSelect,
}

impl EditorKind {
    /// Values backed by a centered option overlay, which open from a
    /// single click on the value column.
    pub(crate) const fn opens_select(self) -> bool {
        matches!(
            self,
            Self::CursorStyleSelect
                | Self::AudioThemeSelect
                | Self::AiProviderSelect
                | Self::VoiceInputDeviceSelect
        )
    }
}

pub(crate) const fn cursor_style_label(style: TuiCursorStyle) -> &'static str {
    match style {
        TuiCursorStyle::Block => "block",
        TuiCursorStyle::Bar => "bar",
        TuiCursorStyle::Underscore => "underscore",
    }
}

pub(crate) const fn cursor_set_cursor_style(
    style: TuiCursorStyle,
) -> crossterm::cursor::SetCursorStyle {
    match style {
        TuiCursorStyle::Block => crossterm::cursor::SetCursorStyle::SteadyBlock,
        TuiCursorStyle::Bar => crossterm::cursor::SetCursorStyle::SteadyBar,
        TuiCursorStyle::Underscore => crossterm::cursor::SetCursorStyle::SteadyUnderScore,
    }
}

pub(crate) fn optional_value_label(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("<unset>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_values_edit_inline_while_choices_and_long_text_do_not() {
        assert!(SettingKey::InlineDateFormat.inline_edit());
        assert!(SettingKey::InlineTimeFormat.inline_edit());
        assert!(SettingKey::InlineDatetimeFormat.inline_edit());
        assert!(SettingKey::AiModel.inline_edit());
        assert!(!SettingKey::AiCustomEndpoint.inline_edit());
        assert!(!SettingKey::VoiceDictionary.inline_edit());
        assert!(!SettingKey::VoiceInputDevice.inline_edit());
        assert!(!SettingKey::AudioTheme.inline_edit());
        assert!(!SettingKey::AudioVolume.inline_edit());
        assert!(!SettingKey::PauseHotkey.inline_edit());
    }

    #[test]
    fn unset_optional_text_edits_from_an_empty_field() {
        let settings = Settings::default();
        assert_eq!(SettingKey::AiCustomEndpoint.edit_value(&settings), "");
        assert_eq!(SettingKey::AiProvider.edit_value(&settings), "");
        assert_eq!(SettingKey::AiModel.edit_value(&settings), "");
    }

    #[test]
    fn voice_device_display_falls_back_to_system_default() {
        let mut settings = Settings::default();
        assert_eq!(
            SettingKey::VoiceInputDevice.display_value(&settings),
            "System Default"
        );
        settings.voice_input_device = Some("USB Microphone".to_string());
        assert_eq!(
            SettingKey::VoiceInputDevice.display_value(&settings),
            "USB Microphone"
        );
    }
}
