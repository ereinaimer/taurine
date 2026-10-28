use rusqlite::{Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};

use super::activity_events::ActivityEventRow;
use super::{AppStatRow, StatRow, TriggerDailyStatRow};

/// Daily aggregated statistics for a specific trigger, formatted for cloud synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudTriggerDailyStatPayload {
    #[serde(default)]
    pub device_id: String,
    pub trigger_id: String,
    pub date: String,
    pub app_key: String,
    pub executions: u64,
    pub words_spoken: u64,
    pub words_expanded: u64,
    pub time_saved_ms: u64,
    pub version: u64,
}

impl From<TriggerDailyStatRow> for CloudTriggerDailyStatPayload {
    fn from(row: TriggerDailyStatRow) -> Self {
        Self {
            device_id: crate::cloud::get_device_hardware_id(),
            trigger_id: row.trigger_id,
            date: row.date,
            app_key: row.app_key,
            executions: row.executions,
            words_spoken: row.words_spoken,
            words_expanded: row.words_expanded,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

impl From<&TriggerDailyStatRow> for CloudTriggerDailyStatPayload {
    fn from(row: &TriggerDailyStatRow) -> Self {
        Self {
            device_id: crate::cloud::get_device_hardware_id(),
            trigger_id: row.trigger_id.clone(),
            date: row.date.clone(),
            app_key: row.app_key.clone(),
            executions: row.executions,
            words_spoken: row.words_spoken,
            words_expanded: row.words_expanded,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

/// Global user daily statistics counters formatted for cloud synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudUserStatPayload {
    #[serde(default)]
    pub device_id: String,
    pub date: String,
    pub executions: i64,
    pub ai_executions: i64,
    pub voice_executions: i64,
    pub words_spoken: i64,
    pub words_expanded: i64,
    pub time_saved_ms: i64,
    pub version: i64,
}

impl From<StatRow> for CloudUserStatPayload {
    fn from(row: StatRow) -> Self {
        Self {
            device_id: crate::cloud::get_device_hardware_id(),
            date: row.date,
            executions: row.executions,
            ai_executions: row.ai_executions,
            voice_executions: row.voice_executions,
            words_spoken: row.words_spoken,
            words_expanded: row.words_expanded,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

impl From<&StatRow> for CloudUserStatPayload {
    fn from(row: &StatRow) -> Self {
        Self {
            device_id: crate::cloud::get_device_hardware_id(),
            date: row.date.clone(),
            executions: row.executions,
            ai_executions: row.ai_executions,
            voice_executions: row.voice_executions,
            words_spoken: row.words_spoken,
            words_expanded: row.words_expanded,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

/// Daily aggregated application statistics counters formatted for cloud synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudUserAppStatPayload {
    #[serde(default)]
    pub device_id: String,
    pub app_key: String,
    pub date: String,
    pub executions: u64,
    pub words_spoken: u64,
    pub words_expanded: u64,
    pub time_saved_ms: u64,
    pub version: u64,
}

impl From<AppStatRow> for CloudUserAppStatPayload {
    fn from(row: AppStatRow) -> Self {
        Self {
            device_id: crate::cloud::get_device_hardware_id(),
            app_key: row.app_key,
            date: row.date,
            executions: row.executions,
            words_spoken: row.words_spoken,
            words_expanded: row.words_expanded,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

impl From<&AppStatRow> for CloudUserAppStatPayload {
    fn from(row: &AppStatRow) -> Self {
        Self {
            device_id: crate::cloud::get_device_hardware_id(),
            app_key: row.app_key.clone(),
            date: row.date.clone(),
            executions: row.executions,
            words_spoken: row.words_spoken,
            words_expanded: row.words_expanded,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

/// Event-level user action formatted for cloud synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudActivityEventPayload {
    #[serde(default)]
    pub device_id: String,
    pub id: String,
    pub trigger_id: Option<String>,
    pub timestamp: i64,
    pub date: String,
    pub category: String,
    pub app_key: String,
    pub word_count: u64,
    pub time_saved_ms: u64,
    pub content: Option<String>,
    pub version: u64,
}

impl From<ActivityEventRow> for CloudActivityEventPayload {
    fn from(row: ActivityEventRow) -> Self {
        Self {
            device_id: crate::cloud::get_device_hardware_id(),
            id: row.id,
            trigger_id: row.trigger_id,
            timestamp: row.timestamp,
            date: row.date,
            category: row.category.as_str().to_string(),
            app_key: row.app_key,
            word_count: row.word_count,
            time_saved_ms: row.time_saved_ms,
            content: row.content,
            version: row.version,
        }
    }
}

impl From<&ActivityEventRow> for CloudActivityEventPayload {
    fn from(row: &ActivityEventRow) -> Self {
        Self {
            device_id: crate::cloud::get_device_hardware_id(),
            id: row.id.clone(),
            trigger_id: row.trigger_id.clone(),
            timestamp: row.timestamp,
            date: row.date.clone(),
            category: row.category.as_str().to_string(),
            app_key: row.app_key.clone(),
            word_count: row.word_count,
            time_saved_ms: row.time_saved_ms,
            content: row.content.clone(),
            version: row.version,
        }
    }
}

/// Retrieves dirty global stats rows that have not yet been synchronized (`is_synced = 0`),
/// ordered chronologically by `updated_at` ascending.
pub fn get_unsynced_stats(conn: &Connection, limit: usize) -> Result<Vec<StatRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT date, executions, ai_executions, voice_executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
         FROM stats
         WHERE is_synced = 0
         ORDER BY updated_at ASC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
        Ok(StatRow {
            date: row.get(0)?,
            executions: row.get(1)?,
            ai_executions: row.get(2)?,
            voice_executions: row.get(3)?,
            words_spoken: row.get(4)?,
            words_expanded: row.get(5)?,
            time_saved_ms: row.get(6)?,
            version: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Marks the specified dates in `stats` as synchronized (`is_synced = 1`).
pub fn mark_stats_synced(conn: &Connection, dates: &[&str]) -> Result<()> {
    if dates.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare_cached("UPDATE stats SET is_synced = 1 WHERE date = ?1")?;
    for date in dates {
        stmt.execute(rusqlite::params![date])?;
    }
    Ok(())
}

/// Retrieves dirty app stats rows that have not yet been synchronized (`is_synced = 0`),
/// ordered chronologically by `updated_at` ascending.
pub fn get_unsynced_app_stats(conn: &Connection, limit: usize) -> Result<Vec<AppStatRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT app_key, date, executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
         FROM app_stats
         WHERE is_synced = 0
         ORDER BY updated_at ASC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
        let executions: i64 = row.get(2)?;
        let words_spoken: i64 = row.get(3)?;
        let words_expanded: i64 = row.get(4)?;
        let time_saved_ms: i64 = row.get(5)?;
        let version: i64 = row.get(6)?;
        Ok(AppStatRow {
            app_key: row.get(0)?,
            date: row.get(1)?,
            executions: executions.max(0) as u64,
            words_spoken: words_spoken.max(0) as u64,
            words_expanded: words_expanded.max(0) as u64,
            time_saved_ms: time_saved_ms.max(0) as u64,
            version: version.max(0) as u64,
            updated_at: row.get(7)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Marks the specified `(app_key, date)` pairs in `app_stats` as synchronized (`is_synced = 1`).
pub fn mark_app_stats_synced(conn: &Connection, keys: &[(&str, &str)]) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }

    let mut stmt =
        conn.prepare_cached("UPDATE app_stats SET is_synced = 1 WHERE app_key = ?1 AND date = ?2")?;
    for (app_key, date) in keys {
        stmt.execute(rusqlite::params![app_key, date])?;
    }
    Ok(())
}

#[derive(Default)]
struct UserStatAgg {
    executions: i64,
    ai_executions: i64,
    voice_executions: i64,
    words_spoken: i64,
    words_expanded: i64,
    time_saved_ms: i64,
    version: i64,
}

#[derive(Default)]
struct AppStatAgg {
    executions: u64,
    words_spoken: u64,
    words_expanded: u64,
    time_saved_ms: u64,
    version: u64,
}

#[derive(Default)]
struct TriggerStatAgg {
    executions: u64,
    words_spoken: u64,
    words_expanded: u64,
    time_saved_ms: u64,
    version: u64,
}

/// Reconciles remote cloud user stats into the local SQLite database.
///
/// Multi-device payloads are grouped by date. If the local date row doesn't exist
/// or has `is_synced = 1` (no dirty local modifications pending push), it is inserted or
/// updated with the cloud totals and marked `is_synced = 1`.
pub fn reconcile_cloud_user_stats(
    conn: &Connection,
    stats: &[CloudUserStatPayload],
) -> Result<usize> {
    if stats.is_empty() {
        return Ok(0);
    }

    use std::collections::BTreeMap;
    let mut by_date: BTreeMap<&str, UserStatAgg> = BTreeMap::new();
    for s in stats {
        let entry = by_date.entry(&s.date).or_default();
        entry.executions += s.executions;
        entry.ai_executions += s.ai_executions;
        entry.voice_executions += s.voice_executions;
        entry.words_spoken += s.words_spoken;
        entry.words_expanded += s.words_expanded;
        entry.time_saved_ms += s.time_saved_ms;
        entry.version = entry.version.max(s.version);
    }

    let mut check_stmt = conn.prepare_cached("SELECT is_synced FROM stats WHERE date = ?1")?;
    let mut upsert_stmt = conn.prepare_cached(
        "INSERT INTO stats (
            date, executions, ai_executions, voice_executions, words_spoken, words_expanded, time_saved_ms, version, is_synced, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)
        ON CONFLICT(date) DO UPDATE SET
            executions       = excluded.executions,
            ai_executions    = excluded.ai_executions,
            voice_executions = excluded.voice_executions,
            words_spoken     = excluded.words_spoken,
            words_expanded   = excluded.words_expanded,
            time_saved_ms    = excluded.time_saved_ms,
            version          = excluded.version,
            is_synced        = 1,
            updated_at       = excluded.updated_at
        WHERE stats.is_synced = 1",
    )?;

    let now_secs = crate::db::now_unix_secs();
    let mut count = 0;
    for (date, agg) in by_date {
        let is_dirty: Option<bool> = check_stmt
            .query_row([date], |row| Ok(!row.get::<_, bool>(0)?))
            .optional()?;

        // If local exists and is dirty (is_synced = 0), do not overwrite unpushed local changes
        if is_dirty == Some(true) {
            continue;
        }

        let rows = upsert_stmt.execute(rusqlite::params![
            date,
            agg.executions,
            agg.ai_executions,
            agg.voice_executions,
            agg.words_spoken,
            agg.words_expanded,
            agg.time_saved_ms,
            agg.version,
            now_secs,
        ])?;
        count += rows;
    }

    Ok(count)
}

/// Reconciles remote cloud app stats into the local SQLite database.
pub fn reconcile_cloud_app_stats(
    conn: &Connection,
    stats: &[CloudUserAppStatPayload],
) -> Result<usize> {
    if stats.is_empty() {
        return Ok(0);
    }

    use std::collections::BTreeMap;
    let mut by_key: BTreeMap<(&str, &str), AppStatAgg> = BTreeMap::new();
    for s in stats {
        let entry = by_key.entry((&s.app_key, &s.date)).or_default();
        entry.executions += s.executions;
        entry.words_spoken += s.words_spoken;
        entry.words_expanded += s.words_expanded;
        entry.time_saved_ms += s.time_saved_ms;
        entry.version = entry.version.max(s.version);
    }

    let mut check_stmt =
        conn.prepare_cached("SELECT is_synced FROM app_stats WHERE app_key = ?1 AND date = ?2")?;
    let mut upsert_stmt = conn.prepare_cached(
        "INSERT INTO app_stats (
            app_key, date, executions, words_spoken, words_expanded, time_saved_ms, version, is_synced, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)
        ON CONFLICT(app_key, date) DO UPDATE SET
            executions     = excluded.executions,
            words_spoken   = excluded.words_spoken,
            words_expanded = excluded.words_expanded,
            time_saved_ms  = excluded.time_saved_ms,
            version        = excluded.version,
            is_synced      = 1,
            updated_at     = excluded.updated_at
        WHERE app_stats.is_synced = 1",
    )?;

    let now_secs = crate::db::now_unix_secs();
    let mut count = 0;
    for ((app_key, date), agg) in by_key {
        let is_dirty: Option<bool> = check_stmt
            .query_row(rusqlite::params![app_key, date], |row| {
                Ok(!row.get::<_, bool>(0)?)
            })
            .optional()?;

        if is_dirty == Some(true) {
            continue;
        }

        let rows = upsert_stmt.execute(rusqlite::params![
            app_key,
            date,
            agg.executions as i64,
            agg.words_spoken as i64,
            agg.words_expanded as i64,
            agg.time_saved_ms as i64,
            agg.version as i64,
            now_secs,
        ])?;
        count += rows;
    }

    Ok(count)
}

/// Reconciles remote cloud per-trigger daily stats into the local SQLite database.
pub fn reconcile_cloud_trigger_daily_stats(
    conn: &Connection,
    stats: &[CloudTriggerDailyStatPayload],
) -> Result<usize> {
    if stats.is_empty() {
        return Ok(0);
    }

    use std::collections::BTreeMap;
    let mut by_key: BTreeMap<(&str, &str, &str), TriggerStatAgg> = BTreeMap::new();
    for s in stats {
        let entry = by_key
            .entry((&s.trigger_id, &s.date, &s.app_key))
            .or_default();
        entry.executions += s.executions;
        entry.words_spoken += s.words_spoken;
        entry.words_expanded += s.words_expanded;
        entry.time_saved_ms += s.time_saved_ms;
        entry.version = entry.version.max(s.version);
    }

    let mut check_stmt = conn.prepare_cached(
        "SELECT is_synced FROM trigger_daily_stats WHERE trigger_id = ?1 AND date = ?2 AND app_key = ?3",
    )?;
    let mut upsert_stmt = conn.prepare_cached(
        "INSERT INTO trigger_daily_stats (
            trigger_id, date, app_key, executions, words_spoken, words_expanded, time_saved_ms, version, is_synced, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)
        ON CONFLICT(trigger_id, date, app_key) DO UPDATE SET
            executions     = excluded.executions,
            words_spoken   = excluded.words_spoken,
            words_expanded = excluded.words_expanded,
            time_saved_ms  = excluded.time_saved_ms,
            version        = excluded.version,
            is_synced      = 1,
            updated_at     = excluded.updated_at
        WHERE trigger_daily_stats.is_synced = 1",
    )?;

    let now_secs = crate::db::now_unix_secs();
    let mut count = 0;
    for ((trigger_id, date, app_key), agg) in by_key {
        let is_dirty: Option<bool> = check_stmt
            .query_row(rusqlite::params![trigger_id, date, app_key], |row| {
                Ok(!row.get::<_, bool>(0)?)
            })
            .optional()?;

        if is_dirty == Some(true) {
            continue;
        }

        let rows = upsert_stmt.execute(rusqlite::params![
            trigger_id,
            date,
            app_key,
            agg.executions as i64,
            agg.words_spoken as i64,
            agg.words_expanded as i64,
            agg.time_saved_ms as i64,
            agg.version as i64,
            now_secs,
        ])?;
        count += rows;
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::StatDeltas;
    use crate::db::crud::increment_stat;
    use crate::db::init::ensure_schema;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_unsynced_stats_and_mark_synced() {
        let conn = setup_test_db();

        increment_stat(
            &conn,
            "2026-10-05",
            &StatDeltas {
                executions: 5,
                words_expanded: 10,
                ..Default::default()
            },
        )
        .unwrap();

        let unsynced = get_unsynced_stats(&conn, 10).unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].date, "2026-10-05");
        assert_eq!(unsynced[0].executions, 5);
        assert_eq!(unsynced[0].words_expanded, 10);

        mark_stats_synced(&conn, &["2026-10-05"]).unwrap();

        let remaining = get_unsynced_stats(&conn, 10).unwrap();
        assert_eq!(remaining.len(), 0);
    }

    #[test]
    fn test_unsynced_app_stats_and_mark_synced() {
        let conn = setup_test_db();

        crate::db::crud::upsert_app_stat_with_conn(
            &conn,
            "code.exe",
            "2026-10-05",
            10,
            0,
            100,
            5000,
        )
        .unwrap();

        let unsynced = get_unsynced_app_stats(&conn, 10).unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].app_key, "code.exe");
        assert_eq!(unsynced[0].date, "2026-10-05");
        assert_eq!(unsynced[0].words_expanded, 100);

        mark_app_stats_synced(&conn, &[("code.exe", "2026-10-05")]).unwrap();

        let remaining = get_unsynced_app_stats(&conn, 10).unwrap();
        assert_eq!(remaining.len(), 0);
    }

    #[test]
    fn test_empty_slices_mark_synced_noop() {
        let conn = setup_test_db();
        mark_stats_synced(&conn, &[]).unwrap();
        mark_app_stats_synced(&conn, &[]).unwrap();
    }

    #[test]
    fn test_cloud_payload_conversions_and_serde() {
        // TriggerDailyStatRow conversion
        let row_trigger = TriggerDailyStatRow {
            trigger_id: "trig-123".to_string(),
            date: "2026-10-05".to_string(),
            app_key: "chrome.exe".to_string(),
            executions: 42,
            words_spoken: 5,
            words_expanded: 500,
            time_saved_ms: 15000,
            version: 3,
            is_synced: false,
            updated_at: 1728123456,
        };
        let payload_trigger: CloudTriggerDailyStatPayload = (&row_trigger).into();
        assert_eq!(payload_trigger.trigger_id, "trig-123");
        assert_eq!(payload_trigger.date, "2026-10-05");
        assert_eq!(payload_trigger.app_key, "chrome.exe");
        assert_eq!(payload_trigger.executions, 42);
        assert_eq!(payload_trigger.words_spoken, 5);
        assert_eq!(payload_trigger.words_expanded, 500);
        assert_eq!(payload_trigger.time_saved_ms, 15000);
        assert_eq!(payload_trigger.version, 3);

        let json_trigger = serde_json::to_string(&payload_trigger).unwrap();
        let deserialized_trigger: CloudTriggerDailyStatPayload =
            serde_json::from_str(&json_trigger).unwrap();
        assert_eq!(deserialized_trigger, payload_trigger);

        // StatRow conversion
        let row_stat = StatRow {
            date: "2026-10-05".to_string(),
            executions: 10,
            ai_executions: 2,
            voice_executions: 3,
            words_spoken: 45,
            words_expanded: 300,
            time_saved_ms: 8000,
            version: 2,
            updated_at: 1728123456,
        };
        let payload_stat: CloudUserStatPayload = (&row_stat).into();
        assert_eq!(payload_stat.date, "2026-10-05");
        assert_eq!(payload_stat.executions, 10);
        assert_eq!(payload_stat.ai_executions, 2);
        assert_eq!(payload_stat.voice_executions, 3);
        assert_eq!(payload_stat.words_spoken, 45);
        assert_eq!(payload_stat.words_expanded, 300);
        assert_eq!(payload_stat.time_saved_ms, 8000);
        assert_eq!(payload_stat.version, 2);

        let json_stat = serde_json::to_string(&payload_stat).unwrap();
        let deserialized_stat: CloudUserStatPayload = serde_json::from_str(&json_stat).unwrap();
        assert_eq!(deserialized_stat, payload_stat);

        // AppStatRow conversion
        let row_app = AppStatRow {
            app_key: "code.exe".to_string(),
            date: "2026-10-05".to_string(),
            executions: 15,
            words_spoken: 0,
            words_expanded: 120,
            time_saved_ms: 6000,
            version: 1,
            updated_at: 1728123456,
        };
        let payload_app: CloudUserAppStatPayload = (&row_app).into();
        assert_eq!(payload_app.app_key, "code.exe");
        assert_eq!(payload_app.date, "2026-10-05");
        assert_eq!(payload_app.executions, 15);
        assert_eq!(payload_app.words_spoken, 0);
        assert_eq!(payload_app.words_expanded, 120);
        assert_eq!(payload_app.time_saved_ms, 6000);
        assert_eq!(payload_app.version, 1);

        let json_app = serde_json::to_string(&payload_app).unwrap();
        let deserialized_app: CloudUserAppStatPayload = serde_json::from_str(&json_app).unwrap();
        assert_eq!(deserialized_app, payload_app);

        // ActivityEventRow conversion
        let row_activity = ActivityEventRow {
            id: "evt-test-123".to_string(),
            trigger_id: Some("trig-456".to_string()),
            timestamp: 1728123456000,
            date: "2026-10-05".to_string(),
            category: super::super::activity_events::ActivityCategory::Voice,
            app_key: "slack.exe".to_string(),
            word_count: 12,
            time_saved_ms: 3400,
            content: Some("testing synced speech content".to_string()),
            version: 1,
            is_synced: false,
            created_at: 1728123456,
        };
        let payload_activity: CloudActivityEventPayload = (&row_activity).into();
        assert_eq!(payload_activity.id, "evt-test-123");
        assert_eq!(payload_activity.trigger_id.as_deref(), Some("trig-456"));
        assert_eq!(payload_activity.timestamp, 1728123456000);
        assert_eq!(payload_activity.date, "2026-10-05");
        assert_eq!(payload_activity.category, "voice");
        assert_eq!(payload_activity.app_key, "slack.exe");
        assert_eq!(payload_activity.word_count, 12);
        assert_eq!(payload_activity.time_saved_ms, 3400);
        assert_eq!(
            payload_activity.content.as_deref(),
            Some("testing synced speech content")
        );
        assert_eq!(payload_activity.version, 1);

        let json_activity = serde_json::to_string(&payload_activity).unwrap();
        assert!(json_activity.contains(r#""content":"testing synced speech content""#));
        let deserialized_activity: CloudActivityEventPayload =
            serde_json::from_str(&json_activity).unwrap();
        assert_eq!(deserialized_activity, payload_activity);
    }

    #[test]
    fn test_payload_device_id() {
        let expected_hw_id = crate::cloud::get_device_hardware_id();

        let stat_row = StatRow {
            date: "2026-10-28".to_string(),
            executions: 5,
            ai_executions: 1,
            voice_executions: 2,
            words_spoken: 50,
            words_expanded: 100,
            time_saved_ms: 1500,
            version: 1,
            updated_at: 1728123456,
        };
        let user_stat_payload = CloudUserStatPayload::from(stat_row);
        assert_eq!(user_stat_payload.device_id, expected_hw_id);

        let json = serde_json::to_string(&user_stat_payload).unwrap();
        assert!(json.contains(&format!(r#""device_id":"{}""#, expected_hw_id)));

        // Deserializing without device_id falls back to default empty string
        let no_device_json = r#"{"date":"2026-10-28","executions":5,"ai_executions":1,"voice_executions":2,"words_spoken":50,"words_expanded":100,"time_saved_ms":1500,"version":1}"#;
        let deserialized: CloudUserStatPayload = serde_json::from_str(no_device_json).unwrap();
        assert_eq!(deserialized.device_id, "");
    }

    #[test]
    fn test_subsequent_stat_increment_resets_is_synced_and_bumps_version() {
        let conn = setup_test_db();

        increment_stat(
            &conn,
            "2026-10-05",
            &StatDeltas {
                executions: 1,
                ..Default::default()
            },
        )
        .unwrap();

        mark_stats_synced(&conn, &["2026-10-05"]).unwrap();
        assert_eq!(get_unsynced_stats(&conn, 10).unwrap().len(), 0);

        // Subsequent increment should mark unsynced again
        increment_stat(
            &conn,
            "2026-10-05",
            &StatDeltas {
                executions: 3,
                ..Default::default()
            },
        )
        .unwrap();

        let unsynced = get_unsynced_stats(&conn, 10).unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].executions, 4);
        assert_eq!(unsynced[0].version, 2);
    }

    #[test]
    fn test_subsequent_app_stat_upsert_resets_is_synced_and_bumps_version() {
        let conn = setup_test_db();

        crate::db::crud::upsert_app_stat_with_conn(&conn, "code.exe", "2026-10-05", 5, 0, 50, 2500)
            .unwrap();

        mark_app_stats_synced(&conn, &[("code.exe", "2026-10-05")]).unwrap();
        assert_eq!(get_unsynced_app_stats(&conn, 10).unwrap().len(), 0);

        // Subsequent upsert should mark unsynced again and bump version
        crate::db::crud::upsert_app_stat_with_conn(
            &conn,
            "code.exe",
            "2026-10-05",
            10,
            0,
            100,
            5000,
        )
        .unwrap();

        let unsynced = get_unsynced_app_stats(&conn, 10).unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].executions, 15);
        assert_eq!(unsynced[0].version, 2);
    }

    #[test]
    fn test_unsynced_stats_limit() {
        let conn = setup_test_db();

        increment_stat(
            &conn,
            "2026-10-01",
            &StatDeltas {
                executions: 1,
                ..Default::default()
            },
        )
        .unwrap();
        increment_stat(
            &conn,
            "2026-10-02",
            &StatDeltas {
                executions: 2,
                ..Default::default()
            },
        )
        .unwrap();

        let unsynced = get_unsynced_stats(&conn, 1).unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].date, "2026-10-01");
    }

    #[test]
    fn test_reconcile_cloud_user_stats_multi_device() {
        let conn = setup_test_db();

        // Fresh database: no stats exist
        let remote_stats = vec![
            CloudUserStatPayload {
                device_id: "laptop-1".to_string(),
                date: "2026-10-28".to_string(),
                executions: 10,
                ai_executions: 2,
                voice_executions: 3,
                words_spoken: 150,
                words_expanded: 300,
                time_saved_ms: 6000,
                version: 1,
            },
            CloudUserStatPayload {
                device_id: "desktop-1".to_string(),
                date: "2026-10-28".to_string(),
                executions: 20,
                ai_executions: 5,
                voice_executions: 10,
                words_spoken: 400,
                words_expanded: 800,
                time_saved_ms: 12000,
                version: 2,
            },
        ];

        let reconciled = reconcile_cloud_user_stats(&conn, &remote_stats).unwrap();
        assert_eq!(reconciled, 1);

        let row = crate::db::crud::get_stat(&conn, "2026-10-28")
            .unwrap()
            .unwrap();
        // Aggregated across both devices: 10 + 20 = 30
        assert_eq!(row.executions, 30);
        assert_eq!(row.ai_executions, 7);
        assert_eq!(row.voice_executions, 13);
        assert_eq!(row.words_spoken, 550);
        assert_eq!(row.words_expanded, 1100);
        assert_eq!(row.time_saved_ms, 18000);
        assert_eq!(row.version, 2);

        // Pulled stats must have is_synced = 1 (not queued as unsynced)
        let unsynced = get_unsynced_stats(&conn, 10).unwrap();
        assert!(unsynced.is_empty());
    }
}
