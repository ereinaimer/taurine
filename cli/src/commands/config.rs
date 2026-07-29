use taurine_core::db::init;
use taurine_core::settings::{Settings, SettingsManager};
use tracing::{info, warn};

pub fn execute_list(json: bool) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);
    let settings = manager.load_all();

    if json {
        println!("{}", serde_json::to_string(&settings).unwrap());
        return Ok(());
    }

    // Helper to build each line as (key, value) pair
    let pairs: Vec<(&str, String)> = vec![
        ("trigger_char", settings.trigger_char.to_string()),
        ("pause_hotkey", settings.pause_hotkey.clone()),
        (
            "pause_notifications_enabled",
            settings.pause_notifications_enabled.to_string(),
        ),
        (
            "pause_audio_enabled",
            settings.pause_audio_enabled.to_string(),
        ),
        ("start_on_boot", settings.start_on_boot.to_string()),
        (
            "system_tray_enabled",
            settings.system_tray_enabled.to_string(),
        ),
        (
            "inline_tab_completion_enabled",
            settings.inline_tab_completion_enabled.to_string(),
        ),
        (
            "inline_history_enabled",
            settings.inline_history_enabled.to_string(),
        ),
        ("wpm", settings.wpm.to_string()),
        (
            "spinner_style",
            format!("{:?}", settings.spinner_style).to_lowercase(),
        ),
        (
            "ai_provider",
            render_optional_setting(settings.ai_provider.as_deref()).to_string(),
        ),
        (
            "ai_model",
            render_optional_setting(settings.ai_model.as_deref()).to_string(),
        ),
        (
            "ai_temperature",
            render_optional_setting(settings.ai_temperature.map(|v| v.to_string()).as_deref())
                .to_string(),
        ),
        (
            "ai_max_tokens",
            render_optional_setting(settings.ai_max_tokens.map(|v| v.to_string()).as_deref())
                .to_string(),
        ),
        (
            "ai_system_prompt",
            render_optional_setting(settings.ai_system_prompt.as_deref()).to_string(),
        ),
        (
            "ai_custom_endpoint",
            render_optional_setting(settings.ai_custom_endpoint.as_deref()).to_string(),
        ),
        (
            "inline_ai_trigger_mode",
            format!("{:?}", settings.inline_ai_trigger_mode).to_lowercase(),
        ),
        ("inline_ai_trigger", settings.inline_ai_trigger.clone()),
        (
            "inline_ai_trigger_open",
            settings.inline_ai_trigger_open.clone(),
        ),
        (
            "inline_ai_trigger_close",
            settings.inline_ai_trigger_close.clone(),
        ),
        (
            "clipboard_restore_delay_ms",
            settings.clipboard_restore_delay_ms.to_string(),
        ),
        (
            "clipboard_history_enabled",
            settings.clipboard_history_enabled.to_string(),
        ),
        (
            "clipboard_history_retention_secs",
            settings.clipboard_history_retention_secs.to_string(),
        ),
        (
            "action_key",
            format!("{:?}", settings.action_key).to_lowercase(),
        ),
        ("triggerless_mode", settings.triggerless_mode.to_string()),
        ("scripts_enabled", settings.scripts_enabled.to_string()),
        ("instant_expand", settings.instant_expand.to_string()),
        (
            "inline_emoji_enabled",
            settings.inline_emoji_enabled.to_string(),
        ),
        (
            "inline_datetime_enabled",
            settings.inline_datetime_enabled.to_string(),
        ),
        (
            "inline_currency_to_words_enabled",
            settings.inline_currency_to_words_enabled.to_string(),
        ),
        (
            "rpc_mode",
            format!("{:?}", settings.rpc_mode).to_lowercase(),
        ),
    ];

    let show_tcp_settings = settings.rpc_mode == taurine_core::settings::RpcMode::Tcp;

    // Calculate key column width
    let max_key_len = pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(10);
    let pad = 2;

    for (key, value) in &pairs {
        println!(
            "{:<kw$}{:pad$}{}",
            key,
            "",
            value,
            kw = max_key_len,
            pad = pad
        );
    }

    if show_tcp_settings {
        println!(
            "{:<kw$}{:pad$}{}",
            "rpc_host",
            "",
            settings.rpc_host,
            kw = max_key_len,
            pad = pad
        );
        println!(
            "{:<kw$}{:pad$}{}",
            "rpc_port",
            "",
            settings.rpc_port,
            kw = max_key_len,
            pad = pad
        );
        println!(
            "{:<kw$}{:pad$}{}",
            "rpc_token",
            "",
            settings.rpc_token,
            kw = max_key_len,
            pad = pad
        );
    }
    println!(
        "{:<kw$}{:pad$}{}",
        "script_timeout",
        "",
        settings.script_timeout,
        kw = max_key_len,
        pad = pad
    );

    Ok(())
}

