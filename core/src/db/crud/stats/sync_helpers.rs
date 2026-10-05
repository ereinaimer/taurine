use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

use super::{AppStatRow, StatRow, TriggerDailyStatRow};

/// Daily aggregated statistics for a specific trigger, formatted for cloud synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudTriggerDailyStatPayload {
    pub trigger_id: String,
    pub date: String,
    pub app_key: String,
    pub executions: u64,
    pub keystrokes_saved: u64,
    pub time_saved_ms: u64,
    pub version: u64,
}

impl From<TriggerDailyStatRow> for CloudTriggerDailyStatPayload {
    fn from(row: TriggerDailyStatRow) -> Self {
        Self {
            trigger_id: row.trigger_id,
            date: row.date,
            app_key: row.app_key,
            executions: row.executions,
            keystrokes_saved: row.keystrokes_saved,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

impl From<&TriggerDailyStatRow> for CloudTriggerDailyStatPayload {
    fn from(row: &TriggerDailyStatRow) -> Self {
        Self {
            trigger_id: row.trigger_id.clone(),
            date: row.date.clone(),
            app_key: row.app_key.clone(),
            executions: row.executions,
            keystrokes_saved: row.keystrokes_saved,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

/// Global user daily statistics counters formatted for cloud synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudUserStatPayload {
    pub date: String,
    pub executions: i64,
    pub ai_executions: i64,
    pub voice_executions: i64,
    pub words_dictated: i64,
    pub keystrokes_saved: i64,
    pub time_saved_ms: i64,
    pub version: i64,
}

impl From<StatRow> for CloudUserStatPayload {
    fn from(row: StatRow) -> Self {
        Self {
            date: row.date,
            executions: row.executions,
            ai_executions: row.ai_executions,
            voice_executions: row.voice_executions,
            words_dictated: row.words_dictated,
            keystrokes_saved: row.keystrokes_saved,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

impl From<&StatRow> for CloudUserStatPayload {
    fn from(row: &StatRow) -> Self {
        Self {
            date: row.date.clone(),
            executions: row.executions,
            ai_executions: row.ai_executions,
            voice_executions: row.voice_executions,
            words_dictated: row.words_dictated,
            keystrokes_saved: row.keystrokes_saved,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

/// Daily aggregated application statistics counters formatted for cloud synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudUserAppStatPayload {
    pub app_key: String,
    pub date: String,
    pub executions: u64,
    pub keystrokes_saved: u64,
    pub time_saved_ms: u64,
    pub version: u64,
}

impl From<AppStatRow> for CloudUserAppStatPayload {
    fn from(row: AppStatRow) -> Self {
        Self {
            app_key: row.app_key,
            date: row.date,
            executions: row.executions,
            keystrokes_saved: row.keystrokes_saved,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

impl From<&AppStatRow> for CloudUserAppStatPayload {
    fn from(row: &AppStatRow) -> Self {
        Self {
            app_key: row.app_key.clone(),
            date: row.date.clone(),
            executions: row.executions,
            keystrokes_saved: row.keystrokes_saved,
            time_saved_ms: row.time_saved_ms,
            version: row.version,
        }
    }
}

/// Retrieves dirty global stats rows that have not yet been synchronized (`is_synced = 0`),
/// ordered chronologically by `updated_at` ascending.
pub fn get_unsynced_stats(conn: &Connection, limit: usize) -> Result<Vec<StatRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT date, executions, ai_executions, voice_executions, words_dictated, keystrokes_saved, time_saved_ms, version, updated_at
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
            words_dictated: row.get(4)?,
            keystrokes_saved: row.get(5)?,
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
        "SELECT app_key, date, executions, keystrokes_saved, time_saved_ms, version, updated_at
         FROM app_stats
         WHERE is_synced = 0
         ORDER BY updated_at ASC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
        let executions: i64 = row.get(2)?;
        let keystrokes_saved: i64 = row.get(3)?;
        let time_saved_ms: i64 = row.get(4)?;
        let version: i64 = row.get(5)?;
        Ok(AppStatRow {
            app_key: row.get(0)?,
            date: row.get(1)?,
            executions: executions.max(0) as u64,
            keystrokes_saved: keystrokes_saved.max(0) as u64,
            time_saved_ms: time_saved_ms.max(0) as u64,
            version: version.max(0) as u64,
            updated_at: row.get(6)?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::StatDeltas;
    use crate::db::crud::increment_stat;
    use crate::db::init::migrate::run_migrations;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
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
                ..Default::default()
            },
        )
        .unwrap();

        let unsynced = get_unsynced_stats(&conn, 10).unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].date, "2026-10-05");
        assert_eq!(unsynced[0].executions, 5);

        mark_stats_synced(&conn, &["2026-10-05"]).unwrap();

        let remaining = get_unsynced_stats(&conn, 10).unwrap();
        assert_eq!(remaining.len(), 0);
    }

    #[test]
    fn test_unsynced_app_stats_and_mark_synced() {
        let conn = setup_test_db();

        crate::db::crud::upsert_app_stat_with_conn(&conn, "code.exe", "2026-10-05", 10, 100, 5000)
            .unwrap();

        let unsynced = get_unsynced_app_stats(&conn, 10).unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].app_key, "code.exe");
        assert_eq!(unsynced[0].date, "2026-10-05");

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
            keystrokes_saved: 500,
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
        assert_eq!(payload_trigger.keystrokes_saved, 500);
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
            words_dictated: 45,
            keystrokes_saved: 300,
            time_saved_ms: 8000,
            version: 2,
            updated_at: 1728123456,
        };
        let payload_stat: CloudUserStatPayload = (&row_stat).into();
        assert_eq!(payload_stat.date, "2026-10-05");
        assert_eq!(payload_stat.executions, 10);
        assert_eq!(payload_stat.ai_executions, 2);
        assert_eq!(payload_stat.voice_executions, 3);
        assert_eq!(payload_stat.words_dictated, 45);
        assert_eq!(payload_stat.keystrokes_saved, 300);
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
            keystrokes_saved: 120,
            time_saved_ms: 6000,
            version: 1,
            updated_at: 1728123456,
        };
        let payload_app: CloudUserAppStatPayload = (&row_app).into();
        assert_eq!(payload_app.app_key, "code.exe");
        assert_eq!(payload_app.date, "2026-10-05");
        assert_eq!(payload_app.executions, 15);
        assert_eq!(payload_app.keystrokes_saved, 120);
        assert_eq!(payload_app.time_saved_ms, 6000);
        assert_eq!(payload_app.version, 1);

        let json_app = serde_json::to_string(&payload_app).unwrap();
        let deserialized_app: CloudUserAppStatPayload = serde_json::from_str(&json_app).unwrap();
        assert_eq!(deserialized_app, payload_app);
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

        crate::db::crud::upsert_app_stat_with_conn(&conn, "code.exe", "2026-10-05", 5, 50, 2500)
            .unwrap();

        mark_app_stats_synced(&conn, &[("code.exe", "2026-10-05")]).unwrap();
        assert_eq!(get_unsynced_app_stats(&conn, 10).unwrap().len(), 0);

        // Subsequent upsert should mark unsynced again and bump version
        crate::db::crud::upsert_app_stat_with_conn(&conn, "code.exe", "2026-10-05", 10, 100, 5000)
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
}
