use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};

static CACHED_AUDIO_THEME: parking_lot::RwLock<AudioTheme> =
    parking_lot::RwLock::new(AudioTheme::Minimal);
static CACHED_AUDIO_VOLUME: AtomicU32 = AtomicU32::new(50);
static CACHED_INLINE_DATE_FORMAT: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_INLINE_TIME_FORMAT: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_INLINE_DATETIME_FORMAT: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_VOICE_PTT_HOTKEY: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_VOICE_HANDSFREE_HOTKEY: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_VOICE_DICTIONARY: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_VOICE_PREFERRED_LANGUAGES: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_VOICE_INPUT_DEVICE: parking_lot::RwLock<Option<String>> =
    parking_lot::RwLock::new(None);
static CACHED_PAUSE_MEDIA_WHILE_DICTATING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
static CACHED_INLINE_DICTIONARY_MODE: parking_lot::RwLock<InlineDictionaryMode> =
    parking_lot::RwLock::new(InlineDictionaryMode::Lite);

// Bumped on every cached-settings write so background loops can poll this
// single counter instead of re-reading the database while idle.
static SETTINGS_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn settings_version() -> u64 {
    SETTINGS_VERSION.load(Ordering::Relaxed)
}

pub(crate) fn bump_settings_version() {
    SETTINGS_VERSION.fetch_add(1, Ordering::Relaxed);
}

pub fn set_cached_audio_theme(theme: AudioTheme) {
    *CACHED_AUDIO_THEME.write() = theme;
    bump_settings_version();
}

pub fn get_cached_audio_theme() -> AudioTheme {
    *CACHED_AUDIO_THEME.read()
}

pub fn set_cached_audio_volume(volume: u32) {
    CACHED_AUDIO_VOLUME.store(Settings::sanitize_audio_volume(volume), Ordering::Relaxed);
    bump_settings_version();
}

pub fn get_cached_audio_volume() -> u32 {
    CACHED_AUDIO_VOLUME.load(Ordering::Relaxed)
}

pub fn set_cached_inline_date_format(f: String) {
    *CACHED_INLINE_DATE_FORMAT.write() = Some(f);
    bump_settings_version();
}

pub fn get_cached_inline_date_format() -> String {
    CACHED_INLINE_DATE_FORMAT
        .read()
        .clone()
        .unwrap_or_else(|| "MMMM D, YYYY".to_string())
}

pub fn set_cached_inline_time_format(f: String) {
    *CACHED_INLINE_TIME_FORMAT.write() = Some(f);
    bump_settings_version();
}

pub fn get_cached_inline_time_format() -> String {
    CACHED_INLINE_TIME_FORMAT
        .read()
        .clone()
        .unwrap_or_else(|| "h:mm A".to_string())
}

pub fn set_cached_inline_datetime_format(f: String) {
    *CACHED_INLINE_DATETIME_FORMAT.write() = Some(f);
    bump_settings_version();
}

pub fn get_cached_inline_datetime_format() -> String {
    CACHED_INLINE_DATETIME_FORMAT
        .read()
        .clone()
        .unwrap_or_else(|| "MMMM D, YYYY 'at' h:mm A".to_string())
}

pub fn set_cached_inline_datetime_date_format(f: String) {
    set_cached_inline_date_format(f);
}

pub fn get_cached_inline_datetime_date_format() -> String {
    get_cached_inline_date_format()
}

pub fn set_cached_inline_datetime_time_format(f: String) {
    set_cached_inline_time_format(f);
}

pub fn get_cached_inline_datetime_time_format() -> String {
    get_cached_inline_time_format()
}

pub fn set_cached_inline_datetime_datetime_format(f: String) {
    set_cached_inline_datetime_format(f);
}

pub fn get_cached_inline_datetime_datetime_format() -> String {
    get_cached_inline_datetime_format()
}

pub fn set_cached_inline_datetime_dialect(_d: String) {}

pub fn get_cached_inline_datetime_dialect() -> String {
    crate::engine::dates::detect_locale_dialect().to_string()
}

