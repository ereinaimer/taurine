use std::path::{Path, PathBuf};

use super::{
    ExchangePayload, ScriptExport, SettingExport, StatExport, TriggerExport, crypto,
    encode_plaintext_payload, serialize_payload,
};
use crate::db::crud::TriggerType;
use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, decompress};
use rusqlite::Connection;
use rusqlite::types::Type;
use time::OffsetDateTime;
use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportOptions {
    pub include_settings: bool,
    pub include_stats: bool,
    pub include_sensitive_settings: bool,
}

struct RawTriggerExport {
    id: String,
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

pub fn export_triggers(
    conn: &Connection,
    options: ExportOptions,
) -> crate::Result<ExchangePayload> {
    let mut stmt = conn.prepare_cached(
        "SELECT
            a.id,
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
         FROM triggers a
         LEFT JOIN scripts s ON s.trigger_id = a.id
         WHERE a.is_deleted = 0
         ORDER BY a.trigger ASC, a.target_os ASC, a.name ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(RawTriggerExport {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            trigger_type: TriggerType::parse_db(&row.get::<_, String>(3)?).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(3, Type::Text, Box::new(err))
            })?,
            trigger: row.get(4)?,
            output: row.get(5)?,
            action_type: row.get(6)?,
            is_enabled: row.get(7)?,
            target_os: row.get(8)?,
            tags: row.get(9)?,
            usage_count: row.get(10)?,
            last_used_at: row.get(11)?,
            interpreter: row.get(12)?,
            behavior: row.get(13)?,
            script_binary: row.get(14)?,
        })
    })?;

    let mut triggers = Vec::new();
    for row in rows {
        triggers.push(to_trigger_export(conn, row?, options)?);
    }

    let settings = if options.include_settings {
        Some(export_settings(conn, options.include_sensitive_settings)?)
    } else {
        None
    };

    let stats = if options.include_stats {
        Some(export_stats(conn)?)
    } else {
        None
    };

    Ok(ExchangePayload {
        schema_version: super::EXCHANGE_SCHEMA_VERSION,
        triggers,
        settings,
        stats,
    })
}

pub fn default_export_filename() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    default_export_filename_for_timestamp(now)
}

pub fn default_export_path() -> crate::Result<PathBuf> {
    Ok(default_export_path_for_cwd(
        &std::env::current_dir()?,
        OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc()),
    ))
}

pub fn resolve_export_path(path: Option<PathBuf>) -> crate::Result<PathBuf> {
    match path {
        Some(path) => {
            let is_dir = path.is_dir()
                || path.to_string_lossy().ends_with('/')
                || path.to_string_lossy().ends_with('\\');
            if is_dir {
                let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
                Ok(get_unique_export_path(&path, now))
            } else {
                Ok(ensure_tau_extension(path))
            }
        }
        None => default_export_path(),
    }
}

pub fn ensure_tau_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("tau");
    }
    path
}

pub fn encode_exchange_blob(
    payload: &ExchangePayload,
    encrypt: bool,
    password: Option<&str>,
) -> crate::Result<Vec<u8>> {
    if !encrypt {
        return encode_plaintext_payload(payload);
    }

    let password = password.ok_or_else(|| {
        crate::Error::Config("An encryption password is required for TAU1 exports".to_string())
    })?;

    let mut serialized = serialize_payload(payload)?;
    let result = crypto::encrypt(&serialized, password);
    serialized.zeroize();
    result
}

