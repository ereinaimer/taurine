use taurine_core::settings::{Settings, SpinnerStyle};

pub(crate) use taurine_core::settings::SettingKey;

pub(crate) trait SettingKeyMeta {
    fn display_name(self) -> &'static str;
    fn description(self) -> &'static str;
    fn editor_kind(self) -> EditorKind;
    fn display_value(self, settings: &Settings) -> String;
    fn edit_value(self, settings: &Settings) -> String;
}

impl SettingKeyMeta for SettingKey {
    fn display_name(self) -> &'static str {
        match self {
            Self::PauseHotkey => "Pause Hotkey",
            Self::PauseNotificationsEnabled => "Pause Notifications",
            Self::PauseAudioEnabled => "Pause Audio",
            Self::StartOnBoot => "Start on Boot",
            Self::AutoUpdate => "Auto Update",
            Self::InlineTabCompletionEnabled => "Inline Tab Completion",
            Self::InlineCaseTransformEnabled => "Inline Case Transform",
            Self::Wpm => "Words Per Minute",
            Self::SpinnerStyle => "Spinner Style",
            Self::AiProvider => "AI Provider",
            Self::AiModel => "AI Model",
            Self::AiCustomEndpoint => "AI Custom Endpoint",
            Self::InlineAiTriggerMode => "Inline AI Trigger Mode",
            Self::InlineAiTrigger => "Inline AI Trigger",
            Self::InlineAiTriggerOpen => "Inline AI Trigger Open",
            Self::InlineAiTriggerClose => "Inline AI Trigger Close",
            Self::ClipboardRestoreDelayMs => "Clipboard Restore Delay (ms)",
            Self::InstantExpand => "Instant Expand",
            Self::IgnoreFullscreen => "Ignore Fullscreen on Windows",
            Self::RpcMode => "Service RPC Mode",
            Self::RpcHost => "Service RPC Host",
            Self::RpcPort => "Service RPC Port",
            Self::ScriptsEnabled => "Scripts Enabled",
            Self::ScriptTimeout => "Script Execution Timeout",
            Self::AiTemperature => "AI Temperature",
            Self::AiMaxTokens => "AI Max Tokens",
            Self::AiSystemPrompt => "AI System Prompt",
            Self::ClipboardHistoryEnabled => "Clipboard History",
            Self::ClipboardHistoryRetentionSecs => "Clipboard History Retention (s)",
            Self::InlineEmojiEnabled => "Inline Emoji",
            Self::InlineEmojiTriggerChar => "Inline Emoji Trigger Character",
            Self::SystemTrayEnabled => "System Tray Icon",
            Self::InlineDatetimeEnabled => "Inline Date & Time",
            Self::InlineDatetimeDateFormat => "Date Format",
            Self::InlineDatetimeTimeFormat => "Time Format",
            Self::InlineDatetimeDatetimeFormat => "DateTime Format",
            Self::InlineDatetimeDialect => "Dialect (uk/us)",
            Self::InlineCurrencyToWordsEnabled => "Inline Currency to Words",
            Self::NotifyOnUpdate => "Notify on Update",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::PauseHotkey => "The keyboard shortcut used to pause Taurine globally",
            Self::PauseNotificationsEnabled => {
                "Show a notification when Taurine is paused or resumed"
            }
            Self::PauseAudioEnabled => "Play an audio cue when Taurine is paused or resumed",
            Self::StartOnBoot => "Start Taurine automatically when the system starts",
            Self::AutoUpdate => {
                "Automatically check for and install updates when the service starts"
            }
            Self::InlineTabCompletionEnabled => {
                "Use Tab and Shift+Tab to cycle trigger completions while typing a trigger word"
            }
            Self::InlineCaseTransformEnabled => {
                "Use Left and Right arrows to cycle through capitalization cases of expanded text within the undo window"
            }
            Self::Wpm => "Used to estimate time saved from keystrokes saved",
            Self::SpinnerStyle => "Animation style used while Taurine is processing",
            Self::AiProvider => "Default AI provider used for inline AI",
            Self::AiModel => "Default AI model used for inline AI",
            Self::AiCustomEndpoint => "Optional custom API endpoint for AI requests",
            Self::InlineAiTriggerMode => {
                "The behavior style of inline AI triggers (Symmetric or Asymmetric)"
            }
            Self::InlineAiTrigger => {
                "The trigger symbol used to both open and close an inline AI prompt in symmetric mode"
            }
            Self::InlineAiTriggerOpen => "The trigger text used to start an inline AI prompt",
            Self::InlineAiTriggerClose => "The trigger text used to end an inline AI prompt",
            Self::ClipboardRestoreDelayMs => {
                "The delay in milliseconds between pasting and restoring the clipboard"
            }
            Self::InstantExpand => {
                "Expand snippets instantly when typed, without needing a Space or Enter key"
            }
            Self::IgnoreFullscreen => {
                "Pause macro evaluation when running a full-screen application (e.g. games)"
            }
            Self::RpcMode => "The transport protocol used for service RPC (socket or tcp)",
            Self::RpcHost => "The network interface IP address the service binds to",
            Self::RpcPort => "The network port the gRPC RPC server listens on (1024-65535)",
            Self::ScriptsEnabled => "Allow execution of shell scripts in triggers",
            Self::ScriptTimeout => {
                "Maximum script execution time before termination (0 for infinite)"
            }
            Self::AiTemperature => "Controls randomness (0.0 for deterministic, 1.0 for creative).",
            Self::AiMaxTokens => "The maximum number of tokens to generate in the completion.",
            Self::AiSystemPrompt => "Overrides the default immutable system instructions.",
            Self::ClipboardHistoryEnabled => {
                "Enable local clipboard history tracking and [clip] variables"
            }
            Self::ClipboardHistoryRetentionSecs => {
                "Delete history items automatically after this time (in seconds)"
            }
            Self::InlineEmojiEnabled => "Enable inline emoji picker and completion",
            Self::InlineEmojiTriggerChar => "The character used to trigger the inline emoji picker",
            Self::SystemTrayEnabled => "Show a system tray icon when the service is running",
            Self::InlineDatetimeEnabled => {
                "Enable expanding natural language dates and times on Enter"
            }
            Self::InlineDatetimeDateFormat => "Output format when only a date is parsed",
            Self::InlineDatetimeTimeFormat => "Output format when only a time is parsed",
            Self::InlineDatetimeDatetimeFormat => {
                "Output format when both date and time are parsed"
            }
            Self::InlineDatetimeDialect => {
                "Preference for ambiguous dates like 07/12 (uk = dd/mm, us = mm/dd)"
            }
            Self::InlineCurrencyToWordsEnabled => {
                "Enable expanding numbers with currency symbols to their text representations on Enter"
            }
            Self::NotifyOnUpdate => {
                "Show a system notification when Taurine successfully updates in the background"
            }
        }
    }

    fn editor_kind(self) -> EditorKind {
        match self {
            Self::PauseNotificationsEnabled
            | Self::PauseAudioEnabled
            | Self::StartOnBoot
            | Self::AutoUpdate
            | Self::InlineTabCompletionEnabled
            | Self::InlineCaseTransformEnabled
            | Self::InstantExpand
            | Self::IgnoreFullscreen
            | Self::ScriptsEnabled
            | Self::ClipboardHistoryEnabled
            | Self::InlineEmojiEnabled
            | Self::SystemTrayEnabled
            | Self::InlineDatetimeEnabled
            | Self::InlineCurrencyToWordsEnabled
            | Self::NotifyOnUpdate => EditorKind::Toggle,
            Self::Wpm
            | Self::ClipboardRestoreDelayMs
            | Self::RpcPort
            | Self::ScriptTimeout
            | Self::AiMaxTokens
            | Self::ClipboardHistoryRetentionSecs => EditorKind::NumberInput,
            Self::SpinnerStyle => EditorKind::SpinnerSelect,
            Self::AiProvider => EditorKind::AiProviderSelect,
            Self::AiCustomEndpoint | Self::AiTemperature | Self::AiSystemPrompt => {
                EditorKind::OptionalTextInput
            }
            Self::InlineAiTriggerMode => EditorKind::InlineAiTriggerModeSelect,
            Self::RpcMode => EditorKind::RpcModeSelect,
            Self::InlineEmojiTriggerChar => EditorKind::SingleCharInput,
            Self::PauseHotkey
            | Self::AiModel
            | Self::InlineAiTrigger
            | Self::InlineAiTriggerOpen
            | Self::InlineAiTriggerClose
            | Self::RpcHost
            | Self::InlineDatetimeDateFormat
            | Self::InlineDatetimeTimeFormat
            | Self::InlineDatetimeDatetimeFormat
            | Self::InlineDatetimeDialect => EditorKind::TextInput,
        }
    }

    fn display_value(self, settings: &Settings) -> String {
        match self {
            Self::PauseHotkey => settings.pause_hotkey.clone(),
            Self::PauseNotificationsEnabled => settings.pause_notifications_enabled.to_string(),
            Self::PauseAudioEnabled => settings.pause_audio_enabled.to_string(),
            Self::StartOnBoot => settings.start_on_boot.to_string(),
            Self::AutoUpdate => settings.auto_update.to_string(),
            Self::InlineTabCompletionEnabled => settings.inline_tab_completion_enabled.to_string(),
            Self::InlineCaseTransformEnabled => settings.inline_case_transform_enabled.to_string(),
            Self::Wpm => settings.wpm.to_string(),
            Self::SpinnerStyle => spinner_style_label(settings.spinner_style).to_string(),
            Self::AiProvider => optional_value_label(settings.ai_provider.as_deref()).to_string(),
            Self::AiModel => optional_value_label(settings.ai_model.as_deref()).to_string(),
            Self::AiCustomEndpoint => {
                optional_value_label(settings.ai_custom_endpoint.as_deref()).to_string()
            }
            Self::InlineAiTriggerMode => match settings.inline_ai_trigger_mode {
                taurine_core::settings::InlineAiTriggerMode::Symmetric => "symmetric".to_string(),
                taurine_core::settings::InlineAiTriggerMode::Asymmetric => "asymmetric".to_string(),
            },
            Self::InlineAiTrigger => settings.inline_ai_trigger.clone(),
            Self::InlineAiTriggerOpen => settings.inline_ai_trigger_open.clone(),
            Self::InlineAiTriggerClose => settings.inline_ai_trigger_close.clone(),
            Self::ClipboardRestoreDelayMs => settings.clipboard_restore_delay_ms.to_string(),
            Self::InstantExpand => settings.instant_expand.to_string(),
            Self::IgnoreFullscreen => settings.ignore_fullscreen.to_string(),
            Self::RpcPort => settings.rpc_port.to_string(),
            Self::ScriptTimeout => settings.script_timeout.to_string(),
            Self::AiTemperature => {
                optional_value_label(settings.ai_temperature.map(|v| v.to_string()).as_deref())
                    .to_string()
            }
            Self::AiMaxTokens => {
                optional_value_label(settings.ai_max_tokens.map(|v| v.to_string()).as_deref())
                    .to_string()
            }
            Self::AiSystemPrompt => settings
                .ai_system_prompt
                .as_deref()
                .unwrap_or(taurine_core::settings::DEFAULT_AI_SYSTEM_PROMPT)
                .to_string(),
            Self::RpcMode => match settings.rpc_mode {
                taurine_core::settings::RpcMode::Socket => "socket".to_string(),
                taurine_core::settings::RpcMode::Tcp => "tcp".to_string(),
            },
            Self::RpcHost => settings.rpc_host.clone(),
            Self::ScriptsEnabled => settings.scripts_enabled.to_string(),
            Self::ClipboardHistoryEnabled => settings.clipboard_history_enabled.to_string(),
            Self::ClipboardHistoryRetentionSecs => {
                settings.clipboard_history_retention_secs.to_string()
            }
            Self::InlineEmojiEnabled => settings.inline_emoji_enabled.to_string(),
            Self::InlineEmojiTriggerChar => settings.inline_emoji_trigger_char.to_string(),
            Self::SystemTrayEnabled => settings.system_tray_enabled.to_string(),
            Self::InlineDatetimeEnabled => settings.inline_datetime_enabled.to_string(),
            Self::InlineDatetimeDateFormat => settings.inline_datetime_date_format.clone(),
            Self::InlineDatetimeTimeFormat => settings.inline_datetime_time_format.clone(),
            Self::InlineDatetimeDatetimeFormat => settings.inline_datetime_datetime_format.clone(),
            Self::InlineDatetimeDialect => settings.inline_datetime_dialect.clone(),
            Self::InlineCurrencyToWordsEnabled => {
                settings.inline_currency_to_words_enabled.to_string()
            }
            Self::NotifyOnUpdate => settings.notify_on_update.to_string(),
        }
    }

    fn edit_value(self, settings: &Settings) -> String {
        match self {
            Self::PauseHotkey => settings.pause_hotkey.clone(),
            Self::Wpm => settings.wpm.to_string(),
            Self::InlineDatetimeDateFormat => settings.inline_datetime_date_format.clone(),
            Self::InlineDatetimeTimeFormat => settings.inline_datetime_time_format.clone(),
            Self::InlineDatetimeDatetimeFormat => settings.inline_datetime_datetime_format.clone(),
            Self::InlineDatetimeDialect => settings.inline_datetime_dialect.clone(),
            Self::AiProvider => settings.ai_provider.clone().unwrap_or_default(),
            Self::AiModel => settings.ai_model.clone().unwrap_or_default(),
            Self::AiCustomEndpoint => settings.ai_custom_endpoint.clone().unwrap_or_default(),
            Self::InlineAiTrigger => settings.inline_ai_trigger.clone(),
            Self::InlineAiTriggerOpen => settings.inline_ai_trigger_open.clone(),
            Self::InlineAiTriggerClose => settings.inline_ai_trigger_close.clone(),
            Self::RpcHost => settings.rpc_host.clone(),
            Self::PauseNotificationsEnabled
            | Self::PauseAudioEnabled
            | Self::StartOnBoot
            | Self::AutoUpdate
            | Self::InlineTabCompletionEnabled
            | Self::InlineCaseTransformEnabled
            | Self::InstantExpand
            | Self::IgnoreFullscreen
            | Self::SpinnerStyle
            | Self::InlineAiTriggerMode
            | Self::RpcMode
            | Self::ScriptsEnabled
            | Self::ClipboardRestoreDelayMs
            | Self::RpcPort
            | Self::ScriptTimeout
            | Self::ClipboardHistoryEnabled
            | Self::ClipboardHistoryRetentionSecs
            | Self::InlineEmojiEnabled
            | Self::InlineEmojiTriggerChar
            | Self::SystemTrayEnabled
            | Self::InlineDatetimeEnabled
            | Self::InlineCurrencyToWordsEnabled
            | Self::NotifyOnUpdate => self.display_value(settings),
            Self::AiTemperature => {
                optional_value_label(settings.ai_temperature.map(|v| v.to_string()).as_deref())
                    .to_string()
            }
            Self::AiMaxTokens => {
                optional_value_label(settings.ai_max_tokens.map(|v| v.to_string()).as_deref())
                    .to_string()
            }
            Self::AiSystemPrompt => settings
                .ai_system_prompt
                .as_deref()
                .unwrap_or(taurine_core::settings::DEFAULT_AI_SYSTEM_PROMPT)
                .to_string(),
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
    AiProviderSelect,
    InlineAiTriggerModeSelect,
    RpcModeSelect,
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
