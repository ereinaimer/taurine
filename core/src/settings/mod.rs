use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};

static CACHED_SCRIPT_TIMEOUT: AtomicU32 = AtomicU32::new(15);
static CACHED_CLIPBOARD_RESTORE_DELAY: AtomicU32 = AtomicU32::new(350);
static CACHED_WPM: AtomicU32 = AtomicU32::new(60);
static CACHED_CLIPBOARD_HISTORY_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
static CACHED_CLIPBOARD_HISTORY_RETENTION_SECS: AtomicU32 = AtomicU32::new(300);
static CACHED_INLINE_EMOJI_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
static CACHED_INLINE_EMOJI_TRIGGER_CHAR: AtomicU32 = AtomicU32::new(':' as u32);
static CACHED_SCRIPTS_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
static CACHED_INLINE_CASE_TRANSFORM_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

static CACHED_INLINE_DATETIME_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
static CACHED_INLINE_DICTIONARY_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
static CACHED_INLINE_DICTIONARY_MODE: parking_lot::RwLock<InlineDictionaryMode> =
    parking_lot::RwLock::new(InlineDictionaryMode::Lite);
static CACHED_INLINE_DATETIME_DATE_FORMAT: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_INLINE_DATETIME_TIME_FORMAT: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_INLINE_DATETIME_DATETIME_FORMAT: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_INLINE_DATETIME_DIALECT: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_INLINE_CURRENCY_TO_WORDS_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static CACHED_AUDIO_THEME: parking_lot::RwLock<AudioTheme> =
    parking_lot::RwLock::new(AudioTheme::Minimal);
static CACHED_AUDIO_VOLUME: AtomicU32 = AtomicU32::new(50);

pub fn set_cached_audio_theme(theme: AudioTheme) {
    *CACHED_AUDIO_THEME.write() = theme;
}

pub fn get_cached_audio_theme() -> AudioTheme {
    *CACHED_AUDIO_THEME.read()
}

pub fn set_cached_audio_volume(volume: u32) {
    CACHED_AUDIO_VOLUME.store(Settings::sanitize_audio_volume(volume), Ordering::Relaxed);
}

pub fn get_cached_audio_volume() -> u32 {
    CACHED_AUDIO_VOLUME.load(Ordering::Relaxed)
}

