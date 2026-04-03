use crate::db::crud::{get_setting, upsert_setting};
use rusqlite::{Connection, Result};
use tracing::{debug, info};

pub fn ensure_defaults(conn: &Connection) -> Result<()> {
    debug!("Checking database for required default settings");

    let trigger_val = get_setting(conn, "trigger_char")?;

    if trigger_val.is_none() {
        info!("Default 'trigger_char' missing. Seeding database with '>'.");
        upsert_setting(conn, "trigger_char", r#"">""#)?;
    }

    Ok(())
}
