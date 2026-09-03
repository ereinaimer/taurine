use super::super::{InlineDictionaryMode, RpcMode, Settings, SpinnerStyle};
use crate::error::{Error, Result};

pub fn default_setting_input(key: &str) -> Result<Option<String>> {
    let actual_key = Settings::resolve_key(key);
    let defaults = Settings::default();

    match actual_key {
        "pause_hotkey" => Ok(Some(defaults.pause_hotkey)),
        "pause_notifications_enabled" => Ok(Some(defaults.pause_notifications_enabled.to_string())),
        "pause_audio_enabled" => Ok(Some(defaults.pause_audio_enabled.to_string())),
        "audio_theme" => Ok(Some(defaults.audio_theme.as_str().to_string())),
        "audio_volume" => Ok(Some(defaults.audio_volume.to_string())),
        "start_on_boot" => Ok(Some(defaults.start_on_boot.to_string())),
        "inline_tab_completion_enabled" => {
            Ok(Some(defaults.inline_tab_completion_enabled.to_string()))
        }
        "inline_case_transform_enabled" => {
            Ok(Some(defaults.inline_case_transform_enabled.to_string()))
        }
        "wpm" => Ok(Some(defaults.wpm.to_string())),
        "spinner_style" => Ok(Some(match defaults.spinner_style {
            SpinnerStyle::Classic => "classic".to_string(),
            SpinnerStyle::Braille => "braille".to_string(),
            SpinnerStyle::Arc => "arc".to_string(),
        })),
        "ai_provider" => Ok(defaults.ai_provider),
        "ai_model" => Ok(defaults.ai_model),
        "ai_custom_endpoint" => Ok(defaults.ai_custom_endpoint),
        "inline_ai_enabled" => Ok(Some(defaults.inline_ai_enabled.to_string())),
        "clipboard_restore_delay_ms" => Ok(Some(defaults.clipboard_restore_delay_ms.to_string())),
        "instant_expand" => Ok(Some(defaults.instant_expand.to_string())),
        "ignore_fullscreen" => Ok(Some(defaults.ignore_fullscreen.to_string())),
        "rpc_port" => Ok(Some(defaults.rpc_port.to_string())),
        "rpc_mode" => Ok(Some(match defaults.rpc_mode {
            RpcMode::Socket => "socket".to_string(),
            RpcMode::Tcp => "tcp".to_string(),
        })),
        "rpc_host" => Ok(Some(defaults.rpc_host)),
        "script_timeout" => Ok(Some(defaults.script_timeout.to_string())),
        "ai_temperature" => Ok(defaults.ai_temperature.map(|v| v.to_string())),
        "ai_max_tokens" => Ok(defaults.ai_max_tokens.map(|v| v.to_string())),
        "ai_system_prompt" => Ok(defaults.ai_system_prompt),
        "auto_update" => Ok(Some(defaults.auto_update.to_string())),
        "clipboard_history_enabled" => Ok(Some(defaults.clipboard_history_enabled.to_string())),
        "clipboard_history_retention_secs" => {
            Ok(Some(defaults.clipboard_history_retention_secs.to_string()))
        }
        "inline_datetime_enabled" => Ok(Some(defaults.inline_datetime_enabled.to_string())),
        "inline_datetime_date_format" => Ok(Some(defaults.inline_datetime_date_format.clone())),
        "inline_datetime_time_format" => Ok(Some(defaults.inline_datetime_time_format.clone())),
        "inline_datetime_datetime_format" => {
            Ok(Some(defaults.inline_datetime_datetime_format.clone()))
        }
        "inline_datetime_dialect" => Ok(Some(defaults.inline_datetime_dialect.clone())),
        "inline_emoji_enabled" => Ok(Some(defaults.inline_emoji_enabled.to_string())),
        "inline_emoji_trigger_char" => Ok(Some(defaults.inline_emoji_trigger_char.to_string())),
        "scripts_enabled" => Ok(Some(defaults.scripts_enabled.to_string())),
        "system_tray_enabled" => Ok(Some(defaults.system_tray_enabled.to_string())),
        "inline_currency_to_words_enabled" => {
            Ok(Some(defaults.inline_currency_to_words_enabled.to_string()))
        }
        "inline_dictionary_enabled" => Ok(Some(defaults.inline_dictionary_enabled.to_string())),
        "inline_dictionary_mode" => Ok(Some(match defaults.inline_dictionary_mode {
            InlineDictionaryMode::Lite => "lite".to_string(),
            InlineDictionaryMode::Full => "full".to_string(),
        })),
        "notify_on_update" => Ok(Some(defaults.notify_on_update.to_string())),
        _ => Err(Error::Config(format!("unknown setting: {actual_key}"))),
    }
}