pub fn set_cached_inline_emoji_enabled(enabled: bool) {
    CACHED_INLINE_EMOJI_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_cached_inline_dictionary_enabled(enabled: bool) {
    CACHED_INLINE_DICTIONARY_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn get_cached_inline_dictionary_enabled() -> bool {
    CACHED_INLINE_DICTIONARY_ENABLED.load(Ordering::Relaxed)
}

pub fn set_cached_inline_dictionary_mode(mode: InlineDictionaryMode) {
    *CACHED_INLINE_DICTIONARY_MODE.write() = mode;
}

pub fn get_cached_inline_dictionary_mode() -> InlineDictionaryMode {
    *CACHED_INLINE_DICTIONARY_MODE.read()
}

pub fn set_cached_inline_emoji_trigger_char(c: char) {
    CACHED_INLINE_EMOJI_TRIGGER_CHAR.store(c as u32, Ordering::Relaxed);
}

pub fn get_cached_inline_emoji_enabled() -> bool {
    CACHED_INLINE_EMOJI_ENABLED.load(Ordering::Relaxed)
}

pub fn get_cached_inline_emoji_trigger_char() -> char {
    let u = CACHED_INLINE_EMOJI_TRIGGER_CHAR.load(Ordering::Relaxed);
    std::char::from_u32(u).unwrap_or(':')
}

pub fn set_cached_scripts_enabled(enabled: bool) {
    CACHED_SCRIPTS_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn get_cached_scripts_enabled() -> bool {
    CACHED_SCRIPTS_ENABLED.load(Ordering::Relaxed)
}

pub fn set_cached_inline_case_transform_enabled(enabled: bool) {
    CACHED_INLINE_CASE_TRANSFORM_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn get_cached_inline_case_transform_enabled() -> bool {
    CACHED_INLINE_CASE_TRANSFORM_ENABLED.load(Ordering::Relaxed)
}

pub fn set_cached_script_timeout(timeout: u32) {
    CACHED_SCRIPT_TIMEOUT.store(timeout, Ordering::Relaxed);
}

pub fn set_cached_clipboard_restore_delay(delay: u32) {
    CACHED_CLIPBOARD_RESTORE_DELAY.store(delay, Ordering::Relaxed);
}

pub fn set_cached_wpm(wpm: u32) {
    CACHED_WPM.store(wpm, Ordering::Relaxed);
}

pub fn set_cached_clipboard_history_enabled(enabled: bool) {
    CACHED_CLIPBOARD_HISTORY_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_cached_clipboard_history_retention_secs(secs: u32) {
    CACHED_CLIPBOARD_HISTORY_RETENTION_SECS.store(secs, Ordering::Relaxed);
}

pub fn get_cached_clipboard_restore_delay() -> u32 {
    CACHED_CLIPBOARD_RESTORE_DELAY.load(Ordering::Relaxed)
}

pub fn get_cached_wpm() -> u32 {
    CACHED_WPM.load(Ordering::Relaxed)
}

pub fn get_cached_clipboard_history_enabled() -> bool {
    CACHED_CLIPBOARD_HISTORY_ENABLED.load(Ordering::Relaxed)
}

pub fn get_cached_clipboard_history_retention_secs() -> u32 {
    CACHED_CLIPBOARD_HISTORY_RETENTION_SECS.load(Ordering::Relaxed)
}

pub fn set_cached_inline_datetime_enabled(enabled: bool) {
    CACHED_INLINE_DATETIME_ENABLED.store(enabled, Ordering::Relaxed);
}
pub fn get_cached_inline_datetime_enabled() -> bool {
    CACHED_INLINE_DATETIME_ENABLED.load(Ordering::Relaxed)
}
pub fn set_cached_inline_currency_to_words_enabled(enabled: bool) {
    CACHED_INLINE_CURRENCY_TO_WORDS_ENABLED.store(enabled, Ordering::Relaxed);
}
pub fn get_cached_inline_currency_to_words_enabled() -> bool {
    CACHED_INLINE_CURRENCY_TO_WORDS_ENABLED.load(Ordering::Relaxed)
}
pub fn set_cached_inline_datetime_date_format(f: String) {
    *CACHED_INLINE_DATETIME_DATE_FORMAT.write() = Some(f);
}
pub fn get_cached_inline_datetime_date_format() -> String {
    CACHED_INLINE_DATETIME_DATE_FORMAT
        .read()
        .clone()
        .unwrap_or_else(|| "MMMM D, YYYY".to_string())
}
pub fn set_cached_inline_datetime_time_format(f: String) {
    *CACHED_INLINE_DATETIME_TIME_FORMAT.write() = Some(f);
}
pub fn get_cached_inline_datetime_time_format() -> String {
    CACHED_INLINE_DATETIME_TIME_FORMAT
        .read()
        .clone()
        .unwrap_or_else(|| "h:mm A".to_string())
}
pub fn set_cached_inline_datetime_datetime_format(f: String) {
    *CACHED_INLINE_DATETIME_DATETIME_FORMAT.write() = Some(f);
}
pub fn get_cached_inline_datetime_datetime_format() -> String {
    CACHED_INLINE_DATETIME_DATETIME_FORMAT
        .read()
        .clone()
        .unwrap_or_else(|| "MMMM D, YYYY 'at' h:mm A".to_string())
}
pub fn set_cached_inline_datetime_dialect(d: String) {
    *CACHED_INLINE_DATETIME_DIALECT.write() = Some(d);
}
pub fn get_cached_inline_datetime_dialect() -> String {
    CACHED_INLINE_DATETIME_DIALECT
        .read()
        .clone()
        .unwrap_or_else(|| "uk".to_string())
}

pub const DEFAULT_AI_SYSTEM_PROMPT: &str = "You are Tau, an inline text expander. Provide complete but highly concise answers. Plain text only. No markdown, lists, code fences, or newlines. No filler, greetings, explanations, or extra context. Output your entire response as one continuous string.";

mod apply;
pub mod manager;

pub use apply::{
    ApplySettingOutcome, apply_setting_input, apply_setting_input_with_manager,
    default_setting_input, parse_boolean_setting_value, parse_spinner_style,
    reset_setting_to_default,
};
pub use manager::SettingsManager;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioTheme {
    #[default]
    Minimal,
    Soft,
    Glass,
    Arcade,
    Mechanical,
    Organic,
    Dreamy,
    Scifi,
    Rubber,
    Cinematic,
    Studio,
    Zen,
}

impl AudioTheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Soft => "soft",
            Self::Glass => "glass",
            Self::Arcade => "arcade",
            Self::Mechanical => "mechanical",
            Self::Organic => "organic",
            Self::Dreamy => "dreamy",
            Self::Scifi => "scifi",
            Self::Rubber => "rubber",
            Self::Cinematic => "cinematic",
            Self::Studio => "studio",
            Self::Zen => "zen",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::Minimal,
            Self::Soft,
            Self::Glass,
            Self::Arcade,
            Self::Mechanical,
            Self::Organic,
            Self::Dreamy,
            Self::Scifi,
            Self::Rubber,
            Self::Cinematic,
            Self::Studio,
            Self::Zen,
        ]
    }
}

impl std::str::FromStr for AudioTheme {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "minimal" => Ok(Self::Minimal),
            "soft" => Ok(Self::Soft),
            "glass" => Ok(Self::Glass),
            "arcade" => Ok(Self::Arcade),
            "mechanical" => Ok(Self::Mechanical),
            "organic" => Ok(Self::Organic),
            "dreamy" => Ok(Self::Dreamy),
            "scifi" | "sci-fi" => Ok(Self::Scifi),
            "rubber" => Ok(Self::Rubber),
            "cinematic" => Ok(Self::Cinematic),
            "studio" => Ok(Self::Studio),
            "zen" => Ok(Self::Zen),
            _ => Err(format!("Unknown audio theme: {s}")),
        }
    }
}

