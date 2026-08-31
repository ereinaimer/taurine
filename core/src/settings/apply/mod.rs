use super::{InlineAiTriggerMode, Settings, SettingsManager, SpinnerStyle};
use crate::{
    ai::AiProvider,
    error::{Error, Result},
    keys::parse_hotkey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApplySettingOutcome {
    pub sync_boot: Option<bool>,
}

pub fn reset_setting_to_default(key: &str) -> Result<()> {
    let default_value = default_setting_input(key)?;
    apply_setting_input(key, default_value.as_deref())
}

pub fn apply_setting_input_with_manager(
    manager: &SettingsManager<'_>,
    key: &str,
    value: Option<&str>,
) -> Result<ApplySettingOutcome> {
    let actual_key = Settings::resolve_key(key);

    let outcome = match actual_key {
        "pause_hotkey" => {
            let hotkey = require_non_empty(value, actual_key)?;
            parse_hotkey(hotkey).map_err(|error| {
                Error::Config(format!("invalid pause_hotkey '{hotkey}': {error}"))
            })?;
            manager.update_setting(actual_key, hotkey.to_string())?;
            ApplySettingOutcome::default()
        }
        "pause_notifications_enabled"
        | "pause_audio_enabled"
        | "inline_tab_completion_enabled"
        | "inline_case_transform_enabled"
        | "ignore_fullscreen"
        | "scripts_enabled"
        | "system_tray_enabled" => {
            manager.update_setting(
                actual_key,
                parse_boolean_setting_value(require_non_empty(value, actual_key)?)?,
            )?;
            ApplySettingOutcome::default()
        }
        "audio_theme" => {
            let theme_str = require_non_empty(value, actual_key)?;
            let theme: super::AudioTheme = theme_str.parse().map_err(Error::Config)?;
            crate::settings::set_cached_audio_theme(theme);
            manager.update_setting(actual_key, theme)?;
            ApplySettingOutcome::default()
        }
        "audio_volume" => {
            let raw_value = require_non_empty(value, actual_key)?;
            let parsed = raw_value
                .parse::<u32>()
                .map_err(|_| Error::Config(format!("bad audio_volume value: {raw_value}")))?;
            let sanitized = Settings::sanitize_audio_volume(parsed);
            crate::settings::set_cached_audio_volume(sanitized);
            manager.update_setting(actual_key, sanitized)?;
            ApplySettingOutcome::default()
        }
        "start_on_boot" => {
            let enabled = parse_boolean_setting_value(require_non_empty(value, actual_key)?)?;
            manager.update_setting(actual_key, enabled)?;
            ApplySettingOutcome {
                sync_boot: Some(enabled),
            }
        }
        "wpm" => {
            let raw_value = require_non_empty(value, actual_key)?;
            let parsed = raw_value
                .parse::<u32>()
                .map_err(|_| Error::Config(format!("bad WPM value: {raw_value}")))?;
            manager.update_setting(actual_key, Settings::sanitize_wpm(parsed))?;
            ApplySettingOutcome::default()
        }
        "spinner_style" => {
            manager.update_setting(
                actual_key,
                parse_spinner_style(require_non_empty(value, actual_key)?)?,
            )?;
            ApplySettingOutcome::default()
        }
        "ai_provider" => {
            let provider = if let Some(provider) = value.map(str::trim).filter(|v| !v.is_empty()) {
                Some(AiProvider::try_from(provider)?.as_str().to_string())
            } else {
                None
            };
            manager.update_setting(actual_key, provider)?;
            ApplySettingOutcome::default()
        }
        "ai_model" => {
            let model = value
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(str::to_string);
            manager.update_setting(actual_key, model)?;
            ApplySettingOutcome::default()
        }
        "ai_custom_endpoint" => {
            let endpoint = value
                .map(str::trim)
                .filter(|endpoint| !endpoint.is_empty())
                .map(str::to_string);
            manager.update_setting(actual_key, endpoint)?;
            ApplySettingOutcome::default()
        }
        "inline_ai_trigger_mode" => {
            let mode_str = require_non_empty(value, actual_key)?;
            let current = manager.load_all();
            validate_delimiter_conflicts(&current, actual_key, mode_str)?;
            manager.update_setting(actual_key, parse_inline_ai_trigger_mode(mode_str)?)?;
            ApplySettingOutcome::default()
        }
        "inline_ai_trigger_open" | "inline_ai_trigger_close" | "inline_ai_trigger" => {
            let parsed = require_non_empty(value, actual_key)?;
            let current = manager.load_all();
            validate_delimiter_conflicts(&current, actual_key, parsed)?;
            manager.update_setting(actual_key, parsed.to_string())?;
            ApplySettingOutcome::default()
        }
        "clipboard_restore_delay_ms" => {
            let raw_value = require_non_empty(value, actual_key)?;
            let parsed = raw_value
                .parse::<u32>()
                .map_err(|_| Error::Config(format!("bad delay value: {raw_value}")))?;
            manager.update_setting(
                actual_key,
                Settings::sanitize_clipboard_restore_delay_ms(parsed),
            )?;
            ApplySettingOutcome::default()
        }
        "instant_expand" => {
            manager.update_setting(
                actual_key,
                parse_boolean_setting_value(require_non_empty(value, actual_key)?)?,
            )?;
            ApplySettingOutcome::default()
        }
        "auto_update" | "notify_on_update" => {
            manager.update_setting(
                actual_key,
                parse_boolean_setting_value(require_non_empty(value, actual_key)?)?,
            )?;
            ApplySettingOutcome::default()
        }
        "clipboard_history_enabled" => {
            let enabled = parse_boolean_setting_value(require_non_empty(value, actual_key)?)?;
            manager.update_setting(actual_key, enabled)?;
            if !enabled {
                crate::engine::variables::system::clip::clip_manager().clear();
            }
            ApplySettingOutcome::default()
        }
        "clipboard_history_retention_secs" => {
            let raw_value = require_non_empty(value, actual_key)?;
            let parsed = raw_value
                .parse::<u32>()
                .map_err(|_| Error::Config(format!("bad retention seconds: {raw_value}")))?;
            manager.update_setting(
                actual_key,
                Settings::sanitize_clipboard_history_retention_secs(parsed),
            )?;
            ApplySettingOutcome::default()
        }
        "inline_datetime_enabled" => {
            let enabled = parse_boolean_setting_value(require_non_empty(value, actual_key)?)?;
            crate::settings::set_cached_inline_datetime_enabled(enabled);
            manager.update_setting(actual_key, enabled)?;
            ApplySettingOutcome::default()
        }
        "inline_currency_to_words_enabled" => {
            let enabled = parse_boolean_setting_value(require_non_empty(value, actual_key)?)?;
            crate::settings::set_cached_inline_currency_to_words_enabled(enabled);
            manager.update_setting(actual_key, enabled)?;
            ApplySettingOutcome::default()
        }
        "inline_dictionary_enabled" => {
            let enabled = parse_boolean_setting_value(require_non_empty(value, actual_key)?)?;
            crate::settings::set_cached_inline_dictionary_enabled(enabled);
            manager.update_setting(actual_key, enabled)?;
            ApplySettingOutcome::default()
        }
        "inline_dictionary_mode" => {
            let mode_str = require_non_empty(value, actual_key)?;
            let mode = parse_inline_dictionary_mode(mode_str)?;
            crate::settings::set_cached_inline_dictionary_mode(mode);
            manager.update_setting(actual_key, mode)?;
            ApplySettingOutcome::default()
        }
        "inline_datetime_date_format" => {
            let val = require_non_empty(value, actual_key)?.to_string();
            crate::settings::set_cached_inline_datetime_date_format(val.clone());
            manager.update_setting(actual_key, val)?;
            ApplySettingOutcome::default()
        }
        "inline_datetime_time_format" => {
            let val = require_non_empty(value, actual_key)?.to_string();
            crate::settings::set_cached_inline_datetime_time_format(val.clone());
            manager.update_setting(actual_key, val)?;
            ApplySettingOutcome::default()
        }
        "inline_datetime_datetime_format" => {
            let val = require_non_empty(value, actual_key)?.to_string();
            crate::settings::set_cached_inline_datetime_datetime_format(val.clone());
            manager.update_setting(actual_key, val)?;
            ApplySettingOutcome::default()
        }
        "inline_datetime_dialect" => {
            let val = require_non_empty(value, actual_key)?.to_lowercase();
            if val != "uk" && val != "us" {
                return Err(Error::Config("dialect must be 'uk' or 'us'".to_string()));
            }
            crate::settings::set_cached_inline_datetime_dialect(val.clone());
            manager.update_setting(actual_key, val)?;
            ApplySettingOutcome::default()
        }
        "inline_emoji_enabled" => {
            let enabled = parse_boolean_setting_value(require_non_empty(value, actual_key)?)?;
            crate::settings::set_cached_inline_emoji_enabled(enabled);
            manager.update_setting(actual_key, enabled)?;
            ApplySettingOutcome::default()
        }
        "inline_emoji_trigger_char" => {
            let c = parse_char_setting(value, actual_key)?;
            crate::settings::set_cached_inline_emoji_trigger_char(c);
            manager.update_setting(actual_key, c)?;
            ApplySettingOutcome::default()
        }
        "rpc_port" => {
            let raw_value = require_non_empty(value, actual_key)?;
            let parsed = raw_value
                .parse::<u16>()
                .map_err(|_| Error::Config(format!("bad port value: {raw_value}")))?;
            if parsed < 1024 {
                return Err(Error::Config(format!(
                    "port must be 1024-65535, got {raw_value}"
                )));
            }
            manager.update_setting(actual_key, parsed)?;
            ApplySettingOutcome::default()
        }
        "script_timeout" => {
            let raw_value = require_non_empty(value, actual_key)?;
            let parsed = raw_value
                .parse::<u32>()
                .map_err(|_| Error::Config(format!("bad timeout value: {raw_value}")))?;
            manager.update_setting(actual_key, parsed)?;
            ApplySettingOutcome::default()
        }
        "ai_temperature" => {
            let parsed = match value {
                Some(v) if !v.trim().is_empty() => Some(
                    v.trim()
                        .parse::<f32>()
                        .map_err(|_| Error::Config(format!("bad temperature value: {v}")))?,
                ),
                _ => None,
            };
            manager.update_setting(actual_key, parsed)?;
            ApplySettingOutcome::default()
        }
        "ai_max_tokens" => {
            let parsed = match value {
                Some(v) if !v.trim().is_empty() => Some(
                    v.trim()
                        .parse::<u32>()
                        .map_err(|_| Error::Config(format!("bad max tokens value: {v}")))?,
                ),
                _ => None,
            };
            manager.update_setting(actual_key, parsed)?;
            ApplySettingOutcome::default()
        }
        "ai_system_prompt" => {
            let parsed = match value {
                Some(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
                _ => None,
            };
            manager.update_setting(actual_key, parsed)?;
            ApplySettingOutcome::default()
        }
        "rpc_mode" => {
            let mode = parse_rpc_mode(require_non_empty(value, actual_key)?)?;
            manager.update_setting(actual_key, mode)?;
            ApplySettingOutcome::default()
        }
        "rpc_host" => {
            let host = require_non_empty(value, actual_key)?;
            manager.update_setting(actual_key, host.to_string())?;
            ApplySettingOutcome::default()
        }
        _ => {
            return Err(Error::Config(format!("unknown setting: {actual_key}")));
        }
    };

    Ok(outcome)
}

pub fn parse_boolean_setting_value(value: &str) -> Result<bool> {
    value
        .trim()
        .to_ascii_lowercase()
        .parse::<bool>()
        .map_err(|_| Error::Config(format!("bad boolean value: {value}")))
}

pub fn parse_spinner_style(value: &str) -> Result<SpinnerStyle> {
    match value.trim().to_ascii_lowercase().as_str() {
        "classic" => Ok(SpinnerStyle::Classic),
        "braille" => Ok(SpinnerStyle::Braille),
        "arc" => Ok(SpinnerStyle::Arc),
        other => Err(Error::Config(format!(
            "bad spinner_style '{other}' (use: classic, braille, arc)"
        ))),
    }
}

pub fn parse_inline_ai_trigger_mode(value: &str) -> Result<super::InlineAiTriggerMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "symmetric" => Ok(super::InlineAiTriggerMode::Symmetric),
        "asymmetric" => Ok(super::InlineAiTriggerMode::Asymmetric),
        other => Err(Error::Config(format!(
            "bad inline_ai_trigger_mode '{other}' (use: symmetric, asymmetric)"
        ))),
    }
}

