use comfy_table::{Table, presets::NOTHING};
use taurine_core::db::init;
use taurine_core::settings::{Settings, SettingsManager};
use tracing::{info, warn};

pub fn execute_list() -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);
    let settings = manager.load_all();

    let mut table = Table::new();
    table.load_preset(NOTHING);
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
        }
        _ => {
            warn!("Unknown setting key: {}", key);
            return Ok(());
        }
    }

    taurine_core::rpc::notify_daemon_reload();
    Ok(())
}
