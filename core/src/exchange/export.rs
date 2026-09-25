use std::path::{Path, PathBuf};

use super::{AliasExport, ExchangePayload, ScriptExport, TriggerExport, crypto, serialize_payload};
use crate::db::crud::{InvocationType, TriggerType, display_alias, list_aliases};
use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, decompress};
use rusqlite::Connection;
use time::OffsetDateTime;
use zeroize::Zeroize;

struct RawTriggerExport {
    id: String,
    name: String,
    description: Option<String>,
    output: String,
    action_type: String,
    is_enabled: bool,
    target_os: String,
    tags: String,
    interpreter: Option<String>,
    behavior: Option<String>,
    script_binary: Option<Vec<u8>>,
}

pub fn export_triggers(conn: &Connection) -> crate::Result<ExchangePayload> {
    let mut stmt = conn.prepare_cached(
        "SELECT
            a.id,
            a.name,
            a.description,
            a.output,
            a.action_type,
            a.is_enabled,
            a.target_os,
            a.tags,
            s.interpreter,
            s.behavior,
            s.compressed_content
         FROM triggers a
         LEFT JOIN scripts s ON s.trigger_id = a.id
         WHERE a.is_deleted = 0
         ORDER BY a.name ASC, a.target_os ASC, a.id ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(RawTriggerExport {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
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

    let mut triggers = Vec::new();
    for row in rows {
        triggers.push(to_trigger_export(conn, row?)?);
    }

    Ok(ExchangePayload {
        schema_version: super::EXCHANGE_SCHEMA_VERSION,
        triggers,
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
    password: Option<&str>,
) -> crate::Result<Vec<u8>> {
    let mut serialized = serialize_payload(payload)?;
    let result = crypto::encrypt(&serialized, password);
    serialized.zeroize();
    result
}

pub fn write_export_file(path: &Path, data: &[u8]) -> crate::Result<()> {
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(data)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, data)?;
    }
    Ok(())
}

fn to_trigger_export(conn: &Connection, row: RawTriggerExport) -> crate::Result<TriggerExport> {
    let tags = serde_json::from_str::<Vec<String>>(&row.tags)?;

    let alias_rows = list_aliases(conn, &row.id).map_err(|err| {
        crate::Error::Service(format!(
            "failed to load aliases for trigger '{}': {err}",
            row.name
        ))
    })?;
    let aliases = alias_rows
        .iter()
        .map(|alias| AliasExport {
            invocation_type: alias.invocation_type,
            invocation: alias.invocation.clone(),
            require_confirmation: alias.require_confirmation,
        })
        .collect::<Vec<_>>();
    let (trigger_type, trigger) = match display_alias(&alias_rows) {
        Some(display) => match display.invocation_type {
            InvocationType::Word => (TriggerType::Word, display.invocation.clone()),
            InvocationType::Hotkey => (TriggerType::Hotkey, display.invocation.clone()),
            InvocationType::Regex => (TriggerType::Regex, display.invocation.clone()),
            // Legacy schema has no voice variant; degrade to word (new-binary
            // import restores voice from `aliases`, so no data loss there).
            InvocationType::Voice => (TriggerType::Word, display.invocation.clone()),
        },
        None => (TriggerType::Word, String::new()),
    };

    let script = if row.action_type == "script" {
        let interpreter = parse_json_variant::<ScriptInterpreter>(row.interpreter.as_deref())?
            .ok_or_else(|| {
                crate::Error::Service(format!(
                    "Script trigger '{trigger}' is missing an interpreter",
                ))
            })?;
        let behavior =
            parse_json_variant::<ScriptBehavior>(row.behavior.as_deref())?.ok_or_else(|| {
                crate::Error::Service(format!("Script trigger '{trigger}' is missing a behavior",))
            })?;
        let script_binary = row.script_binary.ok_or_else(|| {
            crate::Error::Service(format!(
                "Script trigger '{trigger}' is missing script content",
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
        trigger_type,
        trigger,
        output: row.output,
        action_type: row.action_type,
        is_enabled: row.is_enabled,
        target_os: row.target_os,
        tags,
        script,
        assets,
        aliases,
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
    fn encode_exchange_blob_is_always_encrypted_tau2() {
        let blob = encode_exchange_blob(&ExchangePayload::new(vec![]), None).unwrap();
        assert_eq!(&blob[..4], b"TAU\x00");
        assert!(
            !blob
                .windows(b"schema_version".len())
                .any(|window| window == b"schema_version"),
            "Encrypted export should be opaque"
        );
    }

    #[test]
    fn encode_exchange_blob_with_password_sets_flag() {
        let blob = encode_exchange_blob(&ExchangePayload::new(vec![]), Some("hunter222")).unwrap();
        assert_eq!(&blob[..4], b"TAU\x00");
        assert_eq!(blob[4] & 0x01, 0x01);
        assert!(
            !blob
                .windows(b"schema_version".len())
                .any(|window| window == b"schema_version"),
            "Encrypted export should be opaque"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_write_export_file_sets_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_export.tau");
        write_export_file(&file_path, b"test_content").unwrap();
        let metadata = std::fs::metadata(&file_path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
}
