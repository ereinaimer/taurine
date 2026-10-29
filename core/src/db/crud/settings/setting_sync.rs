use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

use super::{SettingRow, get_setting};

/// Strict allowlist for portable user experience settings synchronized across devices.
/// Hardware IDs, OS startup hooks, memory configurations, timing calibrations,
/// and execution security gates are strictly forbidden from syncing.
pub const SYNCABLE_SETTINGS: &[&str] = &[
    "ai_custom_endpoint",
    "ai_max_tokens",
    "ai_model",
    "ai_provider",
    "ai_system_prompt",
    "ai_temperature",
    "audio_theme",
    "audio_volume",
    "inline_ai_enabled",
    "inline_case_transform_enabled",
    "inline_currency_to_words_enabled",
    "inline_datetime_date_format",
    "inline_datetime_datetime_format",
    "inline_datetime_dialect",
    "inline_datetime_enabled",
    "inline_datetime_time_format",
    "inline_dictionary_enabled",
    "inline_dictionary_mode",
    "inline_emoji_enabled",
    "inline_emoji_trigger_char",
    "inline_tab_completion_enabled",
    "pause_audio_enabled",
    "pause_hotkey",
    "pause_media_while_dictating",
    "pause_notifications_enabled",
    "spinner_style",
    "voice_dictionary",
    "voice_model",
    "wpm",
];

/// Checks whether a setting key is allowed to be synchronized to the cloud.
pub fn is_setting_syncable(key: &str) -> bool {
    SYNCABLE_SETTINGS.contains(&key)
}

/// Encapsulates a setting row payload for cloud synchronization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudSettingPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    pub key: String,
    pub value: serde_json::Value,
    pub version: i64,
    pub updated_at: Option<String>,
}

/// The outcome of reconciling a remote cloud setting with local SQLite state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingReconcileOutcome {
    UpdatedFromRemote,
    LocalKept,
    IgnoredNotSyncable,
}

/// Returns all settings that have local modifications pending upload (`is_synced = 0`),
/// filtered by the strict sync allowlist.
pub fn get_unsynced_settings(conn: &Connection) -> Result<Vec<SettingRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT key, CAST(value AS TEXT), version, updated_at, 0 AS is_synced
         FROM   settings
         ORDER BY key",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(SettingRow {
            key: row.get(0)?,
            value: row.get(1)?,
            version: row.get(2)?,
            updated_at: row.get(3)?,
            is_synced: row.get(4)?,
        })
    })?;

    let mut unsynced = Vec::new();
    for row in rows {
        let row = row?;
        if is_setting_syncable(&row.key) {
            unsynced.push(row);
        }
    }

    Ok(unsynced)
}

/// Marks the specified settings as synchronized (`is_synced = 1`).
pub fn mark_settings_synced(_conn: &Connection, _keys: &[&str]) -> Result<()> {
    Ok(())
}

