use rusqlite::{Connection, Result};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiPreset {
    pub name: String,
    pub prompt: String,
}

pub fn add_preset(conn: &Connection, name: &str, prompt: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO ai_presets (name, prompt) VALUES (?1, ?2)
         ON CONFLICT(name) DO UPDATE SET prompt = excluded.prompt",
        [name, prompt],
    )?;
    Ok(())
}

pub fn remove_preset(conn: &Connection, name: &str) -> Result<bool> {
    let affected = conn.execute("DELETE FROM ai_presets WHERE name = ?1", [name])?;
    Ok(affected > 0)
}

pub fn list_presets(conn: &Connection) -> Result<Vec<AiPreset>> {
    let mut stmt = conn.prepare("SELECT name, prompt FROM ai_presets ORDER BY name ASC")?;
    let preset_iter = stmt.query_map([], |row| {
        Ok(AiPreset {
            name: row.get(0)?,
            prompt: row.get(1)?,
        })
    })?;

    let mut presets = Vec::new();
    for preset in preset_iter {
        presets.push(preset?);
    }
    Ok(presets)
}

pub fn get_preset(conn: &Connection, name: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT prompt FROM ai_presets WHERE name = ?1")?;
    let mut rows = stmt.query([name])?;

    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}
