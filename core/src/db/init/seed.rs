use crate::db::crud::{get_setting, upsert_setting};
use rusqlite::{Connection, Result};
use tracing::debug;

pub fn ensure_defaults(conn: &Connection) -> Result<()> {
    // The settings table stores values as JSON. A single-character string is
    // stored as a JSON string literal — i.e. with surrounding double-quotes.
    let trigger_val = get_setting(conn, "trigger_char")?;
    if trigger_val.is_none() {
        debug!("Default 'trigger_char' missing. Seeding database with '>'.");
        // Stored as the JSON string ">" (with quotes) per the settings schema.
        upsert_setting(conn, "trigger_char", r#"">""#)?;
    }

    let start_on_boot_val = get_setting(conn, "start_on_boot")?;
    if start_on_boot_val.is_none() {
        debug!("Default 'start_on_boot' missing. Seeding database with 'true'.");
        // Stored as a JSON boolean literal.
        upsert_setting(conn, "start_on_boot", "true")?;
    }

    let inline_tab_completion_enabled_val = get_setting(conn, "inline_tab_completion_enabled")?;
    if inline_tab_completion_enabled_val.is_none() {
        debug!("Default 'inline_tab_completion_enabled' missing. Seeding database with 'true'.");
        upsert_setting(conn, "inline_tab_completion_enabled", "true")?;
    }

    let inline_history_enabled_val = get_setting(conn, "inline_history_enabled")?;
    if inline_history_enabled_val.is_none() {
        debug!("Default 'inline_history_enabled' missing. Seeding database with 'true'.");
        upsert_setting(conn, "inline_history_enabled", "true")?;
    }

    let wpm_val = get_setting(conn, "wpm")?;
    if wpm_val.is_none() {
        debug!("Default 'wpm' missing. Seeding database with '60'.");
        upsert_setting(conn, "wpm", "60")?;
    }

    // Global daemon pause toggle hotkey.
    // Stored as a JSON string literal. Default: Alt + ` (Alt + Backtick).
    let pause_hotkey_val = get_setting(conn, "pause_hotkey")?;
    if pause_hotkey_val.is_none() {
        debug!("Default 'pause_hotkey' missing. Seeding database with 'Alt + `'.");
        upsert_setting(conn, "pause_hotkey", r#""Alt + `""#)?;
    }

    let pause_notifications_enabled_val = get_setting(conn, "pause_notifications_enabled")?;
    if pause_notifications_enabled_val.is_none() {
        debug!("Default 'pause_notifications_enabled' missing. Seeding database with 'true'.");
        // Stored as a JSON boolean literal.
        upsert_setting(conn, "pause_notifications_enabled", "true")?;
    }

    let spinner_style_val = get_setting(conn, "spinner_style")?;
    if spinner_style_val.is_none() {
        debug!("Default 'spinner_style' missing. Seeding database with 'braille'.");
        // Stored as a JSON string literal.
        upsert_setting(conn, "spinner_style", r#""braille""#)?;
    }

    let ignore_fullscreen_val = get_setting(conn, "ignore_fullscreen")?;
    if ignore_fullscreen_val.is_none() {
        debug!("Default 'ignore_fullscreen' missing. Seeding database with 'true'.");
        upsert_setting(conn, "ignore_fullscreen", "true")?;
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
