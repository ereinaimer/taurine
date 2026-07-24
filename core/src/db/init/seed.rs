use crate::db::crud::{get_all_settings, upsert_setting};
use rusqlite::{Connection, Result};
use tracing::debug;

pub fn ensure_defaults(conn: &Connection) -> Result<()> {
    let existing = get_all_settings(conn).unwrap_or_default();

    // The settings table stores values as JSON. A single-character string is
    // stored as a JSON string literal — i.e. with surrounding double-quotes.
    if !existing.contains_key("trigger_char") {
        debug!("Default 'trigger_char' missing. Seeding database with '>'.");
        // Stored as the JSON string ">" (with quotes) per the settings schema.
        upsert_setting(conn, "trigger_char", r#"">""#)?;
    }

    if !existing.contains_key("start_on_boot") {
        debug!("Default 'start_on_boot' missing. Seeding database with 'true'.");
        // Stored as a JSON boolean literal.
        upsert_setting(conn, "start_on_boot", "true")?;
    }

    if !existing.contains_key("inline_tab_completion_enabled") {
        debug!("Default 'inline_tab_completion_enabled' missing. Seeding database with 'true'.");
        upsert_setting(conn, "inline_tab_completion_enabled", "true")?;
    }

    if !existing.contains_key("inline_history_enabled") {
        debug!("Default 'inline_history_enabled' missing. Seeding database with 'true'.");
        upsert_setting(conn, "inline_history_enabled", "true")?;
    }

    if !existing.contains_key("wpm") {
        debug!("Default 'wpm' missing. Seeding database with '60'.");
        upsert_setting(conn, "wpm", "60")?;
    }

    // Global daemon pause toggle hotkey.
    // Stored as a JSON string literal. Default: Alt + ` (Alt + Backtick).
    if !existing.contains_key("pause_hotkey") {
        debug!("Default 'pause_hotkey' missing. Seeding database with 'Alt + `'.");
        upsert_setting(conn, "pause_hotkey", r#""Alt + `""#)?;
    }

    if !existing.contains_key("pause_notifications_enabled") {
        debug!("Default 'pause_notifications_enabled' missing. Seeding database with 'true'.");
        // Stored as a JSON boolean literal.
        upsert_setting(conn, "pause_notifications_enabled", "true")?;
    }

    if !existing.contains_key("spinner_style") {
        debug!("Default 'spinner_style' missing. Seeding database with 'braille'.");
        // Stored as a JSON string literal.
        upsert_setting(conn, "spinner_style", r#""braille""#)?;
    }

    if !existing.contains_key("ignore_fullscreen") {
        debug!("Default 'ignore_fullscreen' missing. Seeding database with 'true'.");
        upsert_setting(conn, "ignore_fullscreen", "true")?;
    }

    if !existing.contains_key("system_tray_enabled") {
        debug!("Default 'system_tray_enabled' missing. Seeding database with 'true'.");
        upsert_setting(conn, "system_tray_enabled", "true")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::{delete_setting, get_setting_value};
    use crate::testing::open_test_db;

    #[test]
    fn ensure_defaults_seeds_inline_trigger_assist_settings_for_new_databases() {
        let (_dir, conn) = open_test_db();

        assert_eq!(
            get_setting_value(&conn, "inline_tab_completion_enabled").unwrap(),
            Some("true".to_string())
        );
        assert_eq!(
            get_setting_value(&conn, "inline_history_enabled").unwrap(),
            Some("true".to_string())
        );
    }

    #[test]
    fn ensure_defaults_reconciles_missing_inline_trigger_assist_settings() {
        let (_dir, conn) = open_test_db();

        delete_setting(&conn, "inline_tab_completion_enabled").unwrap();
        delete_setting(&conn, "inline_history_enabled").unwrap();

        ensure_defaults(&conn).unwrap();

        assert_eq!(
            get_setting_value(&conn, "inline_tab_completion_enabled").unwrap(),
            Some("true".to_string())
        );
        assert_eq!(
            get_setting_value(&conn, "inline_history_enabled").unwrap(),
            Some("true".to_string())
        );
    }
}