pub fn execute_set(key: String, value: String, json: bool) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);

    let actual_key = Settings::resolve_key(&key);

    match actual_key {
        "trigger_char" => {
            if let Some(c) = value.chars().next() {
                let current = manager.load_all();
                taurine_core::settings::validate_delimiter_conflicts(
                    &current,
                    actual_key,
                    &c.to_string(),
                )?;
                manager.update_setting(actual_key, c)?;
                info!("Updated trigger_char to: {}", c);
            } else {
                warn!("Invalid trigger character provided.");
            }
        }
        "pause_hotkey" => {
            manager.update_setting(actual_key, value.clone())?;
            info!("Updated pause_hotkey to: {}", value);
        }
        "start_on_boot"
        | "pause_notifications_enabled"
        | "pause_audio_enabled"
        | "inline_tab_completion_enabled"
        | "inline_history_enabled"
        | "ignore_fullscreen"
        | "auto_update"
        | "inline_emoji_enabled"
        | "inline_datetime_enabled"
        | "inline_currency_to_words_enabled"
        | "triggerless_mode"
        | "instant_expand"
        | "clipboard_history_enabled"
        | "scripts_enabled"
        | "system_tray_enabled" => {
            let b = parse_boolean_setting_value(&value)?;
            manager.update_setting(actual_key, b)?;
            info!("Updated {} to: {}", actual_key, b);

            if actual_key == "start_on_boot"
                && let Err(e) = taurine_core::service::sync_boot(b)
            {
                warn!("Failed to synchronize OS startup hook: {}", e);
            }
        }
        "wpm" => {
            let parsed = value.parse::<u32>().map_err(|_| {
                taurine_core::error::Error::Config(format!("bad WPM value: {}", value))
            })?;
            let wpm = Settings::sanitize_wpm(parsed);
            manager.update_setting(actual_key, wpm)?;
            info!("Updated wpm to: {}", wpm);
        }
        "clipboard_restore_delay_ms" => {
            let parsed = value.parse::<u32>().map_err(|_| {
                taurine_core::error::Error::Config(format!("bad delay value: {}", value))
            })?;
            let delay = Settings::sanitize_clipboard_restore_delay_ms(parsed);
            manager.update_setting(actual_key, delay)?;
            info!("Updated clipboard_restore_delay_ms to: {}", delay);
        }
        "clipboard_history_retention_secs" => {
            let parsed = value.parse::<u32>().map_err(|_| {
                taurine_core::error::Error::Config(format!("bad retention seconds: {}", value))
            })?;
            let secs = Settings::sanitize_clipboard_history_retention_secs(parsed);
            manager.update_setting(actual_key, secs)?;
            info!("Updated clipboard_history_retention_secs to: {}", secs);
        }
        "spinner_style" => {
            let s = match value.to_lowercase().as_str() {
                "braille" => taurine_core::settings::SpinnerStyle::Braille,
                "arc" => taurine_core::settings::SpinnerStyle::Arc,
                "classic" => taurine_core::settings::SpinnerStyle::Classic,
                _ => {
                    warn!(
                        "Invalid spinner style: {}. Supported: braille, arc, classic",
                        value
                    );
                    return Ok(());
                }
            };
            manager.update_setting(actual_key, s)?;
            info!("Updated spinner_style to: {}", value);
        }
        "action_key" => {
            let s = match value.to_lowercase().as_str() {
                "space" => taurine_core::settings::ActionKey::Space,
                "enter" => taurine_core::settings::ActionKey::Enter,
                _ => {
                    warn!("Invalid action key: {}. Supported: space, enter", value);
                    return Ok(());
                }
            };
            manager.update_setting(actual_key, s)?;
            info!("Updated action_key to: {}", value);
        }
        "ai_provider" => {
            let provider = taurine_core::ai::AiProvider::try_from(value.as_str())?;
            manager.update_setting(actual_key, Some(provider.as_str().to_string()))?;
            info!("Updated ai_provider to: {}", provider.as_str());
        }
        "ai_model" | "ai_custom_endpoint" => {
            let val = value.trim();
            if val.is_empty() {
                return Err(taurine_core::error::Error::Config(format!(
                    "{actual_key} must not be empty"
                )));
            }
            manager.update_setting(actual_key, Some(val.to_string()))?;
            info!("Updated {actual_key} to: {val}");
        }
        "inline_ai_delimiter" => {
            if let Some(c) = value.chars().next() {
                let current = manager.load_all();
                taurine_core::settings::validate_delimiter_conflicts(
                    &current,
                    "inline_ai_trigger",
                    &c.to_string(),
                )?;
                manager.update_setting(actual_key, c)?;
                info!("Updated inline_ai_delimiter to: {}", c);
            } else {
                warn!("Invalid delimiter character provided.");
            }
        }
        "rpc_port" => {
            let parsed = value.parse::<u16>().map_err(|_| {
                taurine_core::error::Error::Config(format!("bad port value: {}", value))
            })?;
            if parsed < 1024 {
                warn!(
                    "Invalid port value: {}. Must be between 1024 and 65535",
                    parsed
                );
                return Ok(());
            }
            manager.update_setting(actual_key, parsed)?;
            info!(
                "Updated rpc_port to: {}. Note: please restart the Taurine service for this to take effect.",
                parsed
            );
        }
        "ai_temperature" => {
            let parsed = value.parse::<f32>().map_err(|_| {
                taurine_core::error::Error::Config(format!("bad temperature value: {}", value))
            })?;
            manager.update_setting(actual_key, Some(parsed))?;
            info!("Updated ai_temperature to: {}", parsed);
        }
        "ai_max_tokens" => {
            let parsed = value.parse::<u32>().map_err(|_| {
                taurine_core::error::Error::Config(format!("bad max tokens value: {}", value))
            })?;
            manager.update_setting(actual_key, Some(parsed))?;
            info!("Updated ai_max_tokens to: {}", parsed);
        }
        "ai_system_prompt" => {
            let val = value.trim();
            if val.is_empty() {
                manager.update_setting(actual_key, None as Option<String>)?;
                info!("Cleared ai_system_prompt");
            } else {
                manager.update_setting(actual_key, Some(val.to_string()))?;
                info!("Updated ai_system_prompt to: {}", val);
            }
        }
        "rpc_mode" => {
            let mode = match value.to_lowercase().as_str() {
                "socket" => taurine_core::settings::RpcMode::Socket,
                "tcp" => taurine_core::settings::RpcMode::Tcp,
                _ => {
                    warn!("Invalid rpc_mode: {}. Supported: socket, tcp", value);
                    return Ok(());
                }
            };
            manager.update_setting(actual_key, mode)?;
            info!("Updated rpc_mode to: {:?}", mode);
        }
        "rpc_host" => {
            let val = value.trim();
            if val.is_empty() {
                warn!("rpc_host cannot be empty.");
                return Ok(());
            }
            manager.update_setting(actual_key, val.to_string())?;
            info!("Updated rpc_host to: {}", val);
        }
        "rpc_token" => {
            let val = value.trim();
            if val.is_empty() {
                warn!("rpc_token cannot be empty.");
                return Ok(());
            }
            manager.update_setting(actual_key, val.to_string())?;
            info!("Updated rpc_token to: {}", val);
        }
        "inline_datetime_date_format"
        | "inline_datetime_time_format"
        | "inline_datetime_datetime_format"
        | "inline_datetime_dialect" => {
            let val = value.trim();
            if val.is_empty() {
                return Err(taurine_core::error::Error::Config(format!(
                    "{actual_key} must not be empty"
                )));
            }
            if actual_key == "inline_datetime_dialect" && val != "uk" && val != "us" {
                return Err(taurine_core::error::Error::Config(format!(
                    "dialect must be 'uk' or 'us', got '{val}'"
                )));
            }
            manager.update_setting(actual_key, val.to_string())?;
            info!("Updated {actual_key} to: {val}");
        }
        _ => {
            warn!("Unknown setting key: {}", key);
            return Ok(());
        }
    }

    taurine_core::rpc::notify_daemon_reload();
    if json {
        println!(
            "{}",
            serde_json::json!({"status": "updated", "key": actual_key})
        );
    }
    Ok(())
}
pub fn execute_reset(key: String, json: bool) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);
    let defaults = Settings::default();

    let actual_key = Settings::resolve_key(&key);

    match actual_key {
        "trigger_char" => {
            manager.update_setting(actual_key, defaults.trigger_char)?;
            info!("Reset trigger_char to default: {}", defaults.trigger_char);
        }
        "pause_hotkey" => {
            manager.update_setting(actual_key, &defaults.pause_hotkey)?;
            info!("Reset pause_hotkey to default: {}", defaults.pause_hotkey);
        }
        "pause_notifications_enabled" => {
            manager.update_setting(actual_key, defaults.pause_notifications_enabled)?;
            info!(
                "Reset pause_notifications_enabled to default: {}",
                defaults.pause_notifications_enabled
            );
        }
        "pause_audio_enabled" => {
            manager.update_setting(actual_key, defaults.pause_audio_enabled)?;
            info!(
                "Reset pause_audio_enabled to default: {}",
                defaults.pause_audio_enabled
            );
        }
        "start_on_boot" => {
            manager.update_setting(actual_key, defaults.start_on_boot)?;
            info!("Reset start_on_boot to default: {}", defaults.start_on_boot);

            if let Err(e) = taurine_core::service::sync_boot(defaults.start_on_boot) {
                warn!("Failed to synchronize OS startup hook: {}", e);
            }
        }
        "inline_tab_completion_enabled" => {
            manager.update_setting(actual_key, defaults.inline_tab_completion_enabled)?;
            info!(
                "Reset inline_tab_completion_enabled to default: {}",
                defaults.inline_tab_completion_enabled
            );
        }
        "inline_history_enabled" => {
            manager.update_setting(actual_key, defaults.inline_history_enabled)?;
            info!(
                "Reset inline_history_enabled to default: {}",
                defaults.inline_history_enabled
            );
        }
        "inline_emoji_enabled" => {
            manager.update_setting(actual_key, defaults.inline_emoji_enabled)?;
            info!(
                "Reset inline_emoji_enabled to default: {}",
                defaults.inline_emoji_enabled
            );
        }
        "inline_datetime_enabled" => {
            manager.update_setting(actual_key, defaults.inline_datetime_enabled)?;
            info!(
                "Reset inline_datetime_enabled to default: {}",
                defaults.inline_datetime_enabled
            );
        }
        "inline_currency_to_words_enabled" => {
            manager.update_setting(actual_key, defaults.inline_currency_to_words_enabled)?;
            info!(
                "Reset inline_currency_to_words_enabled to default: {}",
                defaults.inline_currency_to_words_enabled
            );
        }
        "wpm" => {
            manager.update_setting(actual_key, defaults.wpm)?;
            info!("Reset wpm to default: {}", defaults.wpm);
        }
        "clipboard_restore_delay_ms" => {
            manager.update_setting(actual_key, defaults.clipboard_restore_delay_ms)?;
            info!(
                "Reset clipboard_restore_delay_ms to default: {}",
                defaults.clipboard_restore_delay_ms
            );
        }
        "clipboard_history_enabled" => {
            manager.update_setting(actual_key, defaults.clipboard_history_enabled)?;
            info!(
                "Reset clipboard_history_enabled to default: {}",
                defaults.clipboard_history_enabled
            );
        }
        "clipboard_history_retention_secs" => {
            manager.update_setting(actual_key, defaults.clipboard_history_retention_secs)?;
            info!(
                "Reset clipboard_history_retention_secs to default: {}",
                defaults.clipboard_history_retention_secs
            );
        }
        "spinner_style" => {
            manager.update_setting(actual_key, defaults.spinner_style)?;
            info!(
                "Reset spinner_style to default: {:?}",
                defaults.spinner_style
            );
        }
        "action_key" => {
            manager.update_setting(actual_key, defaults.action_key)?;
            info!("Reset action_key to default: {:?}", defaults.action_key);
        }
        "triggerless_mode" => {
            manager.update_setting(actual_key, defaults.triggerless_mode)?;
            info!(
                "Reset triggerless_mode to default: {}",
                defaults.triggerless_mode
            );
        }
        "instant_expand" => {
            manager.update_setting(actual_key, defaults.instant_expand)?;
            info!(
                "Reset instant_expand to default: {}",
                defaults.instant_expand
            );
        }
        "scripts_enabled" => {
            manager.update_setting(actual_key, defaults.scripts_enabled)?;
            info!(
                "Reset scripts_enabled to default: {}",
                defaults.scripts_enabled
            );
        }
        "system_tray_enabled" => {
            manager.update_setting(actual_key, defaults.system_tray_enabled)?;
            info!(
                "Reset system_tray_enabled to default: {}",
                defaults.system_tray_enabled
            );
        }
        "ai_provider" => {
            manager.update_setting(actual_key, defaults.ai_provider.clone())?;
            info!("Reset ai_provider to default: <unset>");
        }
        "ai_model" => {
            manager.update_setting(actual_key, defaults.ai_model.clone())?;
            info!("Reset ai_model to default: <unset>");
        }
        "ai_temperature" => {
            manager.update_setting(actual_key, defaults.ai_temperature)?;
            info!("Reset ai_temperature to default: <unset>");
        }
        "ai_max_tokens" => {
            manager.update_setting(actual_key, defaults.ai_max_tokens)?;
            info!("Reset ai_max_tokens to default: <unset>");
        }
        "ai_system_prompt" => {
            manager.update_setting(actual_key, defaults.ai_system_prompt.clone())?;
            info!("Reset ai_system_prompt to default: <unset>");
        }
        "ai_custom_endpoint" => {
            manager.update_setting(actual_key, defaults.ai_custom_endpoint.clone())?;
            info!("Reset ai_custom_endpoint to default: <unset>");
        }
        "inline_ai_trigger_mode" => {
            manager.update_setting(actual_key, defaults.inline_ai_trigger_mode)?;
            info!(
                "Reset inline_ai_trigger_mode to default: {:?}",
                defaults.inline_ai_trigger_mode
            );
        }
        "inline_ai_trigger" => {
            manager.update_setting(actual_key, defaults.inline_ai_trigger.clone())?;
            info!(
                "Reset inline_ai_trigger to default: {}",
                defaults.inline_ai_trigger
            );
        }
        "inline_ai_trigger_open" => {
            manager.update_setting(actual_key, defaults.inline_ai_trigger_open.clone())?;
            info!(
                "Reset inline_ai_trigger_open to default: {}",
                defaults.inline_ai_trigger_open
            );
        }
        "inline_ai_trigger_close" => {
            manager.update_setting(actual_key, defaults.inline_ai_trigger_close.clone())?;
            info!(
                "Reset inline_ai_trigger_close to default: {}",
                defaults.inline_ai_trigger_close
            );
        }
        "rpc_port" => {
            manager.update_setting(actual_key, defaults.rpc_port)?;
            info!(
                "Reset rpc_port to default: {}. Note: please restart the Taurine service for this to take effect.",
                defaults.rpc_port
            );
        }
        "rpc_mode" => {
            manager.update_setting(actual_key, defaults.rpc_mode)?;
            info!("Reset rpc_mode to default: {:?}", defaults.rpc_mode);
        }
        "rpc_host" => {
            manager.update_setting(actual_key, defaults.rpc_host.clone())?;
            info!("Reset rpc_host to default: {}", defaults.rpc_host);
        }
        "rpc_token" => {
            let token = uuid::Uuid::new_v4().to_string();
            manager.update_setting(actual_key, token.clone())?;
            info!("Generated new rpc_token: {}", token);
        }
        "script_timeout" => {
            manager.update_setting(actual_key, defaults.script_timeout)?;
            info!(
                "Reset script_timeout to default: {}",
                defaults.script_timeout
            );
        }
        _ => {
            warn!("Unknown setting key: {}", key);
            return Ok(());
        }
    }

    taurine_core::rpc::notify_daemon_reload();
    if json {
        println!(
            "{}",
            serde_json::json!({"status": "reset", "key": actual_key})
        );
    }
    Ok(())
}

