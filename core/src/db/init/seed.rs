use crate::db::crud::{get_setting, upsert_setting};
use rusqlite::{Connection, Result};
use tracing::debug;

pub fn ensure_defaults(conn: &Connection) -> Result<()> {
    debug!("Checking database for required default settings");

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

    // Global daemon pause toggle hotkey.
    // Stored as a JSON string literal. Default: Alt + ` (Alt + Backtick).
    let pause_hotkey_val = get_setting(conn, "pause_hotkey")?;
    if pause_hotkey_val.is_none() {
        debug!("Default 'pause_hotkey' missing. Seeding database with 'Alt + `'.");
        upsert_setting(conn, "pause_hotkey", r#""Alt + `""#)?;
    }

    Ok(())
}
