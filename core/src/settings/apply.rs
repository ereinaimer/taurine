use tracing::warn;

use super::{Settings, SettingsManager, SpinnerStyle};
use crate::{
    ai::AiProvider,
    db::init,
    error::{Error, Result},
    keys::parse_hotkey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApplySettingOutcome {
    pub sync_boot: Option<bool>,
}

pub fn apply_setting_input(key: &str, value: Option<&str>) -> Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);
    let outcome = apply_setting_input_with_manager(&manager, key, value)?;

    if let Some(enabled) = outcome.sync_boot
        && let Err(error) = crate::service::sync_boot(enabled)
    {
        warn!(error = %error, "Failed to synchronize OS startup hook");
    }

    crate::rpc::notify_daemon_reload();
    Ok(())
}

pub fn reset_setting_to_default(key: &str) -> Result<()> {
    let default_value = default_setting_input(key)?;
    apply_setting_input(key, default_value.as_deref())
}

pub fn default_setting_input(key: &str) -> Result<Option<String>> {
    let actual_key = Settings::resolve_key(key);
    let defaults = Settings::default();

    match actual_key {
        "trigger_char" => Ok(Some(defaults.trigger_char.to_string())),
        "pause_hotkey" => Ok(Some(defaults.pause_hotkey)),
        "pause_notifications_enabled" => Ok(Some(defaults.pause_notifications_enabled.to_string())),
        "pause_audio_enabled" => Ok(Some(defaults.pause_audio_enabled.to_string())),
        "start_on_boot" => Ok(Some(defaults.start_on_boot.to_string())),
        "inline_tab_completion_enabled" => {
            Ok(Some(defaults.inline_tab_completion_enabled.to_string()))
        }
        "inline_history_enabled" => Ok(Some(defaults.inline_history_enabled.to_string())),
        "wpm" => Ok(Some(defaults.wpm.to_string())),
        "spinner_style" => Ok(Some(match defaults.spinner_style {
            SpinnerStyle::Classic => "classic".to_string(),
            SpinnerStyle::Braille => "braille".to_string(),
            SpinnerStyle::Arc => "arc".to_string(),
        })),
        "ai_provider" => Ok(defaults.ai_provider),
        "ai_model" => Ok(defaults.ai_model),
        "ai_custom_endpoint" => Ok(defaults.ai_custom_endpoint),
        "ai_delimiter_mode" => Ok(Some(match defaults.ai_delimiter_mode {
            super::AiDelimiterMode::Symmetric => "symmetric".to_string(),
            super::AiDelimiterMode::Asymmetric => "asymmetric".to_string(),
        })),
        "ai_symmetric_delimiter" => Ok(Some(defaults.ai_symmetric_delimiter)),
        "ai_open_delimiter" => Ok(Some(defaults.ai_open_delimiter)),
        "ai_close_delimiter" => Ok(Some(defaults.ai_close_delimiter)),
        "clipboard_restore_delay_ms" => Ok(Some(defaults.clipboard_restore_delay_ms.to_string())),
        "action_delimiter" => Ok(Some(match defaults.action_delimiter {
            super::ActionDelimiter::Space => "space".to_string(),
            super::ActionDelimiter::Enter => "enter".to_string(),
        })),
        "triggerless_mode" => Ok(Some(defaults.triggerless_mode.to_string())),
        "instant_expand" => Ok(Some(defaults.instant_expand.to_string())),
        "ignore_fullscreen" => Ok(Some(defaults.ignore_fullscreen.to_string())),
        "rpc_port" => Ok(Some(defaults.rpc_port.to_string())),
        "rpc_mode" => Ok(Some(match defaults.rpc_mode {
            super::RpcMode::Socket => "socket".to_string(),
            super::RpcMode::Tcp => "tcp".to_string(),
        })),
        "rpc_host" => Ok(Some(defaults.rpc_host)),
        "rpc_token" => Ok(Some(uuid::Uuid::new_v4().to_string())),
        "script_timeout" => Ok(Some(defaults.script_timeout.to_string())),
        "ai_temperature" => Ok(defaults.ai_temperature.map(|v| v.to_string())),
        "ai_max_tokens" => Ok(defaults.ai_max_tokens.map(|v| v.to_string())),
        "ai_system_prompt" => Ok(defaults.ai_system_prompt),
        "auto_update" => Ok(Some(defaults.auto_update.to_string())),
        "clipboard_history_enabled" => Ok(Some(defaults.clipboard_history_enabled.to_string())),
        "clipboard_history_retention_secs" => {
            Ok(Some(defaults.clipboard_history_retention_secs.to_string()))
        }
        "inline_emoji_enabled" => Ok(Some(defaults.inline_emoji_enabled.to_string())),
        "inline_emoji_trigger_char" => Ok(Some(defaults.inline_emoji_trigger_char.to_string())),
        _ => Err(Error::Config(format!("Unknown setting key: {actual_key}"))),
    }
}

