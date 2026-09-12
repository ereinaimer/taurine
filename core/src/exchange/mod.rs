pub mod crypto;
mod export;
mod import;

#[cfg(test)]
mod import_tests;

use crate::db::crud::TriggerType;
use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
use serde::{Deserialize, Serialize};

pub use crypto::{MIN_EXPORT_PASSWORD_LEN, validate_export_password};
pub use export::{
    default_export_filename, default_export_path, encode_exchange_blob, ensure_tau_extension,
    export_triggers, resolve_export_path, write_export_file,
};
pub use import::{
    ExistingTriggerConflict, ImportConflictAction, import_payload_transactionally, import_triggers,
};

pub const TAU_MAGIC: [u8; 4] = *b"TAU\x00";
pub const EXCHANGE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExchangePayload {
    pub schema_version: u32,
    #[serde(default)]
    pub triggers: Vec<TriggerExport>,
}

impl ExchangePayload {
    pub fn new(triggers: Vec<TriggerExport>) -> Self {
        Self {
            schema_version: EXCHANGE_SCHEMA_VERSION,
            triggers,
        }
    }

    fn validate_schema_version(&self) -> crate::Result<()> {
        if self.schema_version > EXCHANGE_SCHEMA_VERSION {
            return Err(crate::Error::Config(format!(
                "schema v{} unsupported (max v{})",
                self.schema_version, EXCHANGE_SCHEMA_VERSION
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerExport {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub trigger_type: TriggerType,
    pub trigger: String,
    pub output: String,
    pub action_type: String,
    pub is_enabled: bool,
    pub target_os: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub script: Option<ScriptExport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<AssetExport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetExport {
    pub id: String,
    pub mime_type: String,
    pub compressed_content_hex: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptExport {
    pub interpreter: ScriptInterpreter,
    pub behavior: ScriptBehavior,
    pub content: String,
}

pub fn decode_exchange_blob(
    bytes: &[u8],
    password: Option<&str>,
) -> crate::Result<ExchangePayload> {
    let mut plaintext = crypto::decrypt(bytes, password)?;
    let payload = deserialize_payload(&plaintext);
    use zeroize::Zeroize;
    plaintext.zeroize();
    payload
}

pub fn payload_contains_run_variables(payload: &ExchangePayload) -> bool {
    payload.triggers.iter().any(|trigger| {
        contains_run_variable(&trigger.output)
            || trigger
                .script
                .as_ref()
                .is_some_and(|script| contains_run_variable(&script.content))
    })
}

pub fn serialize_payload(payload: &ExchangePayload) -> crate::Result<Vec<u8>> {
    payload.validate_schema_version()?;
    let json = serde_json::to_vec(payload)?;
    zstd::bulk::compress(&json, 3).map_err(|e| {
        crate::Error::Service(format!("zstd exchange payload compression failed: {}", e))
    })
}

pub fn deserialize_payload(bytes: &[u8]) -> crate::Result<ExchangePayload> {
    const MAX_PAYLOAD_SIZE: usize = 128 * 1024 * 1024; // 128MB safety limit

    // Attempt zstd decompression. If it fails, fall back to raw JSON deserialization.
    let payload_bytes = match zstd::bulk::decompress(bytes, MAX_PAYLOAD_SIZE) {
        Ok(decompressed) => decompressed,
        Err(_) => bytes.to_vec(),
    };

    let payload: ExchangePayload = serde_json::from_slice(&payload_bytes)?;
    payload.validate_schema_version()?;
    Ok(payload)
}

fn contains_run_variable(content: &str) -> bool {
    content.to_ascii_lowercase().contains("[exec.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::{upsert_script, upsert_trigger, upsert_trigger_with_type};
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, compress, decompress};
    use crate::testing::{init_tracing_for_tests, open_test_db};
    use rusqlite::Connection;
    use serde_json::json;

    fn insert_text_trigger(conn: &Connection) {
        upsert_trigger(
            conn,
            "uuid-text",
            "Greeting",
            Some("Portable greeting"),
            "gm",
            "Good morning!",
            "text",
            "all",
            r#"["daily","team"]"#,
            41,
            Some(1_700_000_123),
        )
        .unwrap();
    }

    fn insert_script_trigger(conn: &Connection) {
        upsert_trigger(
            conn,
            "uuid-script",
            "Refresh Repo",
            Some("Runs git pull"),
            "repo",
            "[Script: bash]",
            "script",
            "linux",
            r#"["git"]"#,
            9,
            Some(1_700_000_456),
        )
        .unwrap();

        let compressed = compress("git pull --ff-only").unwrap();
        upsert_script(
            conn,
            "uuid-script",
            ScriptInterpreter::Bash,
            ScriptBehavior::Silent,
            &compressed,
        )
        .unwrap();

        conn.execute(
            "UPDATE triggers
              SET is_enabled = 0,
                  is_synced = 0
              WHERE id = ?1",
            ["uuid-script"],
        )
        .unwrap();
    }

    fn insert_hotkey_trigger(conn: &Connection) {
        upsert_trigger_with_type(
            conn,
            "uuid-hotkey",
            "Open Git Status",
            Some("Hotkey example"),
            TriggerType::Hotkey,
            "ctrl+shift+g",
            "git status",
            "text",
            "win",
            r#"["git","hotkey"]"#,
            7,
            Some(1_700_000_789),
        )
        .unwrap();
    }

    #[test]
    fn export_strips_local_state_and_decompresses_scripts() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        insert_script_trigger(&conn);

        let payload = export_triggers(&conn).unwrap();
        assert_eq!(payload.schema_version, EXCHANGE_SCHEMA_VERSION);
        assert_eq!(payload.triggers.len(), 1);

        let trigger = &payload.triggers[0];
        assert_eq!(trigger.name, "Refresh Repo");
        assert_eq!(trigger.description.as_deref(), Some("Runs git pull"));
        assert_eq!(trigger.trigger_type, TriggerType::Word);
        assert_eq!(trigger.trigger, "repo");
        assert_eq!(trigger.output, "[Script: bash]");
        assert_eq!(trigger.action_type, "script");
        assert!(!trigger.is_enabled);
        assert_eq!(trigger.target_os, "linux");
        assert_eq!(trigger.tags, vec!["git".to_string()]);
        assert_eq!(
            trigger.script,
            Some(ScriptExport {
                interpreter: ScriptInterpreter::Bash,
                behavior: ScriptBehavior::Silent,
                content: "git pull --ff-only".to_string(),
            })
        );

        let serialized = serde_json::to_value(&payload).unwrap();
        let trigger_json = &serialized["triggers"][0];
        for stripped_field in [
            "id",
            "usage_count",
            "last_used_at",
            "created_at",
            "updated_at",
            "version",
            "is_deleted",
            "is_synced",
        ] {
            assert!(
                trigger_json.get(stripped_field).is_none(),
                "field {stripped_field} must not be exported"
            );
        }

        assert_eq!(trigger_json["is_enabled"], json!(false));
    }

    #[test]
    fn export_includes_trigger_type_for_hotkey_triggers() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        insert_hotkey_trigger(&conn);

        let payload = export_triggers(&conn).unwrap();
        let trigger = payload
            .triggers
            .iter()
            .find(|trigger| trigger.trigger == "ctrl+shift+g")
            .unwrap();

        assert_eq!(trigger.trigger_type, TriggerType::Hotkey);
        assert_eq!(trigger.target_os, "win");
    }

    #[test]
    fn tau_blob_round_trips_without_password() {
        let payload = ExchangePayload::new(vec![TriggerExport {
            name: "Greeting".to_string(),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "gm".to_string(),
            output: "Good morning!".to_string(),
            action_type: "text".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec!["daily".to_string()],
            script: None,
            assets: Vec::new(),
        }]);

        let encoded = crate::exchange::export::encode_exchange_blob(&payload, None).unwrap();
        assert_eq!(&encoded[..TAU_MAGIC.len()], &TAU_MAGIC);
        assert_eq!(decode_exchange_blob(&encoded, None).unwrap(), payload);

        let mut bad = vec![0u8; 128];
        bad[..4].copy_from_slice(b"BADS");
        let err = decode_exchange_blob(&bad, None).unwrap_err();
        assert!(err.to_string().contains("TAU"));
    }

    #[test]
    fn decode_exchange_blob_round_trips_passwordless() {
        let payload = ExchangePayload::new(vec![]);
        let encoded = crate::exchange::export::encode_exchange_blob(&payload, None).unwrap();

        assert_eq!(decode_exchange_blob(&encoded, None).unwrap(), payload);
    }

    #[test]
    fn decode_exchange_blob_requires_password_when_flag_set() {
        let serialized = serialize_payload(&ExchangePayload::new(vec![])).unwrap();
        let blob = crypto::encrypt(&serialized, Some("hunter22")).unwrap();

        let err = decode_exchange_blob(&blob, None).unwrap_err();
        assert!(err.to_string().contains("password required"));
    }

    #[test]
    fn payload_contains_run_variables_detects_output_and_script_content() {
        let mut payload = ExchangePayload::new(vec![TriggerExport {
            name: "Run".to_string(),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "gm".to_string(),
            output: "before [EXEC.bash(echo hi)] after".to_string(),
            action_type: "text".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec![],
            script: None,
            assets: Vec::new(),
        }]);
        assert!(payload_contains_run_variables(&payload));

        payload.triggers[0].output = "safe".to_string();
        payload.triggers[0].script = Some(ScriptExport {
            interpreter: ScriptInterpreter::Bash,
            behavior: ScriptBehavior::Inline,
            content: "echo [exec.bash(date)]".to_string(),
        });
        assert!(payload_contains_run_variables(&payload));
    }

    #[test]
    fn export_then_import_round_trips_portable_fields_and_resets_local_state() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        insert_text_trigger(&conn);
        insert_script_trigger(&conn);
        insert_hotkey_trigger(&conn);

        let payload = export_triggers(&conn).unwrap();

        conn.execute("DELETE FROM scripts", []).unwrap();
        conn.execute("DELETE FROM triggers", []).unwrap();

        let tx = conn.transaction().unwrap();
        let imported =
            import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
        tx.commit().unwrap();
        assert_eq!(imported, 3);

        let re_exported = export_triggers(&conn).unwrap();
        assert_eq!(re_exported, payload);

        let imported_text = conn
            .query_row(
                "SELECT id, usage_count, last_used_at, version, is_deleted, is_synced, is_enabled
                 FROM triggers
                 WHERE trigger = ?1",
                ["gm"],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, bool>(4)?,
                        row.get::<_, bool>(5)?,
                        row.get::<_, bool>(6)?,
                    ))
                },
            )
            .unwrap();
        assert_ne!(imported_text.0, "uuid-text");
        assert_eq!(imported_text.1, 0);
        assert_eq!(imported_text.2, None);
        assert_eq!(imported_text.3, 1);
        assert!(!imported_text.4);
        assert!(imported_text.5);
        assert!(imported_text.6);

        let (script_id, script_enabled, script_binary): (String, bool, Vec<u8>) = conn
            .query_row(
                "SELECT a.id, a.is_enabled, s.compressed_content
                 FROM triggers a
                 INNER JOIN scripts s ON s.trigger_id = a.id
                 WHERE a.trigger = ?1",
                ["repo"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_ne!(script_id, "uuid-script");
        assert!(!script_enabled);
        assert_eq!(decompress(&script_binary).unwrap(), "git pull --ff-only");

        let (hotkey_trigger_type, hotkey_target_os): (String, String) = conn
            .query_row(
                "SELECT trigger_type, target_os
                 FROM triggers
                 WHERE trigger = ?1",
                ["ctrl+shift+g"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(hotkey_trigger_type, "hotkey");
        assert_eq!(hotkey_target_os, "win");
    }

    #[test]
    fn export_triggers_only_contains_triggers_and_no_stats_or_settings() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        insert_text_trigger(&conn);
        conn.execute(
            "INSERT INTO stats (
                date, executions, ai_executions, keystrokes_saved, time_saved_ms, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                "2026-04-01",
                5_i64,
                2_i64,
                50_i64,
                10_000_i64,
                1_700_000_000_i64,
            ),
        )
        .unwrap();

        let payload = export_triggers(&conn).unwrap();

        let trigger = payload
            .triggers
            .iter()
            .find(|trigger| trigger.trigger == "gm")
            .unwrap();
        assert_eq!(trigger.trigger_type, TriggerType::Word);
    }

    #[test]
    fn deserialize_defaults_missing_trigger_type_to_word() {
        let payload = deserialize_payload(
            br#"{
                "schema_version": 1,
                "triggers": [{
                    "name": "Greeting",
                    "description": null,
                    "trigger": "gm",
                    "output": "Good morning!",
                    "action_type": "text",
                    "is_enabled": true,
                    "target_os": "all",
                    "tags": []
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(payload.triggers[0].trigger_type, TriggerType::Word);
    }

    #[test]
    fn deserialize_rejects_invalid_trigger_type() {
        let err = deserialize_payload(
            br#"{
                "schema_version": 1,
                "triggers": [{
                    "name": "Greeting",
                    "description": null,
                    "trigger_type": "gesture",
                    "trigger": "gm",
                    "output": "Good morning!",
                    "action_type": "text",
                    "is_enabled": true,
                    "target_os": "all",
                    "tags": []
                }]
            }"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("gesture"));
    }

    #[test]
    fn export_and_import_with_assets_round_trips() {
        init_tracing_for_tests();
        let (dir, mut conn) = open_test_db();

        const TINY_PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let img_path = dir.path().join("logo.png");
        std::fs::write(&img_path, TINY_PNG).unwrap();

        let script_path = dir.path().join("script.sh");
        std::fs::write(&script_path, "echo Hello from Script!").unwrap();

        let trigger = "asset_test";
        let output = format!(
            "Img: [image({})] Script: [execute.bash.file({})]",
            img_path.to_string_lossy(),
            script_path.to_string_lossy()
        );

        upsert_trigger_with_type(
            &conn,
            "uuid-asset-test",
            "Asset Test",
            Some("Testing img and script file compilation"),
            TriggerType::Word,
            trigger,
            &output,
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        let assets_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(assets_count, 2);

        let rewritten_output: String = conn
            .query_row(
                "SELECT output FROM triggers WHERE id = 'uuid-asset-test'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(rewritten_output.contains("[image(asset("));
        assert!(rewritten_output.contains("file(asset("));

        let payload = export_triggers(&conn).unwrap();
        assert_eq!(payload.triggers.len(), 1);
        assert_eq!(payload.triggers[0].assets.len(), 2);

        conn.execute("DELETE FROM assets", []).unwrap();
        conn.execute("DELETE FROM triggers", []).unwrap();

        let tx = conn.transaction().unwrap();
        let imported =
            import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
        tx.commit().unwrap();
        assert_eq!(imported, 1);

        let restored_assets_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(restored_assets_count, 2);

        let restored_output: String = conn
            .query_row("SELECT output FROM triggers", [], |row| row.get(0))
            .unwrap();
        assert!(restored_output.contains("[image(asset("));
        assert!(restored_output.contains("file(asset("));
    }
}
