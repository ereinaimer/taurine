use rusqlite::{Connection, Result, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityCategory {
    Text,
    Voice,
    Hotkey,
    Script,
}

impl ActivityCategory {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Voice => "voice",
            Self::Hotkey => "hotkey",
            Self::Script => "script",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "text" => Some(Self::Text),
            "voice" => Some(Self::Voice),
            "hotkey" => Some(Self::Hotkey),
            "script" => Some(Self::Script),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEventRow {
    pub id: String,
    pub trigger_id: Option<String>,
    pub timestamp: i64,
    pub date: String,
    pub category: ActivityCategory,
    pub app_key: String,
    pub word_count: u64,
    pub time_saved_ms: u64,
    pub version: u64,
    pub is_synced: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewActivityEvent {
    pub id: Option<String>,
    pub trigger_id: Option<String>,
    pub timestamp: Option<i64>,
    pub date: Option<String>,
    pub category: ActivityCategory,
    pub app_key: Option<String>,
    pub word_count: u64,
    pub time_saved_ms: u64,
}

pub fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn map_activity_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityEventRow> {
    let cat_str: String = row.get(4)?;
    let category = ActivityCategory::parse_str(&cat_str).unwrap_or(ActivityCategory::Text);
    let word_count: i64 = row.get(6)?;
    let time_saved_ms: i64 = row.get(7)?;
    let version: i64 = row.get(8)?;

    Ok(ActivityEventRow {
        id: row.get(0)?,
        trigger_id: row.get(1)?,
        timestamp: row.get(2)?,
        date: row.get(3)?,
        category,
        app_key: row.get(5)?,
        word_count: word_count.max(0) as u64,
        time_saved_ms: time_saved_ms.max(0) as u64,
        version: version.max(0) as u64,
        is_synced: row.get(9)?,
        created_at: row.get(10)?,
    })
}

pub fn insert_activity_event(conn: &Connection, event: &NewActivityEvent) -> Result<String> {
    let id = event
        .id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let timestamp = event.timestamp.unwrap_or_else(now_unix_ms);
    let date = event
        .date
        .clone()
        .unwrap_or_else(crate::stats::get_current_date_string);
    let app_key = event.app_key.as_deref().unwrap_or("");
    let now_secs = crate::db::now_unix_secs();

    conn.execute(
        "INSERT INTO activity_events (
            id, trigger_id, timestamp, date, category, app_key, word_count, time_saved_ms, version, is_synced, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 0, ?9)",
        params![
            id,
            event.trigger_id,
            timestamp,
            date,
            event.category.as_str(),
            app_key,
            event.word_count as i64,
            event.time_saved_ms as i64,
            now_secs,
        ],
    )?;

    Ok(id)
}

pub fn get_activity_events(
    conn: &Connection,
    limit: usize,
    offset: usize,
) -> Result<Vec<ActivityEventRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, trigger_id, timestamp, date, category, app_key, word_count, time_saved_ms, version, is_synced, created_at
         FROM activity_events
         ORDER BY timestamp DESC
         LIMIT ?1 OFFSET ?2",
    )?;

    let rows = stmt.query_map(params![limit as i64, offset as i64], map_activity_event_row)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn get_unsynced_activity_events(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<ActivityEventRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, trigger_id, timestamp, date, category, app_key, word_count, time_saved_ms, version, is_synced, created_at
         FROM activity_events
         WHERE is_synced = 0
         ORDER BY timestamp ASC
         LIMIT ?1",
    )?;

    let rows = stmt.query_map(params![limit as i64], map_activity_event_row)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn mark_activity_events_synced(conn: &Connection, ids: &[&str]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    let mut query = String::from("UPDATE activity_events SET is_synced = 1 WHERE id IN (");
    for (i, _) in ids.iter().enumerate() {
        if i > 0 {
            query.push_str(", ");
        }
        query.push('?');
    }
    query.push(')');

    let mut stmt = conn.prepare(&query)?;
    let params: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    stmt.execute(params.as_slice())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init::schema::ensure_schema;

    #[test]
    fn test_activity_events_lifecycle() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let event = NewActivityEvent {
            id: None,
            trigger_id: Some("trig_1".to_string()),
            timestamp: Some(1700000000000),
            date: Some("2026-10-26".to_string()),
            category: ActivityCategory::Text,
            app_key: Some("code.exe".to_string()),
            word_count: 42,
            time_saved_ms: 5000,
        };

        let id = insert_activity_event(&conn, &event).unwrap();
        assert!(!id.is_empty());

        let events = get_activity_events(&conn, 10, 0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, id);
        assert_eq!(events[0].trigger_id.as_deref(), Some("trig_1"));
        assert_eq!(events[0].category, ActivityCategory::Text);
        assert_eq!(events[0].word_count, 42);
        assert_eq!(events[0].time_saved_ms, 5000);
        assert!(!events[0].is_synced);

        let unsynced = get_unsynced_activity_events(&conn, 10).unwrap();
        assert_eq!(unsynced.len(), 1);

        mark_activity_events_synced(&conn, &[&id]).unwrap();
        let unsynced_after = get_unsynced_activity_events(&conn, 10).unwrap();
        assert!(unsynced_after.is_empty());
    }

    #[test]
    fn test_deleting_trigger_does_not_cascade_delete_activity_events() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        // Insert a trigger
        conn.execute(
            "INSERT INTO triggers (id, description, output, created_at, updated_at)
             VALUES ('trig_persists', 'Desc', 'Output', unixepoch(), unixepoch())",
            [],
        )
        .unwrap();

        // Insert activity event referencing this trigger
        let event = NewActivityEvent {
            id: None,
            trigger_id: Some("trig_persists".to_string()),
            timestamp: Some(1700000000000),
            date: Some("2026-10-26".to_string()),
            category: ActivityCategory::Hotkey,
            app_key: Some("chrome.exe".to_string()),
            word_count: 10,
            time_saved_ms: 1200,
        };
        let event_id = insert_activity_event(&conn, &event).unwrap();

        // Delete the trigger
        conn.execute("DELETE FROM triggers WHERE id = 'trig_persists'", [])
            .unwrap();

        // Verify activity event is STILL intact!
        let events = get_activity_events(&conn, 10, 0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event_id);
        assert_eq!(events[0].trigger_id.as_deref(), Some("trig_persists"));
    }
}