pub fn apply_setting_input_with_manager(
    manager: &SettingsManager<'_>,
    key: &str,
    value: Option<&str>,
) -> Result<ApplySettingOutcome> {
    let actual_key = Settings::resolve_key(key);

    let outcome = match actual_key {
        "trigger_char" => {
            manager.update_setting(actual_key, parse_char_setting(value, actual_key)?)?;
            ApplySettingOutcome::default()
        }
        "pause_hotkey" => {
            let hotkey = require_non_empty(value, actual_key)?;
            parse_hotkey(hotkey).map_err(|error| {
                Error::Config(format!("Invalid pause_hotkey value '{hotkey}': {error}"))
            })?;
            manager.update_setting(actual_key, hotkey.to_string())?;
            ApplySettingOutcome::default()
        }
        "pause_notifications_enabled"
        | "pause_audio_enabled"
        | "inline_tab_completion_enabled"
        | "inline_history_enabled"
        | "ignore_fullscreen" => {
            manager.update_setting(
                actual_key,
                parse_boolean_setting_value(require_non_empty(value, actual_key)?)?,
            )?;
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
                .map_err(|_| Error::Config(format!("Invalid WPM value: {raw_value}")))?;
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
            let provider = AiProvider::try_from(require_non_empty(value, actual_key)?)?;
            manager.update_setting(actual_key, Some(provider.as_str().to_string()))?;
            ApplySettingOutcome::default()
        }
        "ai_model" => {
            let model = require_trimmed_non_empty(value, actual_key)?;
            manager.update_setting(actual_key, Some(model.to_string()))?;
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
        "ai_delimiter_mode" => {
            manager.update_setting(
                actual_key,
                parse_ai_delimiter_mode(require_non_empty(value, actual_key)?)?,
            )?;
            ApplySettingOutcome::default()
        }
        "ai_open_delimiter" | "ai_close_delimiter" | "ai_symmetric_delimiter" => {
            let parsed = require_non_empty(value, actual_key)?;
            manager.update_setting(actual_key, parsed.to_string())?;
            ApplySettingOutcome::default()
        }
        "clipboard_restore_delay_ms" => {
            let raw_value = require_non_empty(value, actual_key)?;
            let parsed = raw_value
                .parse::<u32>()
                .map_err(|_| Error::Config(format!("Invalid delay value: {raw_value}")))?;
            manager.update_setting(
                actual_key,
                Settings::sanitize_clipboard_restore_delay_ms(parsed),
            )?;
            ApplySettingOutcome::default()
        }
        "action_delimiter" => {
            manager.update_setting(
                actual_key,
                parse_action_delimiter(require_non_empty(value, actual_key)?)?,
            )?;
            ApplySettingOutcome::default()
        }
        "triggerless_mode" => {
            manager.update_setting(
                actual_key,
                parse_boolean_setting_value(require_non_empty(value, actual_key)?)?,
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
        "auto_update" => {
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
            let parsed = raw_value.parse::<u32>().map_err(|_| {
                Error::Config(format!("Invalid retention seconds value: {raw_value}"))
            })?;
            manager.update_setting(
                actual_key,
                Settings::sanitize_clipboard_history_retention_secs(parsed),
            )?;
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
                .map_err(|_| Error::Config(format!("Invalid port value: {raw_value}")))?;
            if parsed < 1024 {
                return Err(Error::Config(format!(
                    "Invalid port value: {raw_value}. Must be between 1024 and 65535"
                )));
            }
            manager.update_setting(actual_key, parsed)?;
            ApplySettingOutcome::default()
        }
        "script_timeout" => {
            let raw_value = require_non_empty(value, actual_key)?;
            let parsed = raw_value
                .parse::<u32>()
                .map_err(|_| Error::Config(format!("Invalid timeout value: {raw_value}")))?;
            manager.update_setting(actual_key, parsed)?;
            ApplySettingOutcome::default()
        }
        "ai_temperature" => {
            let parsed = match value {
                Some(v) if !v.trim().is_empty() => Some(
                    v.trim()
                        .parse::<f32>()
                        .map_err(|_| Error::Config(format!("Invalid temperature value: {v}")))?,
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
                        .map_err(|_| Error::Config(format!("Invalid max tokens value: {v}")))?,
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
        "rpc_token" => {
            let token = require_non_empty(value, actual_key)?;
            manager.update_setting(actual_key, token.to_string())?;
            ApplySettingOutcome::default()
        }
        _ => {
            return Err(Error::Config(format!("Unknown setting key: {actual_key}")));
        }
    };

    Ok(outcome)
}

pub fn parse_boolean_setting_value(value: &str) -> Result<bool> {
    value
        .trim()
        .to_ascii_lowercase()
        .parse::<bool>()
        .map_err(|_| Error::Config(format!("Invalid boolean value: {value}")))
}

pub fn parse_spinner_style(value: &str) -> Result<SpinnerStyle> {
    match value.trim().to_ascii_lowercase().as_str() {
        "classic" => Ok(SpinnerStyle::Classic),
        "braille" => Ok(SpinnerStyle::Braille),
        "arc" => Ok(SpinnerStyle::Arc),
        other => Err(Error::Config(format!(
            "Invalid spinner_style value '{other}'. Supported values: classic, braille, arc"
        ))),
    }
}

pub fn parse_action_delimiter(value: &str) -> Result<super::ActionDelimiter> {
    match value.trim().to_ascii_lowercase().as_str() {
        "space" => Ok(super::ActionDelimiter::Space),
        "enter" => Ok(super::ActionDelimiter::Enter),
        other => Err(Error::Config(format!(
            "Invalid action_delimiter value '{other}'. Supported values: space, enter"
        ))),
    }
}

pub fn parse_ai_delimiter_mode(value: &str) -> Result<super::AiDelimiterMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "symmetric" => Ok(super::AiDelimiterMode::Symmetric),
        "asymmetric" => Ok(super::AiDelimiterMode::Asymmetric),
        other => Err(Error::Config(format!(
            "Invalid ai_delimiter_mode value '{other}'. Supported values: symmetric, asymmetric"
        ))),
    }
}

pub fn parse_rpc_mode(value: &str) -> Result<super::RpcMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "socket" => Ok(super::RpcMode::Socket),
        "tcp" => Ok(super::RpcMode::Tcp),
        other => Err(Error::Config(format!(
            "Invalid rpc_mode value '{other}'. Supported values: socket, tcp"
        ))),
    }
}

fn parse_char_setting(value: Option<&str>, key: &str) -> Result<char> {
    value
        .and_then(|value| value.chars().next())
        .ok_or_else(|| Error::Config(format!("Invalid {key} value: must not be empty")))
}

fn require_non_empty<'a>(value: Option<&'a str>, key: &str) -> Result<&'a str> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("Invalid {key} value: must not be empty")))
}

