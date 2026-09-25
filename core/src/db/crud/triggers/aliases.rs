use crate::diagnostic::Diagnostic;
use crate::error::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use super::overlap::{app_filters_overlap, target_os_values_overlap};
use super::trigger_set::{MAX_TRIGGER_LENGTH, prepare_trigger_with_type, with_transaction};
use super::trigger_types::{TriggerAction, TriggerType};
use crate::keys::hotkey_strings_overlap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvocationType {
    #[default]
    Word,
    Hotkey,
    Regex,
    Voice,
}

impl InvocationType {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Word => "word",
            Self::Hotkey => "hotkey",
            Self::Regex => "regex",
            Self::Voice => "voice",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "word" => Some(Self::Word),
            "hotkey" => Some(Self::Hotkey),
            "regex" => Some(Self::Regex),
            "voice" => Some(Self::Voice),
            _ => None,
        }
    }

    pub fn parse_db(value: &str) -> Result<Self> {
        Self::parse_str(value).ok_or_else(|| {
            crate::Error::Config(format!(
                "Invalid invocation_type '{value}'. Expected 'word', 'hotkey', 'regex', or 'voice'."
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerAliasRow {
    pub id: String,
    pub trigger_id: String,
    pub invocation: String,
    pub invocation_type: InvocationType,
    pub require_confirmation: bool,
    pub strict_threshold: Option<f32>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedInvocation {
    pub trigger_id: String,
    pub invocation: String,
    pub invocation_type: InvocationType,
    pub action: TriggerAction,
    pub require_confirmation: bool,
    pub strict_threshold: f32,
}

fn map_alias_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TriggerAliasRow> {
    let invocation_type_str: String = row.get(3)?;
    let invocation_type =
        InvocationType::parse_str(&invocation_type_str).ok_or(rusqlite::Error::InvalidQuery)?;
    let require_confirmation: i64 = row.get(4)?;
    Ok(TriggerAliasRow {
        id: row.get(0)?,
        trigger_id: row.get(1)?,
        invocation: row.get(2)?,
        invocation_type,
        require_confirmation: require_confirmation != 0,
        strict_threshold: row.get(5)?,
        created_at: row.get(6)?,
    })
}

pub fn add_alias(
    conn: &Connection,
    trigger_id: &str,
    invocation_type: InvocationType,
    invocation: &str,
    require_confirmation: bool,
) -> Result<TriggerAliasRow> {
    let (stored, strict_threshold): (String, Option<f32>) = match invocation_type {
        InvocationType::Word => {
            if invocation.trim().is_empty() {
                return Err(Error::Config("Trigger cannot be empty.".to_string()));
            }
            if invocation.len() > MAX_TRIGGER_LENGTH {
                return Err(Error::Config(format!(
                    "Trigger exceeds {} character limit",
                    MAX_TRIGGER_LENGTH
                )));
            }
            if invocation.contains('\n') || invocation.contains('\r') {
                return Err(Error::Config(
                    "Word triggers cannot contain newlines.".to_string(),
                ));
            }
            (invocation.to_string(), None)
        }
        InvocationType::Hotkey => {
            let prepared = prepare_trigger_with_type(invocation, TriggerType::Hotkey, "all")?;
            (prepared.stored_trigger, None)
        }
        InvocationType::Regex => {
            regex::Regex::new(invocation)
                .map_err(|e| Error::Config(format!("Invalid regular expression: {e}")))?;
            (invocation.to_string(), None)
        }
        InvocationType::Voice => {
            let normalized = validate_voice_phrase(invocation)?;
            let threshold = threshold_for_phrase(&normalized);
            (normalized, Some(threshold))
        }
    };

    check_alias_scope_conflict(conn, trigger_id, invocation_type, &stored)?;

    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO trigger_aliases
            (id, trigger_id, invocation, invocation_type, require_confirmation, strict_threshold)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            id,
            trigger_id,
            stored,
            invocation_type.as_db_str(),
            if require_confirmation { 1 } else { 0 },
            strict_threshold,
        ],
    )?;

    Ok(conn.query_row(
        "SELECT id, trigger_id, invocation, invocation_type,
                require_confirmation, strict_threshold, created_at
           FROM trigger_aliases WHERE id = ?1",
        [&id],
        map_alias_row,
    )?)
}

/// Scope-aware duplicate check for one normalized invocation (§0.5/§0.14).
/// Same type+invocation on the SAME parent is always a conflict; on ANOTHER
/// live parent it conflicts only when scopes overlap (`target_os` +
/// app filters). Hotkeys compare with overlap, all other types with equality.
fn check_alias_scope_conflict(
    conn: &Connection,
    trigger_id: &str,
    invocation_type: InvocationType,
    stored: &str,
) -> Result<()> {
    let conflict = || {
        Error::Config(format!(
            "Alias '{stored}' conflicts with an existing trigger."
        ))
    };
    let duplicate_here: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM trigger_aliases
          WHERE trigger_id = ?1 AND invocation_type = ?2 AND invocation = ?3)",
        rusqlite::params![trigger_id, invocation_type.as_db_str(), stored],
        |row| row.get(0),
    )?;
    if duplicate_here {
        return Err(conflict());
    }

    let (parent_os, parent_only, parent_except): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT target_os, only_apps, except_apps FROM triggers WHERE id = ?1",
            [trigger_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

    let mut stmt = conn.prepare_cached(
        "SELECT al.invocation, t.target_os, t.only_apps, t.except_apps
          FROM trigger_aliases al JOIN triggers t ON t.id = al.trigger_id
          WHERE al.invocation_type = ?1 AND al.trigger_id != ?2 AND t.is_deleted = 0",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![invocation_type.as_db_str(), trigger_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )?;
    for row in rows {
        let (existing_invocation, existing_os, existing_only, existing_except) = row?;
        let overlaps = if invocation_type == InvocationType::Hotkey {
            hotkey_strings_overlap(stored, &existing_invocation).map_err(|error| {
                Error::Config(format!(
                    "Invalid stored hotkey '{existing_invocation}' during overlap validation: {error}"
                ))
            })?
        } else {
            stored == existing_invocation
        };
        if overlaps
            && target_os_values_overlap(&parent_os, &existing_os)
            && app_filters_overlap(
                parent_only.as_deref(),
                parent_except.as_deref(),
                existing_only.as_deref(),
                existing_except.as_deref(),
            )
        {
            return Err(conflict());
        }
    }
    Ok(())
}

pub fn list_aliases(conn: &Connection, trigger_id: &str) -> Result<Vec<TriggerAliasRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, trigger_id, invocation, invocation_type,
                require_confirmation, strict_threshold, created_at
           FROM trigger_aliases WHERE trigger_id = ?1 ORDER BY rowid",
    )?;
    let rows = stmt
        .query_map([trigger_id], map_alias_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn count_aliases(conn: &Connection, trigger_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM trigger_aliases WHERE trigger_id = ?1",
        [trigger_id],
        |row| row.get(0),
    )?)
}

fn normalize_lookup(invocation_type: InvocationType, invocation: &str) -> Option<String> {
    match invocation_type {
        InvocationType::Voice => validate_voice_phrase(invocation).ok(),
        InvocationType::Hotkey => prepare_trigger_with_type(invocation, TriggerType::Hotkey, "all")
            .map(|prepared| prepared.stored_trigger)
            .ok(),
        InvocationType::Word | InvocationType::Regex => Some(invocation.to_string()),
    }
}

pub fn find_parent_by_invocation(
    conn: &Connection,
    invocation_type: InvocationType,
    invocation: &str,
) -> Result<Option<String>> {
    let Some(stored) = normalize_lookup(invocation_type, invocation) else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "SELECT trigger_id FROM trigger_aliases
              WHERE invocation_type = ?1 AND invocation = ?2 LIMIT 1",
            rusqlite::params![invocation_type.as_db_str(), stored],
            |row| row.get(0),
        )
        .optional()?)
}

