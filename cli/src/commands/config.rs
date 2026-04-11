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

    info!("All settings have been reset to factory defaults.");

    if let Err(e) = crate::service::sync_boot(defaults.start_on_boot) {
        warn!("Failed to synchronize OS startup hook: {}", e);
    }

    taurine_core::rpc::notify_daemon_reload();
    Ok(())
}
