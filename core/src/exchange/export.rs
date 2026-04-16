use super::{AutomationExport, ExchangePayload, ScriptExport};
use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, decompress};
use rusqlite::Connection;

struct RawAutomationExport {
    name: String,
    description: Option<String>,
    trigger: String,
    output: String,
    action_type: String,
    is_enabled: bool,
    target_os: String,
    tags: String,
    interpreter: Option<String>,
    behavior: Option<String>,
    script_binary: Option<Vec<u8>>,
}

pub fn export_automations(conn: &Connection) -> crate::Result<ExchangePayload> {
    let mut stmt = conn.prepare_cached(
        "SELECT
            a.name,
            a.description,
            a.trigger,
            a.output,
            a.action_type,
            a.is_enabled,
            a.target_os,
            a.tags,
            s.interpreter,
            s.behavior,
            s.compressed_content
         FROM automations a
         LEFT JOIN scripts s ON s.automation_id = a.id
         WHERE a.is_deleted = 0
         ORDER BY a.trigger ASC, a.target_os ASC, a.name ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(RawAutomationExport {
            name: row.get(0)?,
            description: row.get(1)?,
            trigger: row.get(2)?,
            output: row.get(3)?,
            action_type: row.get(4)?,
            is_enabled: row.get(5)?,
            target_os: row.get(6)?,
            tags: row.get(7)?,
            interpreter: row.get(8)?,
            behavior: row.get(9)?,
            script_binary: row.get(10)?,
        })
    })?;

    let mut automations = Vec::new();
    for row in rows {
        automations.push(to_automation_export(row?)?);
    }

    Ok(ExchangePayload::new(automations))
}

fn to_automation_export(row: RawAutomationExport) -> crate::Result<AutomationExport> {
    let tags = serde_json::from_str::<Vec<String>>(&row.tags)?;
    let script = if row.action_type == "script" {
        let interpreter = parse_json_variant::<ScriptInterpreter>(row.interpreter.as_deref())?
            .ok_or_else(|| {
                crate::Error::Service(format!(
                    "Script automation '{}' is missing an interpreter",
                    row.trigger
                ))
            })?;
        let behavior =
            parse_json_variant::<ScriptBehavior>(row.behavior.as_deref())?.ok_or_else(|| {
                crate::Error::Service(format!(
                    "Script automation '{}' is missing a behavior",
                    row.trigger
                ))
            })?;
        let script_binary = row.script_binary.ok_or_else(|| {
            crate::Error::Service(format!(
                "Script automation '{}' is missing script content",
                row.trigger
            ))
        })?;

        Some(ScriptExport {
            interpreter,
            behavior,
            content: decompress(&script_binary)?,
        })
    } else {
        None
    };

    Ok(AutomationExport {
        name: row.name,
        description: row.description,
        trigger: row.trigger,
        output: row.output,
        action_type: row.action_type,
        is_enabled: row.is_enabled,
        target_os: row.target_os,
        tags,
        script,
    })
}

fn parse_json_variant<T>(value: Option<&str>) -> crate::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    match value {
        Some(value) => {
            let trimmed = value.trim_matches('"');
            let parsed = serde_json::from_str::<T>(&format!("\"{}\"", trimmed))?;
            Ok(Some(parsed))
        }
        None => Ok(None),
    }
}