impl std::fmt::Display for AudioTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SpinnerStyle {
    #[default]
    Braille,
    Arc,
    Classic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum InlineDictionaryMode {
    #[default]
    Lite,
    Full,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RpcMode {
    #[default]
    Socket,
    Tcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
    PauseHotkey,
    PauseNotificationsEnabled,
    PauseAudioEnabled,
    AudioTheme,
    AudioVolume,
    StartOnBoot,
    AutoUpdate,
    InlineTabCompletionEnabled,
    InlineCaseTransformEnabled,
    Wpm,
    SpinnerStyle,
    AiProvider,
    AiModel,
    AiCustomEndpoint,
    InlineAiEnabled,
    ClipboardRestoreDelayMs,
    InstantExpand,
    IgnoreFullscreen,
    RpcMode,
    RpcHost,
    RpcPort,
    ScriptsEnabled,
    ScriptTimeout,
    AiTemperature,
    AiMaxTokens,
    AiSystemPrompt,
    ClipboardHistoryEnabled,
    ClipboardHistoryRetentionSecs,
    InlineEmojiEnabled,
    InlineEmojiTriggerChar,
    SystemTrayEnabled,
    InlineDatetimeEnabled,
    InlineDatetimeDateFormat,
    InlineDatetimeTimeFormat,
    InlineDatetimeDatetimeFormat,
    InlineDatetimeDialect,
    InlineCurrencyToWordsEnabled,
    InlineDictionaryEnabled,
    InlineDictionaryMode,
    NotifyOnUpdate,
}

impl SettingKey {
    pub const ALL: [Self; 40] = [
        Self::PauseHotkey,
        Self::PauseNotificationsEnabled,
        Self::PauseAudioEnabled,
        Self::AudioTheme,
        Self::AudioVolume,
        Self::StartOnBoot,
        Self::AutoUpdate,
        Self::InlineTabCompletionEnabled,
        Self::InlineCaseTransformEnabled,
        Self::Wpm,
        Self::SpinnerStyle,
        Self::AiProvider,
        Self::AiModel,
        Self::AiCustomEndpoint,
        Self::InlineAiEnabled,
        Self::ClipboardRestoreDelayMs,
        Self::InstantExpand,
        Self::IgnoreFullscreen,
        Self::RpcMode,
        Self::RpcHost,
        Self::RpcPort,
        Self::ScriptsEnabled,
        Self::ScriptTimeout,
        Self::AiTemperature,
        Self::AiMaxTokens,
        Self::AiSystemPrompt,
        Self::ClipboardHistoryEnabled,
        Self::ClipboardHistoryRetentionSecs,
        Self::InlineEmojiEnabled,
        Self::InlineEmojiTriggerChar,
        Self::SystemTrayEnabled,
        Self::InlineDatetimeEnabled,
        Self::InlineDatetimeDateFormat,
        Self::InlineDatetimeTimeFormat,
        Self::InlineDatetimeDatetimeFormat,
        Self::InlineDatetimeDialect,
        Self::InlineCurrencyToWordsEnabled,
        Self::InlineDictionaryEnabled,
        Self::InlineDictionaryMode,
        Self::NotifyOnUpdate,
    ];

    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::PauseHotkey => "pause_hotkey",
            Self::PauseNotificationsEnabled => "pause_notifications_enabled",
            Self::PauseAudioEnabled => "pause_audio_enabled",
            Self::AudioTheme => "audio_theme",
            Self::AudioVolume => "audio_volume",
            Self::StartOnBoot => "start_on_boot",
            Self::AutoUpdate => "auto_update",
            Self::InlineTabCompletionEnabled => "inline_tab_completion_enabled",
            Self::InlineCaseTransformEnabled => "inline_case_transform_enabled",
            Self::Wpm => "wpm",
            Self::SpinnerStyle => "spinner_style",
            Self::AiProvider => "ai_provider",
            Self::AiModel => "ai_model",
            Self::AiCustomEndpoint => "ai_custom_endpoint",
            Self::InlineAiEnabled => "inline_ai_enabled",
            Self::ClipboardRestoreDelayMs => "clipboard_restore_delay_ms",
            Self::InstantExpand => "instant_expand",
            Self::IgnoreFullscreen => "ignore_fullscreen",
            Self::RpcMode => "rpc_mode",
            Self::RpcHost => "rpc_host",
            Self::RpcPort => "rpc_port",
            Self::ScriptsEnabled => "scripts_enabled",
            Self::ScriptTimeout => "script_timeout",
            Self::AiTemperature => "ai_temperature",
            Self::AiMaxTokens => "ai_max_tokens",
            Self::AiSystemPrompt => "ai_system_prompt",
            Self::ClipboardHistoryEnabled => "clipboard_history_enabled",
            Self::ClipboardHistoryRetentionSecs => "clipboard_history_retention_secs",
            Self::InlineEmojiEnabled => "inline_emoji_enabled",
            Self::InlineEmojiTriggerChar => "inline_emoji_trigger_char",
            Self::SystemTrayEnabled => "system_tray_enabled",
            Self::InlineDatetimeEnabled => "inline_datetime_enabled",
            Self::InlineDatetimeDateFormat => "inline_datetime_date_format",
            Self::InlineDatetimeTimeFormat => "inline_datetime_time_format",
            Self::InlineDatetimeDatetimeFormat => "inline_datetime_datetime_format",
            Self::InlineDatetimeDialect => "inline_datetime_dialect",
            Self::InlineCurrencyToWordsEnabled => "inline_currency_to_words_enabled",
            Self::InlineDictionaryEnabled => "inline_dictionary_enabled",
            Self::InlineDictionaryMode => "inline_dictionary_mode",
            Self::NotifyOnUpdate => "notify_on_update",
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub pause_hotkey: String,
    pub pause_notifications_enabled: bool,
    pub pause_audio_enabled: bool,
    pub audio_theme: AudioTheme,
    pub audio_volume: u32,
    pub start_on_boot: bool,
    pub inline_tab_completion_enabled: bool,
    pub inline_case_transform_enabled: bool,
    pub wpm: u32,
    pub spinner_style: SpinnerStyle,
    pub ai_provider: Option<String>,
    pub ai_model: Option<String>,
    pub ai_custom_endpoint: Option<String>,
    pub inline_ai_enabled: bool,
    pub clipboard_restore_delay_ms: u32,
    pub instant_expand: bool,
    pub rpc_mode: RpcMode,
    pub rpc_host: String,
    pub rpc_port: u16,
    pub ignore_fullscreen: bool,
    pub script_timeout: u32,
    pub ai_temperature: Option<f32>,
    pub ai_max_tokens: Option<u32>,
    pub ai_system_prompt: Option<String>,
    pub auto_update: bool,
    pub clipboard_history_enabled: bool,
    pub clipboard_history_retention_secs: u32,
    pub inline_emoji_enabled: bool,
    pub inline_emoji_trigger_char: char,
    pub scripts_enabled: bool,
    pub system_tray_enabled: bool,
    pub inline_datetime_enabled: bool,
    pub inline_datetime_date_format: String,
    pub inline_datetime_time_format: String,
    pub inline_datetime_datetime_format: String,
    pub inline_datetime_dialect: String,
    pub inline_currency_to_words_enabled: bool,
    pub inline_dictionary_enabled: bool,
    pub inline_dictionary_mode: InlineDictionaryMode,
    pub notify_on_update: bool,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("pause_hotkey", &self.pause_hotkey)
            .field(
                "pause_notifications_enabled",
                &self.pause_notifications_enabled,
            )
            .field("pause_audio_enabled", &self.pause_audio_enabled)
            .field("audio_theme", &self.audio_theme)
            .field("audio_volume", &self.audio_volume)
            .field("start_on_boot", &self.start_on_boot)
            .field(
                "inline_tab_completion_enabled",
                &self.inline_tab_completion_enabled,
            )
            .field(
                "inline_case_transform_enabled",
                &self.inline_case_transform_enabled,
            )
            .field("wpm", &self.wpm)
            .field("spinner_style", &self.spinner_style)
            .field("ai_provider", &self.ai_provider)
            .field("ai_model", &self.ai_model)
            .field("ai_custom_endpoint", &self.ai_custom_endpoint)
            .field("inline_ai_enabled", &self.inline_ai_enabled)
            .field(
                "clipboard_restore_delay_ms",
                &self.clipboard_restore_delay_ms,
            )
            .field("instant_expand", &self.instant_expand)
            .field("rpc_mode", &self.rpc_mode)
            .field("rpc_host", &self.rpc_host)
            .field("rpc_port", &self.rpc_port)
            .field("ignore_fullscreen", &self.ignore_fullscreen)
            .field("script_timeout", &self.script_timeout)
            .field("ai_temperature", &self.ai_temperature)
            .field("ai_max_tokens", &self.ai_max_tokens)
            .field("ai_system_prompt", &self.ai_system_prompt)
            .field("auto_update", &self.auto_update)
            .field("clipboard_history_enabled", &self.clipboard_history_enabled)
            .field(
                "clipboard_history_retention_secs",
                &self.clipboard_history_retention_secs,
            )
            .field("inline_emoji_enabled", &self.inline_emoji_enabled)
            .field("inline_emoji_trigger_char", &self.inline_emoji_trigger_char)
            .field("scripts_enabled", &self.scripts_enabled)
            .field("system_tray_enabled", &self.system_tray_enabled)
            .field("inline_datetime_enabled", &self.inline_datetime_enabled)
            .field(
                "inline_datetime_date_format",
                &self.inline_datetime_date_format,
            )
            .field(
                "inline_datetime_time_format",
                &self.inline_datetime_time_format,
            )
            .field(
                "inline_datetime_datetime_format",
                &self.inline_datetime_datetime_format,
            )
            .field("inline_datetime_dialect", &self.inline_datetime_dialect)
            .field(
                "inline_currency_to_words_enabled",
                &self.inline_currency_to_words_enabled,
            )
            .field("inline_dictionary_enabled", &self.inline_dictionary_enabled)
            .field("inline_dictionary_mode", &self.inline_dictionary_mode)
            .field("notify_on_update", &self.notify_on_update)
            .finish()
    }
}

impl Settings {
    pub fn resolve_key(key: &str) -> &str {
        match key {
            "hotkey" => "pause_hotkey",
            "notifications" => "pause_notifications_enabled",
            "pause_audio" => "pause_audio_enabled",
            "audio_theme" | "sound_theme" | "theme" | "audio_pack" | "sound_pack" => "audio_theme",
            "audio_volume" | "sound_volume" | "volume" => "audio_volume",
            "boot" => "start_on_boot",
            "inline_tab_completion" => "inline_tab_completion_enabled",
            "inline_dictionary" | "inline_dictionary_enabled" | "dictionary" => {
                "inline_dictionary_enabled"
            }
            "inline_dictionary_mode" | "dictionary_mode" => "inline_dictionary_mode",
            "inline_case_transform" | "inline_case_transform_enabled" => {
                "inline_case_transform_enabled"
            }
            "wpm" => "wpm",
            "spinner" => "spinner_style",
            "ai_provider" => "ai_provider",
            "ai_model" => "ai_model",
            "inline_ai" | "inline_ai_enabled" | "ai" => "inline_ai_enabled",
            "endpoint" => "ai_custom_endpoint",
            "custom_endpoint" => "ai_custom_endpoint",
            "ai_custom_endpoint" => "ai_custom_endpoint",
            "clipboard_restore_delay_ms" => "clipboard_restore_delay_ms",
            "clipboard_delay" => "clipboard_restore_delay_ms",
            "instant_expand" => "instant_expand",
            "instant" => "instant_expand",
            "ignore_fullscreen" => "ignore_fullscreen",
            "rpc_mode" | "mode" => "rpc_mode",
            "rpc_host" | "host" => "rpc_host",
            "rpc_port" | "port" => "rpc_port",
            "script_timeout" => "script_timeout",
            "ai_temperature" | "temperature" => "ai_temperature",
            "ai_max_tokens" | "max_tokens" => "ai_max_tokens",
            "ai_system_prompt" | "system_prompt" => "ai_system_prompt",
            "clipboard_history" | "clipboard_history_enabled" => "clipboard_history_enabled",
            "clipboard_history_retention" | "clipboard_history_retention_secs" => {
                "clipboard_history_retention_secs"
            }
            "inline_date_time"
            | "inline_date_time_enabled"
            | "inline_datetime"
            | "inline_datetime_enabled" => "inline_datetime_enabled",
            "inline_currency_to_words"
            | "inline_currency_to_words_enabled"
            | "inline_currency_words"
            | "inline_currency_words_enabled" => "inline_currency_to_words_enabled",
            "inline_date_time_date_format" | "inline_datetime_date_format" => {
                "inline_datetime_date_format"
            }
            "inline_date_time_time_format" | "inline_datetime_time_format" => {
                "inline_datetime_time_format"
            }
            "inline_date_time_datetime_format" | "inline_datetime_datetime_format" => {
                "inline_datetime_datetime_format"
            }
            "inline_date_time_dialect" | "inline_datetime_dialect" => "inline_datetime_dialect",
            "inline_emoji" | "inline_emoji_enabled" => "inline_emoji_enabled",
            "inline_emoji_trigger_char" | "emoji_trigger" => "inline_emoji_trigger_char",
            "scripts_enabled" => "scripts_enabled",
            "system_tray" | "system_tray_enabled" | "tray" => "system_tray_enabled",
            "notify_on_update" | "notify_update" | "update_notify" => "notify_on_update",
            other => other,
        }
    }

