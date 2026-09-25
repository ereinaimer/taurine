// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

use crate::diagnostic::Diagnostic;
use crate::error::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceTriggerRow {
    pub id: String,
    pub spoken_phrase: String,
    pub output: String,
    pub action_type: String,
    pub target_os: String,
    pub only_apps: Option<String>,
    pub except_apps: Option<String>,
    pub require_confirmation: bool,
    pub strict_threshold: f32,
    pub usage_count: i64,
    pub is_enabled: bool,
    pub is_deleted: bool,
    pub version: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Derives the acoustic threshold based on phrase character length.
/// Shorter phrases require higher confidence to prevent accidental misfires.
pub fn threshold_for_phrase(normalized: &str) -> f32 {
    match normalized.chars().count() {
        0..=11 => 0.85,
        12..=24 => 0.75,
        _ => 0.65,
    }
}

/// Validates and normalizes a voice trigger phrase.
/// Normalization: trim, lowercase, Unicode NFC.
pub fn validate_voice_phrase(phrase: &str) -> Result<String> {
    let trimmed = phrase.trim();
    if trimmed.is_empty() {
        let diag = Diagnostic::problem("Voice trigger phrase cannot be empty")
            .help("Provide a spoken phrase to trigger the expansion or script")
            .example("taurine add --voice \"my email\" \"user@example.com\"");
        return Err(Error::Config(diag.render()));
    }

    if trimmed.chars().count() > 200 {
        let diag =
            Diagnostic::problem("Voice trigger phrase exceeds maximum length of 200 characters")
                .help("Keep spoken triggers concise for faster and more accurate recognition");
        return Err(Error::Config(diag.render()));
    }

    let normalized: String = trimmed.nfc().collect::<String>().to_lowercase();

    // Check reserved dictation wake phrases
    if normalized == "type this" || normalized.starts_with("type this ") {
        let diag = Diagnostic::problem(
            "Voice trigger phrase cannot start with reserved command 'type this'",
        )
        .help("The phrase 'type this' is reserved for ambient dictation sessions");
        return Err(Error::Config(diag.render()));
    }

    Ok(normalized)
}

/// Normalizes a spoken phrase or transcript for robust voice trigger matching:
/// - Strips leading/trailing punctuation and whitespace
/// - Replaces ASCII punctuation (periods, commas, exclamation marks, question marks, quotes, hyphens) with space
/// - Normalizes Unicode to NFC and converts to lowercase
/// - Collapses multiple spaces into a single space
pub fn normalize_voice_phrase(raw: &str) -> String {
    let without_punct: String = raw
        .chars()
        .map(|c| if c.is_ascii_punctuation() { ' ' } else { c })
        .collect();

    without_punct
        .nfc()
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

/// Adds or updates an active voice trigger.
pub fn add_voice_trigger(
    conn: &Connection,
    phrase: &str,
    output: &str,
    require_confirmation: bool,
) -> Result<VoiceTriggerRow> {
    add_voice_trigger_full(
        conn,
        phrase,
        output,
        "text",
        "all",
        None,
        None,
        require_confirmation,
    )
}

/// Adds or updates a voice trigger with full scoping options.
#[allow(clippy::too_many_arguments)]
pub fn add_voice_trigger_full(
    conn: &Connection,
    phrase: &str,
    output: &str,
    action_type: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    require_confirmation: bool,
) -> Result<VoiceTriggerRow> {
    let normalized_phrase = validate_voice_phrase(phrase)?;
    let threshold = threshold_for_phrase(&normalized_phrase);
    let id = Uuid::new_v4().to_string();

    let mut stmt = conn.prepare_cached(
        "INSERT INTO voice_triggers (
            id, spoken_phrase, output, action_type, target_os, only_apps, except_apps,
            require_confirmation, strict_threshold, usage_count, is_enabled, is_deleted,
            version, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, 0, 1, 0,
            1, unixepoch(), unixepoch()
        )
        ON CONFLICT(spoken_phrase) DO UPDATE SET
            output = excluded.output,
            action_type = excluded.action_type,
            require_confirmation = excluded.require_confirmation,
            strict_threshold = excluded.strict_threshold,
            is_enabled = 1,
            is_deleted = 0,
            version = voice_triggers.version + 1,
            updated_at = unixepoch()
        RETURNING id, spoken_phrase, output, action_type, target_os, only_apps, except_apps,
                  require_confirmation, strict_threshold, usage_count, is_enabled, is_deleted,
                  version, created_at, updated_at;",
    )?;

    let row = stmt.query_row(
        params![
            id,
            normalized_phrase,
            output,
            action_type,
            target_os,
            only_apps,
            except_apps,
            if require_confirmation { 1 } else { 0 },
            threshold,
        ],
        map_row,
    )?;

    Ok(row)
}

/// Looks up an active voice trigger by phrase (case-insensitive, normalized).
pub fn get_voice_trigger_by_phrase(
    conn: &Connection,
    phrase: &str,
) -> Result<Option<VoiceTriggerRow>> {
    let normalized = validate_voice_phrase(phrase)?;
    let mut stmt = conn.prepare_cached(
        "SELECT id, spoken_phrase, output, action_type, target_os, only_apps, except_apps,
                require_confirmation, strict_threshold, usage_count, is_enabled, is_deleted,
                version, created_at, updated_at
           FROM voice_triggers
          WHERE spoken_phrase = ?1 AND is_deleted = 0 AND is_enabled = 1
          LIMIT 1;",
    )?;

    let row = stmt.query_row([normalized], map_row).optional()?;
    Ok(row)
}

/// Lists all currently active (non-deleted, enabled) voice triggers.
pub fn list_active_voice_triggers(conn: &Connection) -> Result<Vec<VoiceTriggerRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, spoken_phrase, output, action_type, target_os, only_apps, except_apps,
                require_confirmation, strict_threshold, usage_count, is_enabled, is_deleted,
                version, created_at, updated_at
           FROM voice_triggers
          WHERE is_deleted = 0 AND is_enabled = 1
          ORDER BY usage_count DESC, spoken_phrase ASC;",
    )?;

    let rows = stmt
        .query_map([], map_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Soft-deletes a voice trigger by phrase.
pub fn delete_voice_trigger_by_phrase(conn: &Connection, phrase: &str) -> Result<bool> {
    let normalized = match validate_voice_phrase(phrase) {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };

    let mut stmt = conn.prepare_cached(
        "UPDATE voice_triggers
            SET is_deleted = 1,
                updated_at = unixepoch(),
                version = version + 1
          WHERE spoken_phrase = ?1 AND is_deleted = 0;",
    )?;

    let count = stmt.execute([normalized])?;
    Ok(count > 0)
}

/// Soft-deletes voice triggers matching a list of phrase values.
/// Returns the number of affected rows.
pub fn delete_voice_triggers_by_values(conn: &Connection, phrases: &[String]) -> Result<usize> {
    if phrases.is_empty() {
        return Ok(0);
    }

    let mut total = 0;
    for phrase in phrases {
        if delete_voice_trigger_by_phrase(conn, phrase)? {
            total += 1;
        }
    }
    Ok(total)
}

/// Increments the usage counter for a voice trigger upon successful activation.
pub fn increment_voice_trigger_usage(conn: &Connection, id: &str) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "UPDATE voice_triggers
            SET usage_count = usage_count + 1,
                updated_at = unixepoch(),
                version = version + 1
          WHERE id = ?1;",
    )?;

    stmt.execute([id])?;
    Ok(())
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VoiceTriggerRow> {
    let req_conf: i64 = row.get(7)?;
    let is_enabled: i64 = row.get(10)?;
    let is_deleted: i64 = row.get(11)?;

    Ok(VoiceTriggerRow {
        id: row.get(0)?,
        spoken_phrase: row.get(1)?,
        output: row.get(2)?,
        action_type: row.get(3)?,
        target_os: row.get(4)?,
        only_apps: row.get(5)?,
        except_apps: row.get(6)?,
        require_confirmation: req_conf != 0,
        strict_threshold: row.get(8)?,
        usage_count: row.get(9)?,
        is_enabled: is_enabled != 0,
        is_deleted: is_deleted != 0,
        version: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

/// Converts a stored voice trigger row into a canonical [`TriggerAction`].
/// Script interpreters are inferred from a shebang when present, falling back
/// to the row's `target_os` default. Unknown action types fall back to text.
pub fn voice_trigger_row_to_action(row: &VoiceTriggerRow) -> crate::db::crud::TriggerAction {
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, compress, infer_interpreter};
    if row.action_type.trim().eq_ignore_ascii_case("script") {
        let inferred = infer_interpreter(None, &row.output).unwrap_or_else(|| {
            let os =
                crate::db::TargetOs::parse_str(&row.target_os).unwrap_or(crate::db::TargetOs::All);
            ScriptInterpreter::default_for_target_os(os)
        });
        let binary = compress(&row.output).ok();
        return crate::db::crud::TriggerAction {
            output: row.output.clone(),
            action_type: "script".into(),
            only_apps: row.only_apps.clone(),
            except_apps: row.except_apps.clone(),
            auto_case: false,
            interpreter: Some(inferred),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: binary,
        };
    }
    let mut a = crate::db::crud::TriggerAction::text(&row.output);
    a.only_apps = row.only_apps.clone();
    a.except_apps = row.except_apps.clone();
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init::migrate::run_migrations;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_validation_and_threshold_rules() {
        assert!(validate_voice_phrase("").is_err());
        assert!(validate_voice_phrase("   ").is_err());
        assert!(validate_voice_phrase(&"a".repeat(201)).is_err());
        assert!(validate_voice_phrase("type this").is_err());
        assert!(validate_voice_phrase("type this message").is_err());

        assert_eq!(
            validate_voice_phrase("  My Email Address  ").unwrap(),
            "my email address"
        );
        assert_eq!(threshold_for_phrase("my email"), 0.85);
        assert_eq!(threshold_for_phrase("open browser now"), 0.75);
        assert_eq!(
            threshold_for_phrase("deploy the production service cluster"),
            0.65
        );
    }

    #[test]
    fn test_voice_trigger_crud_lifecycle() {
        let conn = fresh_db();

        let created = add_voice_trigger(&conn, "My Email", "contact@example.com", false).unwrap();
        assert_eq!(created.spoken_phrase, "my email");
        assert_eq!(created.output, "contact@example.com");
        assert!(!created.require_confirmation);
        assert_eq!(created.strict_threshold, 0.85);

        let retrieved = get_voice_trigger_by_phrase(&conn, "my email").unwrap();
        assert!(retrieved.is_some());
        let r = retrieved.unwrap();
        assert_eq!(r.id, created.id);

        let list = list_active_voice_triggers(&conn).unwrap();
        assert_eq!(list.len(), 1);

        // Delete test
        let deleted = delete_voice_trigger_by_phrase(&conn, "my email").unwrap();
        assert!(deleted);

        let after_delete = get_voice_trigger_by_phrase(&conn, "my email").unwrap();
        assert!(after_delete.is_none());

        let list_after = list_active_voice_triggers(&conn).unwrap();
        assert_eq!(list_after.len(), 0);
    }

    #[test]
    fn test_script_confirmation_flag() {
        let conn = fresh_db();
        let script = add_voice_trigger_full(
            &conn,
            "deploy production",
            "cargo deploy",
            "script",
            "all",
            None,
            None,
            true,
        )
        .unwrap();

        assert!(script.require_confirmation);
        assert_eq!(script.action_type, "script");
    }

    #[test]
    fn test_delete_by_values() {
        let conn = fresh_db();
        add_voice_trigger(&conn, "phrase one", "out1", false).unwrap();
        add_voice_trigger(&conn, "phrase two", "out2", false).unwrap();

        let count = delete_voice_triggers_by_values(
            &conn,
            &["phrase one".to_string(), "phrase two".to_string()],
        )
        .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_normalize_voice_phrase() {
        assert_eq!(normalize_voice_phrase("My email."), "my email");
        assert_eq!(normalize_voice_phrase("My Email!"), "my email");
        assert_eq!(normalize_voice_phrase("  my   email?  "), "my email");
        assert_eq!(normalize_voice_phrase("my-email"), "my email");
        assert_eq!(normalize_voice_phrase("\"My Email,\""), "my email");
        assert_eq!(normalize_voice_phrase("HELLO WORLD"), "hello world");
    }

    fn voice_row(output: &str, action_type: &str, target_os: &str) -> VoiceTriggerRow {
        VoiceTriggerRow {
            id: "id".into(),
            spoken_phrase: "phrase".into(),
            output: output.into(),
            action_type: action_type.into(),
            target_os: target_os.into(),
            only_apps: None,
            except_apps: None,
            require_confirmation: false,
            strict_threshold: 0.85,
            usage_count: 0,
            is_enabled: true,
            is_deleted: false,
            version: 1,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn test_voice_trigger_row_to_action_script_python() {
        let row = voice_row("#!/usr/bin/env python3\nprint('Hi')", "script", "all");
        let a = voice_trigger_row_to_action(&row);
        assert!(a.is_script());
        assert_eq!(
            a.interpreter,
            Some(crate::engine::shell::ScriptInterpreter::Python)
        );
        assert!(a.script_binary.is_some());
    }

    #[test]
    fn test_voice_trigger_row_to_action_text_passthrough() {
        let mut row = voice_row("hello [time]", "text", "all");
        row.only_apps = Some("notepad".into());
        row.except_apps = Some("game".into());
        let a = voice_trigger_row_to_action(&row);
        assert!(a.is_text());
        assert_eq!(a.output, "hello [time]");
        assert_eq!(a.only_apps.as_deref(), Some("notepad"));
        assert_eq!(a.except_apps.as_deref(), Some("game"));
    }

    #[test]
    fn test_voice_trigger_row_to_action_script_fallback_win() {
        let row = voice_row("echo hi", "script", "win");
        let a = voice_trigger_row_to_action(&row);
        assert!(a.is_script());
        assert_eq!(
            a.interpreter,
            Some(crate::engine::shell::ScriptInterpreter::PowerShell)
        );
    }
}