// Deprecated forwarders for removed settings caches
pub fn set_cached_scripts_enabled(_enabled: bool) {}
pub fn get_cached_scripts_enabled() -> bool {
    true
}
pub fn set_cached_inline_emoji_enabled(_enabled: bool) {}
pub fn get_cached_inline_emoji_enabled() -> bool {
    true
}
pub fn set_cached_inline_emoji_trigger_char(_c: char) {}
pub fn get_cached_inline_emoji_trigger_char() -> char {
    ':'
}
pub fn set_cached_inline_case_transform_enabled(_enabled: bool) {}
pub fn get_cached_inline_case_transform_enabled() -> bool {
    true
}
pub fn set_cached_script_timeout(_timeout: u32) {}
pub fn set_cached_clipboard_restore_delay(_delay: u32) {}
pub fn get_cached_clipboard_restore_delay() -> u32 {
    Settings::default_clipboard_restore_delay_ms()
}
pub fn set_cached_wpm(_wpm: u32) {
    bump_settings_version();
}
pub fn get_cached_wpm() -> u32 {
    Settings::default_wpm()
}
pub fn set_cached_clipboard_history_enabled(_enabled: bool) {}
pub fn get_cached_clipboard_history_enabled() -> bool {
    true
}
pub fn set_cached_clipboard_history_retention_secs(_secs: u32) {}
pub fn get_cached_clipboard_history_retention_secs() -> u32 {
    300
}
pub fn set_cached_inline_datetime_enabled(_enabled: bool) {}
pub fn get_cached_inline_datetime_enabled() -> bool {
    true
}
pub fn set_cached_inline_currency_to_words_enabled(_enabled: bool) {}
pub fn get_cached_inline_currency_to_words_enabled() -> bool {
    true
}
pub fn set_cached_inline_dictionary_enabled(_enabled: bool) {}
pub fn get_cached_inline_dictionary_enabled() -> bool {
    true
}
pub fn set_cached_inline_dictionary_mode(mode: InlineDictionaryMode) {
    *CACHED_INLINE_DICTIONARY_MODE.write() = mode;
    bump_settings_version();
}
pub fn get_cached_inline_dictionary_mode() -> InlineDictionaryMode {
    *CACHED_INLINE_DICTIONARY_MODE.read()
}

pub fn set_cached_voice_ptt_hotkey(hotkey: String) {
    *CACHED_VOICE_PTT_HOTKEY.write() = Some(hotkey);
    bump_settings_version();
}

pub fn get_cached_voice_ptt_hotkey() -> String {
    CACHED_VOICE_PTT_HOTKEY
        .read()
        .clone()
        .unwrap_or_else(|| "win+lctrl".to_string())
}

pub fn set_cached_voice_handsfree_hotkey(hotkey: String) {
    *CACHED_VOICE_HANDSFREE_HOTKEY.write() = Some(hotkey);
    bump_settings_version();
}

pub fn get_cached_voice_handsfree_hotkey() -> String {
    CACHED_VOICE_HANDSFREE_HOTKEY
        .read()
        .clone()
        .unwrap_or_else(|| "win+lctrl+lalt".to_string())
}

pub fn set_cached_voice_dictionary(dict: String) {
    *CACHED_VOICE_DICTIONARY.write() = Some(dict);
    bump_settings_version();
}

pub fn get_cached_voice_dictionary() -> String {
    CACHED_VOICE_DICTIONARY.read().clone().unwrap_or_default()
}

pub fn set_cached_voice_preferred_languages(langs: String) {
    *CACHED_VOICE_PREFERRED_LANGUAGES.write() = Some(langs);
    bump_settings_version();
}

pub fn get_cached_voice_preferred_languages() -> String {
    CACHED_VOICE_PREFERRED_LANGUAGES
        .read()
        .clone()
        .unwrap_or_else(|| "en".to_string())
}

pub fn set_cached_voice_input_device(device: Option<String>) {
    *CACHED_VOICE_INPUT_DEVICE.write() = device;
    bump_settings_version();
}

