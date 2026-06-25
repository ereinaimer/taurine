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
        "inline_ai_delimiter" => Ok(Some(defaults.inline_ai_delimiter.to_string())),
        "clipboard_restore_delay_ms" => Ok(Some(defaults.clipboard_restore_delay_ms.to_string())),
        "action_delimiter" => Ok(Some(match defaults.action_delimiter {
            super::ActionDelimiter::Space => "space".to_string(),
            super::ActionDelimiter::Enter => "enter".to_string(),
        })),
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
        | "inline_history_enabled" => {
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
        "inline_ai_delimiter" => {
            manager.update_setting(actual_key, parse_char_setting(value, actual_key)?)?;
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
}
