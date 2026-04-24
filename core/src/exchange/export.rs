use super::{AutomationExport, ExchangePayload, MetricExport, ScriptExport, SettingExport};
use crate::db::crud::TriggerType;
use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, decompress};
use rusqlite::Connection;
use rusqlite::types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportOptions {
    pub include_settings: bool,
    pub include_metrics: bool,
    pub include_sensitive_settings: bool,
}

struct RawAutomationExport {
    name: String,
    description: Option<String>,
    trigger_type: TriggerType,
    trigger: String,
    output: String,
    action_type: String,
    is_enabled: bool,
    target_os: String,
    tags: String,
    usage_count: i64,
    last_used_at: Option<i64>,
    interpreter: Option<String>,
    behavior: Option<String>,
    script_binary: Option<Vec<u8>>,
}

pub fn export_automations(
    conn: &Connection,
    options: ExportOptions,
) -> crate::Result<ExchangePayload> {
    let mut stmt = conn.prepare_cached(
        "SELECT
            a.name,
            a.description,
            a.trigger_type,
            a.trigger,
            a.output,
            a.action_type,
            a.is_enabled,
            a.target_os,
            a.tags,
            a.usage_count,
            a.last_used_at,
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
            trigger_type: TriggerType::parse_db(&row.get::<_, String>(2)?).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(2, Type::Text, Box::new(err))
            })?,
            trigger: row.get(3)?,
            output: row.get(4)?,
            action_type: row.get(5)?,
            is_enabled: row.get(6)?,
            target_os: row.get(7)?,
            tags: row.get(8)?,
            usage_count: row.get(9)?,
            last_used_at: row.get(10)?,
            interpreter: row.get(11)?,
            behavior: row.get(12)?,
            script_binary: row.get(13)?,
        })
    })?;

    let mut automations = Vec::new();
    for row in rows {
        automations.push(to_automation_export(row?, options)?);
    }

    let settings = if options.include_settings {
        Some(export_settings(conn, options.include_sensitive_settings)?)
    } else {
        None
    };

    let metrics = if options.include_metrics {
        Some(export_metrics(conn)?)
    } else {
        None
    };

    Ok(ExchangePayload {
        schema_version: super::EXCHANGE_SCHEMA_VERSION,
        automations,
        settings,
        metrics,
    })
}

fn to_automation_export(
    row: RawAutomationExport,
    options: ExportOptions,
) -> crate::Result<AutomationExport> {
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
        trigger_type: row.trigger_type,
        trigger: row.trigger,
        output: row.output,
        action_type: row.action_type,
        is_enabled: row.is_enabled,
        target_os: row.target_os,
        tags,
        usage_count: options.include_metrics.then_some(row.usage_count),
        last_used_at: if options.include_metrics {
            row.last_used_at
        } else {
            None
        },
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

fn export_settings(
    conn: &Connection,
    include_sensitive_settings: bool,
) -> crate::Result<Vec<SettingExport>> {
    let mut stmt = conn.prepare_cached(
        "SELECT key, value
         FROM settings
         ORDER BY key ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(SettingExport {
            key: row.get(0)?,
            value: row.get(1)?,
        })
    })?;

    let mut settings = Vec::new();
    for row in rows {
        let setting = row?;
        if include_sensitive_settings || !is_sensitive_setting_key(&setting.key) {
            settings.push(setting);
        }
    }

    Ok(settings)
}

fn export_metrics(conn: &Connection) -> crate::Result<Vec<MetricExport>> {
    let mut stmt = conn.prepare_cached(
        "SELECT date, executions, keystrokes_saved
         FROM metrics
         ORDER BY date ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(MetricExport {
            date: row.get(0)?,
            executions: row.get(1)?,
            keystrokes_saved: row.get(2)?,
        })
    })?;

    let mut metrics = Vec::new();
    for row in rows {
        metrics.push(row?);
    }

    Ok(metrics)
}

fn is_sensitive_setting_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "access_key",
        "private_key",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}
