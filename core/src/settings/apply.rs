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
}
