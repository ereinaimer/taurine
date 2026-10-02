use rusqlite::Connection;
use taurine_core::db::crud::{
    InvocationType, find_parent_by_invocation, get_trigger, set_trigger_enabled,
};
use taurine_core::db::init;
use taurine_core::error::{Error, Result};

pub fn find_trigger_id(conn: &Connection, identifier: &str) -> Result<String> {
    // 1. Direct ID lookup
    if let Ok(Some(row)) = get_trigger(conn, identifier)
        && !row.is_deleted
    {
        return Ok(row.id);
    }

    // 2. Lookup by trigger name (case-insensitive)
    let mut stmt = conn.prepare(
        "SELECT id FROM triggers WHERE LOWER(name) = LOWER(?1) AND is_deleted = 0 LIMIT 1",
    )?;
    let by_name: Option<String> = stmt.query_row([identifier], |row| row.get(0)).ok();
    if let Some(id) = by_name {
        return Ok(id);
    }

    // 3. Lookup by invocation across word, hotkey, regex, voice
    for invocation_type in [
        InvocationType::Word,
        InvocationType::Hotkey,
        InvocationType::Regex,
        InvocationType::Voice,
    ] {
        if let Some(pid) = find_parent_by_invocation(conn, invocation_type, identifier)? {
            return Ok(pid);
        }
    }

    Err(Error::NotFound(format!("Trigger '{identifier}' not found")))
}

pub fn execute_enable(trigger: &str, json: bool) -> Result<()> {
    let conn = init::setup()?;
    let trigger_id = find_trigger_id(&conn, trigger)?;
    set_trigger_enabled(&conn, &trigger_id, true)?;

    let name = get_trigger(&conn, &trigger_id)?
        .map(|r| r.name)
        .unwrap_or_else(|| trigger_id.clone());

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "enabled",
                "id": trigger_id,
                "name": name,
            })
        );
    } else {
        println!("Trigger '{name}' enabled.");
    }

    Ok(())
}

pub fn execute_disable(trigger: &str, json: bool) -> Result<()> {
    let conn = init::setup()?;
    let trigger_id = find_trigger_id(&conn, trigger)?;
    set_trigger_enabled(&conn, &trigger_id, false)?;

    let name = get_trigger(&conn, &trigger_id)?
        .map(|r| r.name)
        .unwrap_or_else(|| trigger_id.clone());

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "disabled",
                "id": trigger_id,
                "name": name,
            })
        );
    } else {
        println!("Trigger '{name}' disabled.");
    }

    Ok(())
}