fn require_trimmed_non_empty<'a>(value: Option<&'a str>, key: &str) -> Result<&'a str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("Invalid {key} value: must not be empty")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::open_test_db;

    #[test]
    fn toggling_inline_history_persists_changed_value() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        apply_setting_input_with_manager(&manager, "inline_history_enabled", Some("false"))
            .unwrap();

        assert!(!manager.load_all().inline_history_enabled);
    }

    #[test]
    fn invalid_wpm_is_rejected() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        assert!(apply_setting_input_with_manager(&manager, "wpm", Some("fast")).is_err());
    }

    #[test]
    fn valid_spinner_style_arc_is_accepted() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        apply_setting_input_with_manager(&manager, "spinner_style", Some("arc")).unwrap();

        assert_eq!(manager.load_all().spinner_style, SpinnerStyle::Arc);
    }

    #[test]
    fn ai_custom_endpoint_can_be_cleared() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);
        manager
            .update_setting(
                "ai_custom_endpoint",
                Some("https://example.com".to_string()),
            )
            .unwrap();

        apply_setting_input_with_manager(&manager, "ai_custom_endpoint", None).unwrap();

        assert_eq!(manager.load_all().ai_custom_endpoint, None);
    }

    #[test]
    fn pause_hotkey_uses_shared_validation() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        assert!(
            apply_setting_input_with_manager(&manager, "pause_hotkey", Some("not+a+hotkey+???"))
                .is_err()
        );
    }

    #[test]
    fn default_setting_input_uses_canonical_defaults() {
        assert_eq!(
            default_setting_input("trigger_char").unwrap(),
            Some(">".to_string())
        );
        assert_eq!(
            default_setting_input("inline_tab_completion_enabled").unwrap(),
            Some("true".to_string())
        );
        assert_eq!(default_setting_input("ai_custom_endpoint").unwrap(), None);
    }

    #[test]
    fn resetting_inline_tab_completion_restores_default() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);
        manager
            .update_setting("inline_tab_completion_enabled", false)
            .unwrap();

        let default_value = default_setting_input("inline_tab_completion_enabled").unwrap();
        apply_setting_input_with_manager(
            &manager,
            "inline_tab_completion_enabled",
            default_value.as_deref(),
        )
        .unwrap();

        assert!(manager.load_all().inline_tab_completion_enabled);
    }

    #[test]
    fn resetting_trigger_char_restores_default() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);
        manager.update_setting("trigger_char", ';').unwrap();

        let default_value = default_setting_input("trigger_char").unwrap();
        apply_setting_input_with_manager(&manager, "trigger_char", default_value.as_deref())
            .unwrap();

        assert_eq!(
            manager.load_all().trigger_char,
            Settings::default().trigger_char
        );
    }

    #[test]
    fn test_apply_clipboard_restore_delay_ms() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        // Apply a valid value
        apply_setting_input_with_manager(&manager, "clipboard_restore_delay_ms", Some("1200"))
            .unwrap();
        assert_eq!(manager.load_all().clipboard_restore_delay_ms, 1200);

        // Apply clamped value
        apply_setting_input_with_manager(&manager, "clipboard_restore_delay_ms", Some("3000"))
            .unwrap();
        assert_eq!(manager.load_all().clipboard_restore_delay_ms, 2000);
    }

    #[test]
    fn resetting_rpc_token_generates_new_uuid() {
        let default_val = default_setting_input("rpc_token").unwrap();
        assert!(default_val.is_some());
        let token_str = default_val.unwrap();
        assert!(!token_str.is_empty());
        assert!(uuid::Uuid::parse_str(&token_str).is_ok());
    }

    #[test]
    fn test_inline_emoji_settings() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        // Verify default setting input
        assert_eq!(
            default_setting_input("inline_emoji_enabled").unwrap(),
            Some("true".to_string())
        );
        assert_eq!(
            default_setting_input("inline_emoji_trigger_char").unwrap(),
            Some(":".to_string())
        );

        // Apply new values
        apply_setting_input_with_manager(&manager, "inline_emoji_enabled", Some("false")).unwrap();
        apply_setting_input_with_manager(&manager, "inline_emoji_trigger_char", Some(";")).unwrap();

        let loaded = manager.load_all();
        assert!(!loaded.inline_emoji_enabled);
        assert_eq!(loaded.inline_emoji_trigger_char, ';');

        assert!(!crate::settings::get_cached_inline_emoji_enabled());
        assert_eq!(crate::settings::get_cached_inline_emoji_trigger_char(), ';');
    }
}