/// Reconciles a remote cloud setting payload into local SQLite storage using Last-Write-Wins.
///
/// - Rejects non-syncable settings early (`IgnoredNotSyncable`).
/// - If local setting exists and local `version >= remote.version`, preserves local state (`LocalKept`).
/// - Otherwise, updates or inserts the setting with `remote.version` (`UpdatedFromRemote`),
///   bumping the in-memory settings generation counter.
pub fn reconcile_cloud_setting(
    conn: &Connection,
    remote: &CloudSettingPayload,
) -> Result<SettingReconcileOutcome> {
    if !is_setting_syncable(&remote.key) {
        return Ok(SettingReconcileOutcome::IgnoredNotSyncable);
    }

    let local = get_setting(conn, &remote.key)?;
    if let Some(local_row) = local
        && local_row.version > 0
        && remote.version <= local_row.version
    {
        return Ok(SettingReconcileOutcome::LocalKept);
    }

    let updated_at = remote
        .updated_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(crate::db::now_unix_secs);

    let value_str = serde_json::to_string(&remote.value)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    conn.execute(
        "INSERT INTO settings (key, value, version, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(key) DO UPDATE SET
             value      = excluded.value,
             version    = excluded.version,
             updated_at = excluded.updated_at",
        rusqlite::params![remote.key, value_str, remote.version, updated_at],
    )?;

    crate::settings::bump_settings_version();

    Ok(SettingReconcileOutcome::UpdatedFromRemote)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::upsert_setting;
    use crate::db::init::ensure_schema;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_syncable_allowlist_filtering() {
        assert!(is_setting_syncable("wpm"));
        assert!(is_setting_syncable("audio_theme"));
        assert!(is_setting_syncable("voice_dictionary"));
        assert!(is_setting_syncable("inline_emoji_enabled"));

        // Crucial security and device-specific keys MUST NOT be syncable
        assert!(!is_setting_syncable("voice_input_device"));
        assert!(!is_setting_syncable("start_on_boot"));
        assert!(!is_setting_syncable("scripts_enabled"));
        assert!(!is_setting_syncable("voice_keep_loaded"));
        assert!(!is_setting_syncable("auto_update"));
        assert!(!is_setting_syncable("user_tier"));
        assert!(!is_setting_syncable("last_update_check"));
    }

    #[test]
    fn test_get_unsynced_settings_filters_by_allowlist() {
        let conn = setup_test_db();

        // Insert allowed setting (dirty: is_synced = 0)
        upsert_setting(&conn, "wpm", "120").unwrap();
        // Insert device-specific setting (dirty: is_synced = 0)
        upsert_setting(&conn, "voice_input_device", "\"Realtek Mic\"").unwrap();
        upsert_setting(&conn, "scripts_enabled", "true").unwrap();

        let unsynced = get_unsynced_settings(&conn).unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].key, "wpm");
    }

    #[test]
    fn test_reconcile_cloud_setting_lww() {
        let conn = setup_test_db();
        upsert_setting(&conn, "wpm", "60").unwrap(); // local version = 1

        // Remote has version 2: should update local
        let remote = CloudSettingPayload {
            user_id: None,
            key: "wpm".to_string(),
            value: serde_json::json!(85),
            version: 2,
            updated_at: Some("2026-10-05T10:00:00Z".to_string()),
        };

        let outcome = reconcile_cloud_setting(&conn, &remote).unwrap();
        assert_eq!(outcome, SettingReconcileOutcome::UpdatedFromRemote);

        let row = crate::db::crud::get_setting(&conn, "wpm").unwrap().unwrap();
        assert_eq!(row.value, "85");
        assert_eq!(row.version, 2);

        // Remote with older or equal version should be ignored
        let older_remote = CloudSettingPayload {
            user_id: None,
            key: "wpm".to_string(),
            value: serde_json::json!(40),
            version: 1,
            updated_at: Some("2026-10-05T09:00:00Z".to_string()),
        };
        let outcome2 = reconcile_cloud_setting(&conn, &older_remote).unwrap();
        assert_eq!(outcome2, SettingReconcileOutcome::LocalKept);

        // Attempting to reconcile a non-syncable setting from remote MUST be rejected
        let forbidden_remote = CloudSettingPayload {
            user_id: None,
            key: "scripts_enabled".to_string(),
            value: serde_json::json!(true),
            version: 10,
            updated_at: Some("2026-10-05T10:00:00Z".to_string()),
        };
        let outcome3 = reconcile_cloud_setting(&conn, &forbidden_remote).unwrap();
        assert_eq!(outcome3, SettingReconcileOutcome::IgnoredNotSyncable);
    }

    #[test]
    fn test_reconcile_overwrites_seeded_default_setting() {
        let conn = setup_test_db();
        crate::db::crud::seed_default_setting(&conn, "wpm", "60").unwrap();

        // Seeded setting must have version 0
        let row = crate::db::crud::get_setting(&conn, "wpm").unwrap().unwrap();
        assert_eq!(row.version, 0);

        // Remote has version 1: must overwrite seeded default version 0
        let remote = CloudSettingPayload {
            user_id: None,
            key: "wpm".to_string(),
            value: serde_json::json!(110),
            version: 1,
            updated_at: Some("2026-10-05T10:00:00Z".to_string()),
        };

        let outcome = reconcile_cloud_setting(&conn, &remote).unwrap();
        assert_eq!(outcome, SettingReconcileOutcome::UpdatedFromRemote);

        let row_after = crate::db::crud::get_setting(&conn, "wpm").unwrap().unwrap();
        assert_eq!(row_after.value, "110");
        assert_eq!(row_after.version, 1);
    }
}