    pub const fn default_wpm() -> u32 {
        60
    }

    pub const fn sanitize_wpm(wpm: u32) -> u32 {
        if wpm == 0 {
            Self::default_wpm()
        } else if wpm > 150 {
            150
        } else {
            wpm
        }
    }

    pub const fn default_rpc_port() -> u16 {
        50051
    }

    pub const fn sanitize_rpc_port(port: u16) -> u16 {
        if port < 1024 { 1024 } else { port }
    }

    pub fn get_script_timeout() -> Option<std::time::Duration> {
        let timeout = CACHED_SCRIPT_TIMEOUT.load(Ordering::Relaxed);
        if timeout == 0 {
            None
        } else {
            Some(std::time::Duration::from_secs(timeout as u64))
        }
    }

    pub fn default_clipboard_restore_delay_ms() -> u32 {
        static DELAY: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        *DELAY.get_or_init(|| {
            if cfg!(target_os = "windows") {
                let info = os_info::get();
                let is_win10 = match info.version() {
                    os_info::Version::Semantic(major, _, patch) => *major == 10 && *patch < 22000,
                    _ => {
                        let s = info.to_string();
                        s.contains("Windows 10") && !s.contains("Windows 11")
                    }
                };
                if is_win10 { 800 } else { 500 }
            } else if cfg!(target_os = "linux") {
                let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
                if session.eq_ignore_ascii_case("wayland") {
                    600
                } else {
                    400
                }
            } else {
                350
            }
        })
    }