pub fn get_cached_voice_input_device() -> Option<String> {
    CACHED_VOICE_INPUT_DEVICE.read().clone()
}

pub fn set_cached_pause_media_while_dictating(enabled: bool) {
    CACHED_PAUSE_MEDIA_WHILE_DICTATING.store(enabled, Ordering::Relaxed);
    bump_settings_version();
}

pub fn get_cached_pause_media_while_dictating() -> bool {
    CACHED_PAUSE_MEDIA_WHILE_DICTATING.load(Ordering::Relaxed)
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
    Arcade,
    Mechanical,
    Organic,
    Scifi,
    Rubber,
    Zen,
}

impl AudioTheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Arcade => "arcade",
            Self::Mechanical => "mechanical",
            Self::Organic => "organic",
            Self::Scifi => "scifi",
            Self::Rubber => "rubber",
            Self::Zen => "zen",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::Minimal,
            Self::Arcade,
            Self::Mechanical,
            Self::Organic,
            Self::Scifi,
            Self::Rubber,
            Self::Zen,
        ]
    }

    pub const ALL_NAMES: [&'static str; 7] = [
        "minimal",
        "arcade",
        "mechanical",
        "organic",
        "scifi",
        "rubber",
        "zen",
    ];
}

impl std::str::FromStr for AudioTheme {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "minimal" | "default" => Ok(Self::Minimal),
            "arcade" => Ok(Self::Arcade),
            "mechanical" | "typewriter" => Ok(Self::Mechanical),
            "organic" => Ok(Self::Organic),
            "scifi" | "sci-fi" => Ok(Self::Scifi),
            "rubber" => Ok(Self::Rubber),
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
pub enum TuiCursorStyle {
    Block,
    #[default]
    Bar,
    Underscore,
}

impl TuiCursorStyle {
    pub const ALL: [Self; 3] = [Self::Block, Self::Bar, Self::Underscore];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Bar => "bar",
            Self::Underscore => "underscore",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum InlineDictionaryMode {
    #[default]
    Lite,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
    PauseHotkey,
    StartOnBoot,
    AutoUpdate,
    InstantExpand,
    IgnoreFullscreen,
    SystemTrayEnabled,
    InlineDateFormat,
    InlineTimeFormat,
    InlineDatetimeFormat,
    AudioTheme,
    AudioVolume,
    AiProvider,
    AiModel,
    AiCustomEndpoint,
    VoicePttHotkey,
    VoiceHandsfreeHotkey,
    VoiceDictionary,
    VoicePreferredLanguages,
    VoiceInputDevice,
    PauseMediaWhileDictating,
    TuiCursorStyle,
}

impl SettingKey {
    pub const ALL: [Self; 21] = [
        Self::PauseHotkey,
        Self::StartOnBoot,
        Self::AutoUpdate,
        Self::InstantExpand,
        Self::IgnoreFullscreen,
        Self::SystemTrayEnabled,
        Self::InlineDateFormat,
        Self::InlineTimeFormat,
        Self::InlineDatetimeFormat,
        Self::AudioTheme,
        Self::AudioVolume,
        Self::AiProvider,
        Self::AiModel,
        Self::AiCustomEndpoint,
        Self::VoicePttHotkey,
        Self::VoiceHandsfreeHotkey,
        Self::VoiceDictionary,
        Self::VoicePreferredLanguages,
        Self::VoiceInputDevice,
        Self::PauseMediaWhileDictating,
        Self::TuiCursorStyle,
    ];

    /// Whether the key may be read or changed through the CLI. TUI-only
    /// settings are configured exclusively in TUI settings.
    pub const fn cli_visible(self) -> bool {
        !matches!(self, Self::TuiCursorStyle)
    }

