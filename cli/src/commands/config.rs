use comfy_table::{Table, TableComponent, modifiers, presets};
use taurine_core::db::init;
use taurine_core::settings::{Settings, SettingsManager};
use tracing::{info, warn};

pub fn execute_list() -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);
    let settings = manager.load_all();

    let mut table = Table::new();
    table
        .load_preset(presets::UTF8_FULL_CONDENSED)
        .apply_modifier(modifiers::UTF8_ROUND_CORNERS);

    table.set_style(TableComponent::HeaderLines, '─');
    table.set_style(TableComponent::LeftHeaderIntersection, '├');
    table.set_style(TableComponent::MiddleHeaderIntersections, '┼');
    table.set_style(TableComponent::RightHeaderIntersection, '┤');
    table.set_style(TableComponent::VerticalLines, '│');

    table.set_header(vec!["KEY", "VALUE"]);

    table.add_row(vec!["trigger_char", &settings.trigger_char.to_string()]);
    table.add_row(vec!["pause_hotkey", &settings.pause_hotkey]);
    table.add_row(vec![
        "pause_notifications_enabled",
        &settings.pause_notifications_enabled.to_string(),
    ]);
    table.add_row(vec!["start_on_boot", &settings.start_on_boot.to_string()]);
    table.add_row(vec![
        "spinner_style",
        &format!("{:?}", settings.spinner_style).to_lowercase(),
    ]);
    table.add_row(vec![
        "ai_provider",
        render_optional_setting(settings.ai_provider.as_deref()),
    ]);
    table.add_row(vec![
        "ai_model",
        render_optional_setting(settings.ai_model.as_deref()),
    ]);
    table.add_row(vec![
        "ai_custom_endpoint",
        render_optional_setting(settings.ai_custom_endpoint.as_deref()),
    ]);
    table.add_row(vec![
        "inline_ai_delimiter",
        &settings.inline_ai_delimiter.to_string(),
    ]);

    println!("{}", table);

    Ok(())
}

pub fn execute_set(key: String, value: String) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);

    let actual_key = Settings::resolve_key(&key);

    match actual_key {
        "trigger_char" => {
            if let Some(c) = value.chars().next() {
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
        "pause_notifications_enabled" | "start_on_boot" => {
            let b = value.to_lowercase().parse::<bool>().map_err(|_| {
                taurine_core::error::Error::Config(format!("Invalid boolean value: {}", value))
            })?;
            manager.update_setting(actual_key, b)?;
            info!("Updated {} to: {}", actual_key, b);

            if actual_key == "start_on_boot"
                && let Err(e) = crate::service::sync_boot(b)
            {
                warn!("Failed to synchronize OS startup hook: {}", e);
            }
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
        "ai_provider" => {
            let provider = taurine_core::ai::AiProvider::try_from(value.as_str())?;
            manager.update_setting(actual_key, Some(provider.as_str().to_string()))?;
            info!("Updated ai_provider to: {}", provider.as_str());
        }
        "ai_model" | "ai_custom_endpoint" => {
            let val = value.trim();
            if val.is_empty() {
                return Err(taurine_core::error::Error::Config(format!(
                    "Invalid {actual_key} value: must not be empty"
                )));
            }
            manager.update_setting(actual_key, Some(val.to_string()))?;
            info!("Updated {actual_key} to: {val}");
        }
        "inline_ai_delimiter" => {
            if let Some(c) = value.chars().next() {
                manager.update_setting(actual_key, c)?;
                info!("Updated inline_ai_delimiter to: {}", c);
            } else {
                warn!("Invalid delimiter character provided.");
            }
        }
        _ => {
            warn!("Unknown setting key: {}", key);
            return Ok(());
        }
    }

    taurine_core::rpc::notify_daemon_reload();
    Ok(())
}
pub fn execute_reset(key: String) -> taurine_core::error::Result<()> {
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
        "start_on_boot" => {
            manager.update_setting(actual_key, defaults.start_on_boot)?;
            info!("Reset start_on_boot to default: {}", defaults.start_on_boot);

            if let Err(e) = crate::service::sync_boot(defaults.start_on_boot) {
                warn!("Failed to synchronize OS startup hook: {}", e);
            }
        }
        "spinner_style" => {
            manager.update_setting(actual_key, defaults.spinner_style)?;
            info!(
                "Reset spinner_style to default: {:?}",
                defaults.spinner_style
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
        "ai_custom_endpoint" => {
            manager.update_setting(actual_key, defaults.ai_custom_endpoint.clone())?;
            info!("Reset ai_custom_endpoint to default: <unset>");
        }
        "inline_ai_delimiter" => {
            manager.update_setting(actual_key, defaults.inline_ai_delimiter)?;
            info!(
                "Reset inline_ai_delimiter to default: {}",
                defaults.inline_ai_delimiter
            );
        }
        _ => {
            warn!("Unknown setting key: {}", key);
            return Ok(());
        }
    }

    taurine_core::rpc::notify_daemon_reload();
    Ok(())
}

pub fn execute_reset_all() -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);
    let defaults = Settings::default();

    manager.update_setting("trigger_char", defaults.trigger_char)?;
    manager.update_setting("pause_hotkey", &defaults.pause_hotkey)?;
    manager.update_setting(
        "pause_notifications_enabled",
        defaults.pause_notifications_enabled,
    )?;
    manager.update_setting("start_on_boot", defaults.start_on_boot)?;
    manager.update_setting("spinner_style", defaults.spinner_style)?;
    manager.update_setting("ai_provider", defaults.ai_provider.clone())?;
    manager.update_setting("ai_model", defaults.ai_model.clone())?;
    manager.update_setting("ai_custom_endpoint", defaults.ai_custom_endpoint.clone())?;
    manager.update_setting("inline_ai_delimiter", defaults.inline_ai_delimiter)?;

    info!("All settings have been reset to factory defaults.");

    if let Err(e) = crate::service::sync_boot(defaults.start_on_boot) {
        warn!("Failed to synchronize OS startup hook: {}", e);
    }

    taurine_core::rpc::notify_daemon_reload();
    Ok(())
}

fn render_optional_setting(value: Option<&str>) -> &str {
    value.filter(|v| !v.is_empty()).unwrap_or("<unset>")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