/// Removes one alias row; when it was the parent's last alias, tombstones the parent via `tombstone_entry`.
pub fn delete_alias(
    conn: &Connection,
    trigger_id: &str,
    invocation_type: InvocationType,
    invocation: &str,
) -> Result<bool> {
    let Some(stored) = normalize_lookup(invocation_type, invocation) else {
        return Ok(false);
    };
    let removed = conn.execute(
        "DELETE FROM trigger_aliases
          WHERE trigger_id = ?1 AND invocation_type = ?2 AND invocation = ?3",
        rusqlite::params![trigger_id, invocation_type.as_db_str(), stored],
    )?;
    if removed == 0 {
        return Ok(false);
    }
    if count_aliases(conn, trigger_id)? == 0 {
        tombstone_entry(conn, trigger_id, crate::db::now_unix_secs())?;
    }
    Ok(true)
}

pub fn tombstone_entry(conn: &Connection, trigger_id: &str, now: i64) -> Result<()> {
    with_transaction(conn, || {
        conn.execute(
            "UPDATE triggers SET is_deleted = 1, version = version + 1, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, trigger_id],
        )?;
        conn.execute(
            "DELETE FROM trigger_aliases WHERE trigger_id = ?1",
            [trigger_id],
        )?;
        Ok(())
    })
}