fn to_trigger_export(
    conn: &Connection,
    row: RawTriggerExport,
    options: ExportOptions,
) -> crate::Result<TriggerExport> {
    let tags = serde_json::from_str::<Vec<String>>(&row.tags)?;
    let script = if row.action_type == "script" {
        let interpreter = parse_json_variant::<ScriptInterpreter>(row.interpreter.as_deref())?
            .ok_or_else(|| {
                crate::Error::Service(format!(
                    "Script trigger '{}' is missing an interpreter",
                    row.trigger
                ))
            })?;
        let behavior =
            parse_json_variant::<ScriptBehavior>(row.behavior.as_deref())?.ok_or_else(|| {
                crate::Error::Service(format!(
                    "Script trigger '{}' is missing a behavior",
                    row.trigger
                ))
            })?;
        let script_binary = row.script_binary.ok_or_else(|| {
            crate::Error::Service(format!(
                "Script trigger '{}' is missing script content",
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

    let mut asset_stmt = conn.prepare_cached(
        "SELECT id, mime_type, compressed_content FROM assets WHERE trigger_id = ?1",
    )?;
    let asset_rows = asset_stmt.query_map([&row.id], |r| {
        let id: String = r.get(0)?;
        let mime_type: String = r.get(1)?;
        let compressed: Vec<u8> = r.get(2)?;
        Ok(super::AssetExport {
            id,
            mime_type,
            compressed_content_hex: hex::encode(compressed),
        })
    })?;

    let mut assets = Vec::new();
    for asset in asset_rows {
        assets.push(asset?);
    }

    Ok(TriggerExport {
        name: row.name,
        description: row.description,
        trigger_type: row.trigger_type,
        trigger: row.trigger,
        output: row.output,
        action_type: row.action_type,
        is_enabled: row.is_enabled,
        target_os: row.target_os,
        tags,
        usage_count: options.include_stats.then_some(row.usage_count),
        last_used_at: if options.include_stats {
            row.last_used_at
        } else {
            None
        },
        script,
        assets,
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
        "SELECT key, CAST(value AS TEXT)
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

fn export_stats(conn: &Connection) -> crate::Result<Vec<StatExport>> {
    let mut stmt = conn.prepare_cached(
        "SELECT date, executions, ai_executions, keystrokes_saved, time_saved_ms
         FROM stats
         ORDER BY date ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(StatExport {
            date: row.get(0)?,
            executions: row.get(1)?,
            ai_executions: row.get(2)?,
            keystrokes_saved: row.get(3)?,
            time_saved_ms: row.get(4)?,
        })
    })?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(row?);
    }

    Ok(stats)
}

pub(crate) fn is_sensitive_setting_key(key: &str) -> bool {
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

fn default_export_filename_for_timestamp(now: OffsetDateTime) -> String {
    let year = now.year() % 100;
    format!("tx-{:02}{:02}{:02}.tau", year, now.month() as u8, now.day())
}

fn get_unique_export_path(directory: &Path, now: OffsetDateTime) -> PathBuf {
    let year = now.year() % 100;
    let base_name = format!("tx-{:02}{:02}{:02}", year, now.month() as u8, now.day());

    let mut candidate = directory.join(format!("{}.tau", base_name));
    let mut counter = 1;
    while candidate.exists() {
        candidate = directory.join(format!("{}_{}.tau", base_name, counter));
        counter += 1;
    }
    candidate
}

fn default_export_path_for_cwd(cwd: &Path, now: OffsetDateTime) -> PathBuf {
    get_unique_export_path(cwd, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::ENCRYPTED_MAGIC_HEADER;
    use time::macros::datetime;

    #[test]
    fn ensure_tau_extension_appends_when_missing() {
        let resolved = ensure_tau_extension(PathBuf::from("my_scripts"));
        assert_eq!(resolved, PathBuf::from("my_scripts.tau"));
    }

    #[test]
    fn ensure_tau_extension_preserves_existing_extension() {
        let resolved = ensure_tau_extension(PathBuf::from("custom-pack.tau"));
        assert_eq!(resolved, PathBuf::from("custom-pack.tau"));
    }

    #[test]
    fn default_export_filename_uses_tau_extension() {
        let filename = default_export_filename_for_timestamp(datetime!(2026-04-30 09:15:00 +05:30));
        assert_eq!(filename, "tx-260430.tau");
    }

    #[test]
    fn default_export_path_uses_current_working_directory() {
        let path =
            default_export_path_for_cwd(Path::new("C:/tmp"), datetime!(2026-04-30 09:15:00 +05:30));
        assert_eq!(path, PathBuf::from("C:/tmp/tx-260430.tau"));
    }

    #[test]
    fn encode_exchange_blob_uses_taup_for_plaintext_exports() {
        let blob = encode_exchange_blob(&ExchangePayload::new(vec![]), false, None).unwrap();
        assert_eq!(&blob[..4], &super::super::PLAINTEXT_MAGIC_HEADER);
    }

    #[test]
    fn encode_exchange_blob_uses_tau1_for_encrypted_exports() {
        let blob =
            encode_exchange_blob(&ExchangePayload::new(vec![]), true, Some("hunter2")).unwrap();
        assert_eq!(&blob[..4], &ENCRYPTED_MAGIC_HEADER);
        assert!(
            !blob
                .windows(b"schema_version".len())
                .any(|window| window == b"schema_version"),
            "Encrypted export should be opaque"
        );
    }
}