    pub const fn storage_key(self) -> &'static str {
        match self {
            Self::PauseHotkey => "pause_hotkey",
            Self::StartOnBoot => "start_on_boot",
            Self::AutoUpdate => "auto_update",
            Self::InstantExpand => "instant_expand",
            Self::IgnoreFullscreen => "ignore_fullscreen",
            Self::SystemTrayEnabled => "system_tray_enabled",
            Self::InlineDateFormat => "inline_date_format",
            Self::InlineTimeFormat => "inline_time_format",
            Self::InlineDatetimeFormat => "inline_datetime_format",
            Self::AudioTheme => "audio_theme",
            Self::AudioVolume => "audio_volume",
            Self::AiProvider => "ai_provider",
            Self::AiModel => "ai_model",
            Self::AiCustomEndpoint => "ai_custom_endpoint",
            Self::VoicePttHotkey => "voice_ptt_hotkey",
            Self::VoiceHandsfreeHotkey => "voice_handsfree_hotkey",
            Self::VoiceDictionary => "voice_dictionary",
            Self::VoicePreferredLanguages => "voice_preferred_languages",
            Self::VoiceInputDevice => "voice_input_device",
            Self::PauseMediaWhileDictating => "pause_media_while_dictating",
            Self::TuiCursorStyle => "tui_cursor_style",
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub pause_hotkey: String,
    pub start_on_boot: bool,
    pub auto_update: bool,
    pub instant_expand: bool,
    pub ignore_fullscreen: bool,
    pub system_tray_enabled: bool,
    pub inline_date_format: String,
    pub inline_time_format: String,
    pub inline_datetime_format: String,
    pub audio_theme: AudioTheme,
    pub audio_volume: u32,
    pub ai_provider: Option<String>,
    pub ai_model: Option<String>,
    pub ai_custom_endpoint: Option<String>,
    pub voice_ptt_hotkey: String,
    pub voice_handsfree_hotkey: String,
    pub voice_dictionary: String,
    pub voice_preferred_languages: String,
    pub voice_input_device: Option<String>,
    pub pause_media_while_dictating: bool,
    pub tui_cursor_style: TuiCursorStyle,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("pause_hotkey", &self.pause_hotkey)
            .field("start_on_boot", &self.start_on_boot)
            .field("auto_update", &self.auto_update)
            .field("instant_expand", &self.instant_expand)
            .field("ignore_fullscreen", &self.ignore_fullscreen)
            .field("system_tray_enabled", &self.system_tray_enabled)
            .field("inline_date_format", &self.inline_date_format)
            .field("inline_time_format", &self.inline_time_format)
            .field("inline_datetime_format", &self.inline_datetime_format)
            .field("audio_theme", &self.audio_theme)
            .field("audio_volume", &self.audio_volume)
            .field("ai_provider", &self.ai_provider)
            .field("ai_model", &self.ai_model)
            .field("ai_custom_endpoint", &self.ai_custom_endpoint)
            .field("voice_ptt_hotkey", &self.voice_ptt_hotkey)
            .field("voice_handsfree_hotkey", &self.voice_handsfree_hotkey)
            .field("voice_dictionary", &self.voice_dictionary)
            .field("voice_preferred_languages", &self.voice_preferred_languages)
            .field("voice_input_device", &self.voice_input_device)
            .field(
                "pause_media_while_dictating",
                &self.pause_media_while_dictating,
            )
            .field("tui_cursor_style", &self.tui_cursor_style)
            .finish()
    }
}

impl Settings {
    pub const ALL_KEYS: [&'static str; 21] = [
        "pause_hotkey",
        "start_on_boot",
        "auto_update",
        "instant_expand",
        "ignore_fullscreen",
        "system_tray_enabled",
        "inline_date_format",
        "inline_time_format",
        "inline_datetime_format",
        "audio_theme",
        "audio_volume",
        "ai_provider",
        "ai_model",
        "ai_custom_endpoint",
        "voice_ptt_hotkey",
        "voice_handsfree_hotkey",
        "voice_dictionary",
        "voice_preferred_languages",
        "voice_input_device",
        "pause_media_while_dictating",
        "tui_cursor_style",
    ];