pub fn increment_usage_count_by_id(conn: &Connection, trigger_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE triggers SET usage_count = usage_count + 1, last_used_at = ?1
          WHERE id = ?2 AND is_deleted = 0",
        rusqlite::params![crate::db::now_unix_secs(), trigger_id],
    )?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init::migrate::run_migrations(&conn).unwrap();
        conn
    }

    fn seed_parent(conn: &Connection) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO triggers (id, name, output, action_type, target_os, tags, created_at, updated_at)
             VALUES (?1, '', 'Hello!', 'text', 'all', '[]', unixepoch(), unixepoch())",
            [id.clone()],
        )
        .unwrap();
        id
    }

    #[test]
    fn add_and_list_alias_roundtrip() {
        let conn = fresh_db();
        let pid = seed_parent(&conn);
        let row = add_alias(&conn, &pid, InvocationType::Word, "hi", false).unwrap();
        assert_eq!(row.invocation, "hi");
        assert_eq!(list_aliases(&conn, &pid).unwrap().len(), 1);
        assert_eq!(
            find_parent_by_invocation(&conn, InvocationType::Word, "hi").unwrap(),
            Some(pid)
        );
    }

    #[test]
    fn hotkey_alias_is_canonicalized() {
        let conn = fresh_db();
        let pid = seed_parent(&conn);
        let row = add_alias(
            &conn,
            &pid,
            InvocationType::Hotkey,
            "Shift + Ctrl + G",
            false,
        )
        .unwrap();
        assert_eq!(row.invocation, "ctrl+shift+g");
    }

    #[test]
    fn voice_alias_derives_threshold() {
        let conn = fresh_db();
        let pid = seed_parent(&conn);
        let row = add_alias(&conn, &pid, InvocationType::Voice, "My Email", false).unwrap();
        assert_eq!(row.invocation, "my email");
        assert_eq!(row.strict_threshold, Some(0.85));
    }

    #[test]
    fn duplicate_alias_conflicts() {
        let conn = fresh_db();
        let pid = seed_parent(&conn);
        add_alias(&conn, &pid, InvocationType::Word, "hi", false).unwrap();
        let pid2 = seed_parent(&conn);
        let err = add_alias(&conn, &pid2, InvocationType::Word, "hi", false).unwrap_err();
        assert!(err.to_string().contains("conflicts"));
    }

    #[test]
    fn regex_with_comma_is_one_alias() {
        let conn = fresh_db();
        let pid = seed_parent(&conn);
        let row = add_alias(&conn, &pid, InvocationType::Regex, r"\d{1,3}", false).unwrap();
        assert_eq!(row.invocation, r"\d{1,3}");
    }

    #[test]
    fn lookup_normalizes_voice_and_hotkey_inputs() {
        let conn = fresh_db();
        let pid = seed_parent(&conn);
        add_alias(&conn, &pid, InvocationType::Voice, "My Email", false).unwrap();
        add_alias(
            &conn,
            &pid,
            InvocationType::Hotkey,
            "Shift + Ctrl + G",
            false,
        )
        .unwrap();
        assert_eq!(
            find_parent_by_invocation(&conn, InvocationType::Voice, "  My Email ").unwrap(),
            Some(pid.clone())
        );
        assert_eq!(
            find_parent_by_invocation(&conn, InvocationType::Hotkey, "Shift + Ctrl + G").unwrap(),
            Some(pid.clone())
        );
        assert!(delete_alias(&conn, &pid, InvocationType::Hotkey, "Shift + Ctrl + G").unwrap());
        assert_eq!(count_aliases(&conn, &pid).unwrap(), 1);
    }

    #[test]
    fn lookup_invalid_voice_returns_none() {
        let conn = fresh_db();
        let pid = seed_parent(&conn);
        add_alias(&conn, &pid, InvocationType::Voice, "My Email", false).unwrap();
        assert_eq!(
            find_parent_by_invocation(&conn, InvocationType::Voice, "").unwrap(),
            None
        );
        assert!(!delete_alias(&conn, &pid, InvocationType::Voice, "").unwrap());
    }
}