pub fn execute_reset_all(json: bool) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);
    let defaults = Settings::default();

    manager.update_setting("trigger_char", defaults.trigger_char)?;
    manager.update_setting("pause_hotkey", &defaults.pause_hotkey)?;
    manager.update_setting(
        "pause_notifications_enabled",
        defaults.pause_notifications_enabled,
    )?;
    manager.update_setting("pause_audio_enabled", defaults.pause_audio_enabled)?;
    manager.update_setting("start_on_boot", defaults.start_on_boot)?;
    manager.update_setting("system_tray_enabled", defaults.system_tray_enabled)?;
    manager.update_setting(
        "inline_tab_completion_enabled",
        defaults.inline_tab_completion_enabled,
    )?;
    manager.update_setting("inline_history_enabled", defaults.inline_history_enabled)?;
    manager.update_setting("wpm", defaults.wpm)?;
    manager.update_setting("spinner_style", defaults.spinner_style)?;
    manager.update_setting("ai_provider", defaults.ai_provider.clone())?;
    manager.update_setting("ai_model", defaults.ai_model.clone())?;
    manager.update_setting("ai_custom_endpoint", defaults.ai_custom_endpoint.clone())?;
    manager.update_setting("inline_ai_trigger_mode", defaults.inline_ai_trigger_mode)?;
    manager.update_setting("inline_ai_trigger", defaults.inline_ai_trigger.clone())?;
    manager.update_setting(
        "inline_ai_trigger_open",
        defaults.inline_ai_trigger_open.clone(),
    )?;
    manager.update_setting(
        "inline_ai_trigger_close",
        defaults.inline_ai_trigger_close.clone(),
    )?;
    manager.update_setting(
        "clipboard_restore_delay_ms",
        defaults.clipboard_restore_delay_ms,
    )?;
    manager.update_setting(
        "clipboard_history_enabled",
        defaults.clipboard_history_enabled,
    )?;
    manager.update_setting(
        "clipboard_history_retention_secs",
        defaults.clipboard_history_retention_secs,
    )?;
    manager.update_setting("action_key", defaults.action_key)?;
    manager.update_setting("ignore_fullscreen", defaults.ignore_fullscreen)?;
    manager.update_setting("script_timeout", defaults.script_timeout)?;
    manager.update_setting("ai_temperature", defaults.ai_temperature)?;
    manager.update_setting("ai_max_tokens", defaults.ai_max_tokens)?;
    manager.update_setting("ai_system_prompt", defaults.ai_system_prompt.clone())?;
    manager.update_setting("triggerless_mode", defaults.triggerless_mode)?;
    manager.update_setting("instant_expand", defaults.instant_expand)?;
    manager.update_setting("rpc_port", defaults.rpc_port)?;
    manager.update_setting("rpc_mode", defaults.rpc_mode)?;
    manager.update_setting("rpc_host", defaults.rpc_host.clone())?;
    manager.update_setting("rpc_token", uuid::Uuid::new_v4().to_string())?;
    manager.update_setting("inline_emoji_enabled", defaults.inline_emoji_enabled)?;
    manager.update_setting("inline_datetime_enabled", defaults.inline_datetime_enabled)?;
    manager.update_setting(
        "inline_currency_to_words_enabled",
        defaults.inline_currency_to_words_enabled,
    )?;

    info!("All settings have been reset to factory defaults.");

    if let Err(e) = taurine_core::service::sync_boot(defaults.start_on_boot) {
        warn!("Failed to synchronize OS startup hook: {}", e);
    }

    taurine_core::rpc::notify_daemon_reload();
    if json {
        println!("{}", serde_json::json!({"status": "reset_all"}));
    }
    Ok(())
}