    pub fn resolve_key(key: &str) -> &str {
        match key {
            "hotkey" => "pause_hotkey",
            "boot" => "start_on_boot",
            "instant" => "instant_expand",
            "system_tray" | "tray" => "system_tray_enabled",
            "date_format" | "inline_datetime_date_format" | "inline_date_time_date_format" => {
                "inline_date_format"
            }
            "time_format" | "inline_datetime_time_format" | "inline_date_time_time_format" => {
                "inline_time_format"
            }
            "datetime_format"
            | "date_time_format"
            | "inline_datetime_datetime_format"
            | "inline_date_time_datetime_format" => "inline_datetime_format",
            "audio_theme" | "sound_theme" | "audio_pack" | "sound_pack" => "audio_theme",
            "audio_volume" | "sound_volume" | "volume" => "audio_volume",
            "ai_provider" => "ai_provider",
            "ai_model" => "ai_model",
            "endpoint" | "custom_endpoint" | "ai_custom_endpoint" => "ai_custom_endpoint",
            "voice_ptt_hotkey" | "voice_ptt" | "ptt_hotkey" | "ptt" => "voice_ptt_hotkey",
            "voice_handsfree_hotkey" | "voice_handsfree" | "handsfree_hotkey" | "handsfree" => {
                "voice_handsfree_hotkey"
            }
            "voice_dictionary" | "voice_vocab" | "voice_words" => "voice_dictionary",
            "voice_preferred_languages"
            | "preferred_languages"
            | "voice_languages"
            | "languages"
            | "langs" => "voice_preferred_languages",
            "voice_input_device" | "voice_device" | "voice_mic" | "input_device" | "mic"
            | "microphone" => "voice_input_device",
            "pause_media_while_dictating" | "pause_media" => "pause_media_while_dictating",
            "tui_cursor_style" | "cursor_style" | "cursor" => "tui_cursor_style",
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

    pub fn get_script_timeout() -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs(15))
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
            start_on_boot: true,
            auto_update: true,
            instant_expand: false,
            ignore_fullscreen: true,
            system_tray_enabled: true,
            inline_date_format: "MMMM D, YYYY".to_string(),
            inline_time_format: "h:mm A".to_string(),
            inline_datetime_format: "MMMM D, YYYY 'at' h:mm A".to_string(),
            audio_theme: AudioTheme::default(),
            audio_volume: Self::default_audio_volume(),
            ai_provider: None,
            ai_model: None,
            ai_custom_endpoint: None,
            voice_ptt_hotkey: "win+lctrl".to_string(),
            voice_handsfree_hotkey: "win+lctrl+lalt".to_string(),
            voice_dictionary: String::new(),
            voice_preferred_languages: "en".to_string(),
            voice_input_device: None,
            pause_media_while_dictating: true,
            tui_cursor_style: TuiCursorStyle::default(),
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
    fn test_inline_date_time_format_forwarders() {
        assert_eq!(
            get_cached_inline_date_format(),
            get_cached_inline_datetime_date_format()
        );
        assert_eq!(
            get_cached_inline_time_format(),
            get_cached_inline_datetime_time_format()
        );
        assert_eq!(
            get_cached_inline_datetime_format(),
            get_cached_inline_datetime_datetime_format()
        );
    }

    #[test]
    fn test_system_tray_enabled_default_is_true() {
        assert!(Settings::default().system_tray_enabled);
    }

    #[test]
    fn test_voice_settings_defaults() {
        let defaults = Settings::default();
        assert_eq!(defaults.voice_ptt_hotkey, "win+lctrl");
        assert_eq!(defaults.voice_handsfree_hotkey, "win+lctrl+lalt");
        assert!(defaults.voice_dictionary.is_empty());
        assert_eq!(defaults.voice_preferred_languages, "en");
        assert_eq!(defaults.voice_input_device, None);
    }

    #[test]
    fn test_resolve_key_date_time_formats() {
        assert_eq!(Settings::resolve_key("date_format"), "inline_date_format");
        assert_eq!(
            Settings::resolve_key("inline_datetime_date_format"),
            "inline_date_format"
        );
        assert_eq!(
            Settings::resolve_key("inline_date_time_date_format"),
            "inline_date_format"
        );
        assert_eq!(Settings::resolve_key("time_format"), "inline_time_format");
        assert_eq!(
            Settings::resolve_key("inline_datetime_time_format"),
            "inline_time_format"
        );
        assert_eq!(
            Settings::resolve_key("inline_date_time_time_format"),
            "inline_time_format"
        );
        assert_eq!(
            Settings::resolve_key("datetime_format"),
            "inline_datetime_format"
        );
        assert_eq!(
            Settings::resolve_key("date_time_format"),
            "inline_datetime_format"
        );
        assert_eq!(
            Settings::resolve_key("inline_datetime_datetime_format"),
            "inline_datetime_format"
        );
        assert_eq!(
            Settings::resolve_key("inline_date_time_datetime_format"),
            "inline_datetime_format"
        );
    }

    #[test]
    fn pause_media_while_dictating_defaults_on_and_key_registered() {
        assert!(Settings::default().pause_media_while_dictating);
        assert!(Settings::ALL_KEYS.contains(&"pause_media_while_dictating"));
        assert_eq!(
            Settings::resolve_key("pause_media"),
            "pause_media_while_dictating"
        );
        assert!(get_cached_pause_media_while_dictating());
    }

    #[test]
    fn tui_cursor_style_defaults_to_bar_and_key_registered() {
        assert_eq!(Settings::default().tui_cursor_style, TuiCursorStyle::Bar);
        assert_eq!(TuiCursorStyle::default(), TuiCursorStyle::Bar);
        assert_eq!(TuiCursorStyle::Bar.as_str(), "bar");
        assert!(Settings::ALL_KEYS.contains(&"tui_cursor_style"));
        assert_eq!(Settings::resolve_key("cursor_style"), "tui_cursor_style");
        assert_eq!(Settings::resolve_key("cursor"), "tui_cursor_style");
        assert!(!SettingKey::TuiCursorStyle.cli_visible());
        assert_eq!(
            SettingKey::ALL
                .iter()
                .filter(|key| !key.cli_visible())
                .count(),
            1
        );
    }

    #[test]
    fn test_resolve_key_voice_settings() {
        assert_eq!(
            Settings::resolve_key("voice_ptt_hotkey"),
            "voice_ptt_hotkey"
        );
        assert_eq!(Settings::resolve_key("ptt"), "voice_ptt_hotkey");
        assert_eq!(
            Settings::resolve_key("voice_handsfree_hotkey"),
            "voice_handsfree_hotkey"
        );
        assert_eq!(Settings::resolve_key("handsfree"), "voice_handsfree_hotkey");
        assert_eq!(
            Settings::resolve_key("voice_dictionary"),
            "voice_dictionary"
        );
        assert_eq!(Settings::resolve_key("voice_vocab"), "voice_dictionary");
        assert_eq!(
            Settings::resolve_key("voice_preferred_languages"),
            "voice_preferred_languages"
        );
        assert_eq!(
            Settings::resolve_key("preferred_languages"),
            "voice_preferred_languages"
        );
        assert_eq!(
            Settings::resolve_key("voice_languages"),
            "voice_preferred_languages"
        );
        assert_eq!(
            Settings::resolve_key("languages"),
            "voice_preferred_languages"
        );
        assert_eq!(Settings::resolve_key("langs"), "voice_preferred_languages");
        assert_eq!(
            Settings::resolve_key("voice_input_device"),
            "voice_input_device"
        );
        assert_eq!(Settings::resolve_key("voice_device"), "voice_input_device");
        assert_eq!(Settings::resolve_key("mic"), "voice_input_device");
        assert_eq!(Settings::resolve_key("microphone"), "voice_input_device");
    }

    #[test]
    fn test_script_timeout_returns_constant_15_seconds() {
        set_cached_script_timeout(0);
        assert_eq!(
            Settings::get_script_timeout(),
            Some(std::time::Duration::from_secs(15))
        );
        set_cached_script_timeout(60);
        assert_eq!(
            Settings::get_script_timeout(),
            Some(std::time::Duration::from_secs(15))
        );
    }
}
