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

static CACHED_INLINE_DATETIME_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);
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

pub fn set_cached_inline_emoji_enabled(enabled: bool) {
    CACHED_INLINE_EMOJI_ENABLED.store(enabled, Ordering::Relaxed);
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
    reset_setting_to_default, validate_delimiter_conflicts,
};
pub use manager::SettingsManager;

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
pub enum ActionKey {
    Space,
    #[default]
    Enter,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum InlineAiTriggerMode {
    Symmetric,
    #[default]
    Asymmetric,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RpcMode {
    #[default]
    Socket,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub trigger_char: char,
    pub pause_hotkey: String,
    pub pause_notifications_enabled: bool,
    pub pause_audio_enabled: bool,
    pub start_on_boot: bool,
    pub inline_tab_completion_enabled: bool,
    pub inline_history_enabled: bool,
    pub wpm: u32,
    pub spinner_style: SpinnerStyle,
    pub ai_provider: Option<String>,
    pub ai_model: Option<String>,
    pub ai_custom_endpoint: Option<String>,
    pub inline_ai_trigger_mode: InlineAiTriggerMode,
    pub inline_ai_trigger: String,
    pub inline_ai_trigger_open: String,
    pub inline_ai_trigger_close: String,
    pub clipboard_restore_delay_ms: u32,
    pub action_key: ActionKey,
    pub triggerless_mode: bool,
    pub instant_expand: bool,
    pub rpc_mode: RpcMode,
    pub rpc_host: String,
    pub rpc_port: u16,
    pub rpc_token: String,
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
}

impl Settings {
    pub fn resolve_key(key: &str) -> &str {
        match key {
            "trigger" => "trigger_char",
            "hotkey" => "pause_hotkey",
            "notifications" => "pause_notifications_enabled",
            "pause_audio" => "pause_audio_enabled",
            "boot" => "start_on_boot",
            "inline_tab_completion" => "inline_tab_completion_enabled",
            "inline_history" => "inline_history_enabled",
            "wpm" => "wpm",
            "spinner" => "spinner_style",
            "ai_provider" => "ai_provider",
            "ai_model" => "ai_model",
            "inline_ai_trigger_mode" | "ai_delimiter_mode" | "delimiter_mode" => {
                "inline_ai_trigger_mode"
            }
            "inline_ai_trigger" | "ai_symmetric_delimiter" | "symmetric_delimiter" => {
                "inline_ai_trigger"
            }
            "inline_ai_trigger_open" | "ai_open_delimiter" | "open_delimiter" | "delimiter" => {
                "inline_ai_trigger_open"
            }
            "inline_ai_trigger_close" | "ai_close_delimiter" | "close_delimiter" => {
                "inline_ai_trigger_close"
            }
            "endpoint" => "ai_custom_endpoint",
            "custom_endpoint" => "ai_custom_endpoint",
            "ai_custom_endpoint" => "ai_custom_endpoint",
            "clipboard_restore_delay_ms" => "clipboard_restore_delay_ms",
            "clipboard_delay" => "clipboard_restore_delay_ms",
            "action_key" => "action_key",
            "triggerless" => "triggerless_mode",
            "triggerless_mode" => "triggerless_mode",
            "instant_expand" => "instant_expand",
            "instant" => "instant_expand",
            "ignore_fullscreen" => "ignore_fullscreen",
            "rpc_mode" | "mode" => "rpc_mode",
            "rpc_host" | "host" => "rpc_host",
            "rpc_port" | "port" => "rpc_port",
            "rpc_token" | "token" => "rpc_token",
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
            _ => key,
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
                500
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trigger_char: '>',
            pause_hotkey: "Alt + `".to_string(),
            pause_notifications_enabled: true,
            pause_audio_enabled: true,
            start_on_boot: true,
            inline_tab_completion_enabled: true,
            inline_history_enabled: true,
            wpm: Self::default_wpm(),
            spinner_style: SpinnerStyle::default(),
            ai_provider: None,
            ai_model: None,
            ai_custom_endpoint: None,
            inline_ai_trigger_mode: InlineAiTriggerMode::default(),
            inline_ai_trigger: "^".to_string(),
            inline_ai_trigger_open: ">>".to_string(),
            inline_ai_trigger_close: "<<".to_string(),
            clipboard_restore_delay_ms: Self::default_clipboard_restore_delay_ms(),
            action_key: ActionKey::default(),
            triggerless_mode: true,
            instant_expand: false,
            rpc_mode: RpcMode::default(),
            rpc_host: "127.0.0.1".to_string(),
            rpc_port: Self::default_rpc_port(),
            rpc_token: "".to_string(),
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
}