fn render_optional_setting(value: Option<&str>) -> &str {
    value.filter(|v| !v.is_empty()).unwrap_or("<unset>")
}

fn parse_boolean_setting_value(value: &str) -> taurine_core::error::Result<bool> {
    value.to_lowercase().parse::<bool>().map_err(|_| {
        taurine_core::error::Error::Config(format!("Invalid boolean value: {}", value))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::settings::validate_delimiter_conflicts;

    #[test]
    fn render_optional_setting_uses_unset_placeholder() {
        assert_eq!(render_optional_setting(None), "<unset>");
        assert_eq!(render_optional_setting(Some("")), "<unset>");
        assert_eq!(render_optional_setting(Some("openai")), "openai");
    }

    #[test]
    fn core_ai_provider_parser_validates_config_value() {
        assert_eq!(
            taurine_core::ai::AiProvider::try_from("gemini")
                .expect("gemini should parse")
                .as_str(),
            "gemini"
        );
        assert!(
            taurine_core::ai::AiProvider::try_from("unknown").is_err(),
            "invalid provider must be rejected"
        );
    }

    #[test]
    fn parse_boolean_setting_value_accepts_trigger_assist_booleans() {
        assert!(parse_boolean_setting_value("true").unwrap());
        assert!(!parse_boolean_setting_value("false").unwrap());
    }

    #[test]
    fn parse_boolean_setting_value_rejects_invalid_trigger_assist_boolean() {
        assert!(parse_boolean_setting_value("definitely").is_err());
    }

    #[test]
    fn test_settings_json_serializes_all_keys() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let map = value.as_object().unwrap();

        assert!(map.contains_key("trigger_char"));
        assert!(map.contains_key("pause_hotkey"));
        assert!(map.contains_key("wpm"));
        assert!(map.contains_key("spinner_style"));
        assert!(map.contains_key("rpc_mode"));
        assert!(map.contains_key("script_timeout"));
        assert!(map.contains_key("system_tray_enabled"));

        assert_eq!(map["trigger_char"], ">");
        assert_eq!(map["wpm"], 60);
        assert_eq!(map["rpc_mode"], "socket");
        assert_eq!(map["system_tray_enabled"], true);
    }

    #[test]
    fn test_settings_json_optional_fields() {
        let settings = Settings {
            ai_provider: Some("openai".to_string()),
            ai_model: Some("gpt-4".to_string()),
            ai_temperature: Some(0.7),
            ai_max_tokens: Some(2048),
            ai_custom_endpoint: None,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["ai_provider"], "openai");
        assert_eq!(value["ai_model"], "gpt-4");
        assert_eq!(value["ai_custom_endpoint"], serde_json::Value::Null);
    }

    #[test]
    fn test_settings_json_with_unset_optionals() {
        let settings = Settings {
            ai_provider: None,
            ai_model: None,
            ai_temperature: None,
            ai_max_tokens: None,
            ai_system_prompt: None,
            ai_custom_endpoint: None,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["ai_provider"], serde_json::Value::Null);
        assert_eq!(value["ai_model"], serde_json::Value::Null);
        assert_eq!(value["ai_temperature"], serde_json::Value::Null);
    }

    #[test]
    fn validate_trigger_char_conflict_with_inline_ai_trigger_open() {
        let settings = taurine_core::settings::Settings {
            inline_ai_trigger_open: ">".to_string(),
            ..taurine_core::settings::Settings::default()
        };
        let result = validate_delimiter_conflicts(&settings, "trigger_char", ">");
        assert!(result.is_err());
    }

    #[test]
    fn validate_trigger_char_no_conflict_succeeds() {
        let settings = taurine_core::settings::Settings {
            inline_ai_trigger_open: ">>".to_string(),
            inline_ai_trigger_close: "<<".to_string(),
            inline_ai_trigger: "^".to_string(),
            inline_ai_trigger_mode: taurine_core::settings::InlineAiTriggerMode::Asymmetric,
            ..taurine_core::settings::Settings::default()
        };
        let result = validate_delimiter_conflicts(&settings, "trigger_char", "|");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_inline_ai_delimiter_conflict_with_trigger_char() {
        let settings = taurine_core::settings::Settings {
            trigger_char: '>',
            inline_ai_trigger_mode: taurine_core::settings::InlineAiTriggerMode::Symmetric,
            ..taurine_core::settings::Settings::default()
        };
        let result = validate_delimiter_conflicts(&settings, "inline_ai_trigger", ">");
        assert!(result.is_err());
    }

    #[test]
    fn test_settings_all_enum_variants_serialize() {
        let settings = Settings {
            spinner_style: taurine_core::settings::SpinnerStyle::Arc,
            action_key: taurine_core::settings::ActionKey::Space,
            inline_ai_trigger_mode: taurine_core::settings::InlineAiTriggerMode::Symmetric,
            rpc_mode: taurine_core::settings::RpcMode::Tcp,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["spinner_style"], "arc");
        assert_eq!(value["action_key"], "space");
        assert_eq!(value["inline_ai_trigger_mode"], "symmetric");
        assert_eq!(value["rpc_mode"], "tcp");
    }
}