    pub const fn sanitize_clipboard_restore_delay_ms(delay: u32) -> u32 {
        if delay > 2000 { 2000 } else { delay }
    }

    pub const fn sanitize_clipboard_history_retention_secs(secs: u32) -> u32 {
        if secs > 86400 { 86400 } else { secs }
    }

    pub const fn default_audio_volume() -> u32 {
        50
    }

    pub const fn sanitize_audio_volume(volume: u32) -> u32 {
        if volume > 100 { 100 } else { volume }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pause_hotkey: "Alt + `".to_string(),
            pause_notifications_enabled: false,
            pause_audio_enabled: true,
            audio_theme: AudioTheme::default(),
            audio_volume: Self::default_audio_volume(),
            start_on_boot: true,
            inline_tab_completion_enabled: true,
            inline_case_transform_enabled: true,
            wpm: Self::default_wpm(),
            spinner_style: SpinnerStyle::default(),
            ai_provider: None,
            ai_model: None,
            ai_custom_endpoint: None,
            inline_ai_enabled: true,
            clipboard_restore_delay_ms: Self::default_clipboard_restore_delay_ms(),
            instant_expand: false,
            rpc_mode: RpcMode::default(),
            rpc_host: "127.0.0.1".to_string(),
            rpc_port: Self::default_rpc_port(),
            ignore_fullscreen: true,
            script_timeout: 15,
            ai_temperature: None,
            ai_max_tokens: None,
            ai_system_prompt: None,
            auto_update: true,
            clipboard_history_enabled: true,
            clipboard_history_retention_secs: 300,
            inline_emoji_enabled: true,
            inline_emoji_trigger_char: ':',
            scripts_enabled: true,
            system_tray_enabled: true,
            inline_datetime_enabled: true,
            inline_datetime_date_format: "MMMM D, YYYY".to_string(),
            inline_datetime_time_format: "h:mm A".to_string(),
            inline_datetime_datetime_format: "MMMM D, YYYY 'at' h:mm A".to_string(),
            inline_datetime_dialect: "uk".to_string(),
            inline_currency_to_words_enabled: false,
            inline_dictionary_enabled: true,
            inline_dictionary_mode: InlineDictionaryMode::default(),
            notify_on_update: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_restore_delay_ms_clamping() {
        assert_eq!(Settings::sanitize_clipboard_restore_delay_ms(1500), 1500);
        assert_eq!(Settings::sanitize_clipboard_restore_delay_ms(2500), 2000);
    }

    #[test]
    fn test_wpm_clamping() {
        assert_eq!(Settings::sanitize_wpm(0), 60);
        assert_eq!(Settings::sanitize_wpm(120), 120);
        assert_eq!(Settings::sanitize_wpm(200), 150);
    }

    #[test]
    fn test_system_tray_enabled_default_is_true() {
        assert!(Settings::default().system_tray_enabled);
    }

    #[test]
    fn test_inline_currency_to_words_enabled_default_is_false() {
        assert!(!Settings::default().inline_currency_to_words_enabled);
    }

    #[test]
    fn test_resolve_key_inline_currency_to_words() {
        assert_eq!(
            Settings::resolve_key("inline_currency_to_words"),
            "inline_currency_to_words_enabled"
        );
        assert_eq!(
            Settings::resolve_key("inline_currency_to_words_enabled"),
            "inline_currency_to_words_enabled"
        );
        assert_eq!(
            Settings::resolve_key("inline_currency_words"),
            "inline_currency_to_words_enabled"
        );
    }

    #[test]
    fn test_inline_dictionary_enabled_default_is_true() {
        assert!(Settings::default().inline_dictionary_enabled);
    }

    #[test]
    fn test_resolve_key_inline_dictionary() {
        assert_eq!(
            Settings::resolve_key("inline_dictionary"),
            "inline_dictionary_enabled"
        );
        assert_eq!(
            Settings::resolve_key("inline_dictionary_enabled"),
            "inline_dictionary_enabled"
        );
        assert_eq!(
            Settings::resolve_key("dictionary"),
            "inline_dictionary_enabled"
        );
    }

    #[test]
    fn test_inline_dictionary_mode_default_is_lite() {
        assert_eq!(
            Settings::default().inline_dictionary_mode,
            InlineDictionaryMode::Lite
        );
    }

    #[test]
    fn test_resolve_key_inline_dictionary_mode() {
        assert_eq!(
            Settings::resolve_key("inline_dictionary_mode"),
            "inline_dictionary_mode"
        );
        assert_eq!(
            Settings::resolve_key("dictionary_mode"),
            "inline_dictionary_mode"
        );
    }
}
