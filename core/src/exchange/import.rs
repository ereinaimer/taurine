use super::ExchangePayload;
use crate::db::crud::{upsert_automation, upsert_script};
use crate::engine::shell::compress;
use rusqlite::Connection;
use uuid::Uuid;

pub fn import_automations(conn: &Connection, payload: &ExchangePayload) -> crate::Result<usize> {
    payload.validate_schema_version()?;

    let mut imported = 0usize;
    for automation in &payload.automations {
        let id = Uuid::new_v4().to_string();
        let tags_json = serde_json::to_string(&automation.tags)?;

        upsert_automation(
            conn,
            &id,
            &automation.name,
            automation.description.as_deref(),
            &automation.trigger,
            &automation.output,
            &automation.action_type,
            &automation.target_os,
            &tags_json,
            0,
            None,
        )?;

        if !automation.is_enabled {
            conn.execute(
                "UPDATE automations
                 SET is_enabled = 0
                 WHERE id = ?1",
                [&id],
            )?;
        }

        if automation.action_type == "script" {
            let script = automation.script.as_ref().ok_or_else(|| {
                crate::Error::Config(format!(
                    "Script automation '{}' is missing script data",
                    automation.trigger
                ))
            })?;
            let compressed = compress(&script.content)?;
            upsert_script(conn, &id, script.interpreter, script.behavior, &compressed)?;
        }

        imported += 1;
    }

    Ok(imported)
}