pub fn parse_inline_dictionary_mode(value: &str) -> Result<super::InlineDictionaryMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "lite" => Ok(super::InlineDictionaryMode::Lite),
        "full" => Ok(super::InlineDictionaryMode::Full),
        other => Err(Error::Config(format!(
            "bad inline_dictionary_mode '{other}' (use: lite, full)"
        ))),
    }
}

pub fn parse_rpc_mode(value: &str) -> Result<super::RpcMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "socket" => Ok(super::RpcMode::Socket),
        "tcp" => Ok(super::RpcMode::Tcp),
        other => Err(Error::Config(format!(
            "bad rpc_mode '{other}' (use: socket, tcp)"
        ))),
    }
}

fn parse_char_setting(value: Option<&str>, key: &str) -> Result<char> {
    value
        .and_then(|value| value.chars().next())
        .ok_or_else(|| Error::Config(format!("{key} must not be empty")))
}

fn require_non_empty<'a>(value: Option<&'a str>, key: &str) -> Result<&'a str> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("{key} must not be empty")))
}

pub fn validate_delimiter_conflicts(settings: &Settings, key: &str, new_value: &str) -> Result<()> {
    match key {
        "inline_ai_trigger_open" => {
            if settings.inline_ai_trigger_mode == InlineAiTriggerMode::Asymmetric
                && new_value == settings.inline_ai_trigger_close
            {
                return Err(Error::Config(format!(
                    "'{}' value '{}' conflicts with '{}' value '{}'",
                    key, new_value, "inline_ai_trigger_close", settings.inline_ai_trigger_close
                )));
            }
            Ok(())
        }
        "inline_ai_trigger_close" => {
            if settings.inline_ai_trigger_mode == InlineAiTriggerMode::Asymmetric
                && new_value == settings.inline_ai_trigger_open
            {
                return Err(Error::Config(format!(
                    "'{}' value '{}' conflicts with '{}' value '{}'",
                    key, new_value, "inline_ai_trigger_open", settings.inline_ai_trigger_open
                )));
            }
            Ok(())
        }
        "inline_ai_trigger" => Ok(()),
        "inline_ai_trigger_mode" => {
            if new_value.trim().eq_ignore_ascii_case("asymmetric")
                && settings.inline_ai_trigger_open == settings.inline_ai_trigger_close
            {
                return Err(Error::Config(format!(
                    "'{}' value '{}' conflicts with '{}' value '{}'",
                    key, new_value, "inline_ai_trigger_open", settings.inline_ai_trigger_open
                )));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

mod defaults;
mod sync;

pub use defaults::default_setting_input;
pub use sync::apply_setting_input;
#[cfg(test)]
mod tests;
