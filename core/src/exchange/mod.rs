pub mod crypto;
mod export;
mod import;

use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
use serde::{Deserialize, Serialize};

pub use export::export_automations;
pub use import::import_automations;

pub const PLAINTEXT_MAGIC_HEADER: [u8; 4] = *b"TAUP";
pub const ENCRYPTED_MAGIC_HEADER: [u8; 4] = *b"TAU1";
pub const EXCHANGE_SCHEMA_VERSION: u32 = 1;
const MAGIC_HEADER_LEN: usize = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExchangePayload {
    pub schema_version: u32,
    #[serde(default)]
    pub automations: Vec<AutomationExport>,
    #[serde(default)]
    pub settings: Option<Vec<SettingExport>>,
    #[serde(default)]
    pub metrics: Option<Vec<MetricExport>>,
}

impl ExchangePayload {
    pub fn new(automations: Vec<AutomationExport>) -> Self {
        Self {
            schema_version: EXCHANGE_SCHEMA_VERSION,
            automations,
            settings: None,
            metrics: None,
        }
    }

    fn validate_schema_version(&self) -> crate::Result<()> {
        if self.schema_version != EXCHANGE_SCHEMA_VERSION {
            return Err(crate::Error::Config(format!(
                "Unsupported exchange schema version: {}",
                self.schema_version
            )));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeFormat {
    Plaintext,
    Encrypted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationExport {
    pub name: String,
    pub description: Option<String>,
    pub trigger: String,
    pub output: String,
    pub action_type: String,
    pub is_enabled: bool,
    pub target_os: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub script: Option<ScriptExport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptExport {
    pub interpreter: ScriptInterpreter,
    pub behavior: ScriptBehavior,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingExport {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricExport {
    pub date: String,
    pub executions: i64,
    pub keystrokes_saved: i64,
}

pub fn encode_plaintext_payload(payload: &ExchangePayload) -> crate::Result<Vec<u8>> {
    let json = serialize_payload(payload)?;
    let mut encoded = Vec::with_capacity(MAGIC_HEADER_LEN + json.len());
    encoded.extend_from_slice(&PLAINTEXT_MAGIC_HEADER);
    encoded.extend_from_slice(&json);
    Ok(encoded)
}

pub fn decode_plaintext_payload(bytes: &[u8]) -> crate::Result<ExchangePayload> {
    match detect_exchange_format(bytes)? {
        ExchangeFormat::Plaintext => deserialize_payload(&bytes[MAGIC_HEADER_LEN..]),
        ExchangeFormat::Encrypted => Err(crate::Error::Config(
            "Expected TAUP plaintext data, but received TAU1 encrypted data".to_string(),
        )),
    }
}

pub fn detect_exchange_format(bytes: &[u8]) -> crate::Result<ExchangeFormat> {
    if bytes.len() < MAGIC_HEADER_LEN {
        return Err(crate::Error::Config(
            "Exchange file is too short to contain a valid header".to_string(),
        ));
    }

    let header = &bytes[..MAGIC_HEADER_LEN];
    if header == PLAINTEXT_MAGIC_HEADER {
        Ok(ExchangeFormat::Plaintext)
    } else if header == ENCRYPTED_MAGIC_HEADER {
        Ok(ExchangeFormat::Encrypted)
    } else {
        Err(crate::Error::Config(
            "Unsupported exchange file header; expected TAUP or TAU1".to_string(),
        ))
    }
}

pub fn serialize_payload(payload: &ExchangePayload) -> crate::Result<Vec<u8>> {
    payload.validate_schema_version()?;
    Ok(serde_json::to_vec(payload)?)
}

pub fn deserialize_payload(bytes: &[u8]) -> crate::Result<ExchangePayload> {
    let payload: ExchangePayload = serde_json::from_slice(bytes)?;
    payload.validate_schema_version()?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::{upsert_automation, upsert_script};
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, compress, decompress};
    use crate::testing::{init_tracing_for_tests, open_test_db};
    use rusqlite::Connection;
    use serde_json::json;

    fn insert_text_automation(conn: &Connection) {
        upsert_automation(
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

    fn insert_script_automation(conn: &Connection) {
        upsert_automation(
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
            "UPDATE automations
             SET is_enabled = 0,
                 is_synced = 0
             WHERE id = ?1",
            ["uuid-script"],
        )
        .unwrap();
    }

    #[test]
    fn export_strips_local_state_and_decompresses_scripts() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        insert_script_automation(&conn);

        let payload = export_automations(&conn).unwrap();
        assert_eq!(payload.schema_version, EXCHANGE_SCHEMA_VERSION);
        assert_eq!(payload.settings, None);
        assert_eq!(payload.metrics, None);
        assert_eq!(payload.automations.len(), 1);

        let automation = &payload.automations[0];
        assert_eq!(automation.name, "Refresh Repo");
        assert_eq!(automation.description.as_deref(), Some("Runs git pull"));
        assert_eq!(automation.trigger, "repo");
        assert_eq!(automation.output, "[Script: bash]");
        assert_eq!(automation.action_type, "script");
        assert!(!automation.is_enabled);
        assert_eq!(automation.target_os, "linux");
        assert_eq!(automation.tags, vec!["git".to_string()]);
        assert_eq!(
            automation.script,
            Some(ScriptExport {
                interpreter: ScriptInterpreter::Bash,
                behavior: ScriptBehavior::Silent,
                content: "git pull --ff-only".to_string(),
            })
        );

        let serialized = serde_json::to_value(&payload).unwrap();
        let automation_json = &serialized["automations"][0];
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
                automation_json.get(stripped_field).is_none(),
                "field {stripped_field} must not be exported"
            );
        }

        assert_eq!(automation_json["is_enabled"], json!(false));
    }

    #[test]
    fn taup_plaintext_codec_round_trips_and_rejects_invalid_headers() {
        let payload = ExchangePayload::new(vec![AutomationExport {
            name: "Greeting".to_string(),
            description: None,
            trigger: "gm".to_string(),
            output: "Good morning!".to_string(),
            action_type: "text".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec!["daily".to_string()],
            script: None,
        }]);

        let encoded = encode_plaintext_payload(&payload).unwrap();
        assert_eq!(
            &encoded[..PLAINTEXT_MAGIC_HEADER.len()],
            &PLAINTEXT_MAGIC_HEADER
        );
        assert_eq!(decode_plaintext_payload(&encoded).unwrap(), payload);

        let err = decode_plaintext_payload(b"TAU1not-json").unwrap_err();
        assert!(err.to_string().contains("TAU1"));
    }

    #[test]
    fn detect_exchange_format_routes_taup_and_tau1_headers() {
        assert_eq!(
            detect_exchange_format(b"TAUP{}").unwrap(),
            ExchangeFormat::Plaintext
        );
        assert_eq!(
            detect_exchange_format(b"TAU1opaque").unwrap(),
            ExchangeFormat::Encrypted
        );

        let err = detect_exchange_format(b"BADS").unwrap_err();
        assert!(err.to_string().contains("TAUP or TAU1"));
    }

    #[test]
    fn export_then_import_round_trips_portable_fields_and_resets_local_state() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        insert_text_automation(&conn);
        insert_script_automation(&conn);

        let payload = export_automations(&conn).unwrap();

        conn.execute("DELETE FROM scripts", []).unwrap();
        conn.execute("DELETE FROM automations", []).unwrap();

        let imported = import_automations(&conn, &payload).unwrap();
        assert_eq!(imported, 2);

        let re_exported = export_automations(&conn).unwrap();
        assert_eq!(re_exported, payload);

        let imported_text = conn
            .query_row(
                "SELECT id, usage_count, last_used_at, version, is_deleted, is_synced, is_enabled
                 FROM automations
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
                 FROM automations a
                 INNER JOIN scripts s ON s.automation_id = a.id
                 WHERE a.trigger = ?1",
                ["repo"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_ne!(script_id, "uuid-script");
        assert!(!script_enabled);
        assert_eq!(decompress(&script_binary).unwrap(), "git pull --ff-only");
    }
}
