use crate::db::crud::{get_setting, upsert_automation, upsert_setting};
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

    // ──────────────────────────────────────────────────────────────────────────
    // TEST SEED DATA — FOR DEVELOPMENT / TESTING ONLY.
    // TODO: Remove this entire block before shipping to production.
    // ──────────────────────────────────────────────────────────────────────────
    seed_test_automations(conn)?;

    Ok(())
}

/// Inserts a small set of canned automations for manual testing of the
/// expansion engine. These rows are idempotent (upsert semantics) so running
/// the daemon multiple times won't duplicate them.
///
/// # ⚠️ Testing only
/// TODO: Remove this function and its call site before shipping to production.
fn seed_test_automations(conn: &Connection) -> Result<()> {
    let rows: &[(&str, &str, &str, &str)] = &[
        // (id, trigger, payload, name)
        ("seed-uuid-0001", "gm", "Good morning!", "Good Morning"),
        ("seed-uuid-0002", "ty", "Thank you so much!", "Thank You"),
        ("seed-uuid-0003", "shrug", r"¯\_(ツ)_/¯", "Shrug"),
        ("seed-uuid-0004", "brb", "Be right back!", "Be Right Back"),
    ];

    for (id, trigger, payload, name) in rows {
        upsert_automation(
            conn, id, name, None, // description
            trigger, payload, "text", // action_type
            false,  // is_regex
            "all",  // target_os — matches every platform
            "[]",   // tags (empty JSON array)
            0,      // usage_count
            None,   // last_used_at
        )?;
    }

    Ok(())
}
