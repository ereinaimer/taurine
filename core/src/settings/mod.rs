use serde::{Deserialize, Serialize};

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
pub enum SpinnerStyle {
    #[default]
    Braille,
    Arc,
    Classic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActionDelimiter {
    Space,
    #[default]
    Enter,
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
    pub inline_ai_delimiter: char,
    pub clipboard_restore_delay_ms: u32,
    pub action_delimiter: ActionDelimiter,
    pub triggerless_mode: bool,
    pub rpc_port: u16,
    pub ignore_fullscreen: bool,
    pub script_timeout: u32,
    pub ai_temperature: Option<f32>,
    pub ai_max_tokens: Option<u32>,
    pub ai_system_prompt: Option<String>,
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
            "inline_ai_delimiter" => "inline_ai_delimiter",
            "delimiter" => "inline_ai_delimiter",
            "endpoint" => "ai_custom_endpoint",
            "custom_endpoint" => "ai_custom_endpoint",
            "ai_custom_endpoint" => "ai_custom_endpoint",
            "clipboard_restore_delay_ms" => "clipboard_restore_delay_ms",
            "clipboard_delay" => "clipboard_restore_delay_ms",
            "action_delimiter" => "action_delimiter",
            "triggerless" => "triggerless_mode",
            "triggerless_mode" => "triggerless_mode",
            "ignore_fullscreen" => "ignore_fullscreen",
            "rpc_port" | "port" => "rpc_port",
            "script_timeout" => "script_timeout",
            "ai_temperature" | "temperature" => "ai_temperature",
            "ai_max_tokens" | "max_tokens" => "ai_max_tokens",
            "ai_system_prompt" | "system_prompt" => "ai_system_prompt",
            _ => key,
        }
    }

    pub const fn default_wpm() -> u32 {
        60
    }

    pub const fn sanitize_wpm(wpm: u32) -> u32 {
        if wpm == 0 { Self::default_wpm() } else { wpm }
    }

    pub const fn default_rpc_port() -> u16 {
        50051
    }

    pub const fn sanitize_rpc_port(port: u16) -> u16 {
        if port < 1024 { 1024 } else { port }
    }

    pub fn get_script_timeout() -> Option<std::time::Duration> {
        if let Ok(conn) = rusqlite::Connection::open(crate::paths::get_db_path()) {
            let manager = crate::settings::SettingsManager::new(&conn);
            let timeout = manager.load_all().script_timeout;
            if timeout == 0 {
                None
            } else {
                Some(std::time::Duration::from_secs(timeout as u64))
            }
        } else {
            Some(std::time::Duration::from_secs(15))
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
                if is_win10 { 450 } else { 220 }
            } else if cfg!(target_os = "linux") {
                300
            } else {
                160
            }
        })
    }

    pub const fn sanitize_clipboard_restore_delay_ms(delay: u32) -> u32 {
        if delay > 2000 { 2000 } else { delay }
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
            inline_ai_delimiter: '`',
            clipboard_restore_delay_ms: Self::default_clipboard_restore_delay_ms(),
            action_delimiter: ActionDelimiter::default(),
            triggerless_mode: true,
            rpc_port: Self::default_rpc_port(),
            ignore_fullscreen: true,
            script_timeout: 15,
            ai_temperature: None,
            ai_max_tokens: None,
            ai_system_prompt: None,
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
}
