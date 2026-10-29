use crate::db::crud::CloudActivityEventPayload;
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
    pub content: Option<String>,
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
    pub content: Option<String>,
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
    let content: Option<String> = row.get(8)?;
    let version: i64 = row.get(9)?;

    Ok(ActivityEventRow {
        id: row.get(0)?,
        trigger_id: row.get(1)?,
        timestamp: row.get(2)?,
        date: row.get(3)?,
        category,
        app_key: row.get(5)?,
        word_count: word_count.max(0) as u64,
        time_saved_ms: time_saved_ms.max(0) as u64,
        content,
        version: version.max(0) as u64,
        is_synced: row.get(10)?,
        created_at: row.get(11)?,
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
            id, trigger_id, timestamp, date, category, app_key, word_count, time_saved_ms, content, version, is_synced, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, 0, ?10)",
        params![
            id,
            event.trigger_id,
            timestamp,
            date,
            event.category.as_str(),
            app_key,
            event.word_count as i64,
            event.time_saved_ms as i64,
            event.content,
            now_secs,
        ],
    )?;

    Ok(id)
}

pub fn reconcile_cloud_activity_events(
    conn: &Connection,
    events: &[CloudActivityEventPayload],
) -> Result<usize> {
    if events.is_empty() {
        return Ok(0);
    }

    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO activity_events (
            id, trigger_id, timestamp, date, category, app_key, word_count, time_saved_ms, content, version, is_synced, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11)",
    )?;

    let mut inserted = 0;
    for evt in events {
        let created_at = evt.timestamp / 1000;
        let count = stmt.execute(params![
            evt.id,
            evt.trigger_id,
            evt.timestamp,
            evt.date,
            evt.category,
            evt.app_key,
            evt.word_count as i64,
            evt.time_saved_ms as i64,
            evt.content,
            evt.version as i64,
            created_at,
        ])?;
        inserted += count;
    }

    Ok(inserted)
}

pub fn get_activity_events(
    conn: &Connection,
    limit: usize,
    offset: usize,
) -> Result<Vec<ActivityEventRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, trigger_id, timestamp, date, category, app_key, word_count, time_saved_ms, content, version, is_synced, created_at
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

pub fn get_recent_activity_events(
    conn: &Connection,
    limit: usize,
    offset: usize,
    category_filter: Option<ActivityCategory>,
) -> Result<Vec<ActivityEventRow>> {
    let mut sql = String::from(
        "SELECT id, trigger_id, timestamp, date, category, app_key, word_count, time_saved_ms, content, version, is_synced, created_at
         FROM activity_events ",
    );
    let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(cat) = category_filter {
        sql.push_str("WHERE category = ? ");
        params_vec.push(rusqlite::types::Value::Text(cat.as_str().to_string()));
    }

    sql.push_str("ORDER BY timestamp DESC, created_at DESC LIMIT ? OFFSET ?");
    params_vec.push((limit as i64).into());
    params_vec.push((offset as i64).into());

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params_vec),
        map_activity_event_row,
    )?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

pub fn get_unsynced_activity_events(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<ActivityEventRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, trigger_id, timestamp, date, category, app_key, word_count, time_saved_ms, content, version, is_synced, created_at
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

pub fn prune_expired_activity_events(conn: &Connection, retention_days: u32) -> Result<usize> {
    let modifier = format!("-{} days", retention_days);
    let deleted = conn.execute(
        "DELETE FROM activity_events WHERE date < date('now', ?1)",
        [&modifier],
    )?;
    Ok(deleted)
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
            content: None,
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
        assert_eq!(events[0].content, None);
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
            content: None,
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

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_insert_and_retrieve_activity_event_with_content() {
        let conn = setup_test_db();
        let event = NewActivityEvent {
            id: None,
            trigger_id: Some("trig-content-1".to_string()),
            timestamp: Some(1_700_000_000),
            date: Some("2026-10-27".to_string()),
            category: ActivityCategory::Voice,
            app_key: Some("slack.exe".to_string()),
            word_count: 5,
            time_saved_ms: 2500,
            content: Some("Testing voice transcription output".to_string()),
        };

        let id = insert_activity_event(&conn, &event).unwrap();
        let events = get_activity_events(&conn, 10, 0).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, id);
        assert_eq!(
            events[0].content.as_deref(),
            Some("Testing voice transcription output")
        );
    }

    #[test]
    fn test_prune_expired_activity_events() {
        let conn = setup_test_db();

        // Event from ancient date (older than 30-day cutoff)
        let old_event = NewActivityEvent {
            id: Some("old-1".to_string()),
            trigger_id: None,
            timestamp: Some(100),
            date: Some("2020-01-01".to_string()),
            category: ActivityCategory::Text,
            app_key: None,
            word_count: 3,
            time_saved_ms: 1000,
            content: Some("ancient snippet".to_string()),
        };
        insert_activity_event(&conn, &old_event).unwrap();

        // Event from today
        let current_date = crate::stats::get_current_date_string();
        let fresh_event = NewActivityEvent {
            id: Some("fresh-1".to_string()),
            trigger_id: None,
            timestamp: Some(2_000_000_000),
            date: Some(current_date),
            category: ActivityCategory::Voice,
            app_key: None,
            word_count: 5,
            time_saved_ms: 2000,
            content: Some("fresh dictation".to_string()),
        };
        insert_activity_event(&conn, &fresh_event).unwrap();

        let pruned = prune_expired_activity_events(&conn, 30).unwrap();
        assert_eq!(pruned, 1);

        let remaining = get_activity_events(&conn, 10, 0).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "fresh-1");
    }

    #[test]
    fn test_get_recent_activity_events_pagination_and_filter() {
        let conn = setup_test_db();
        for i in 0..5 {
            let cat = if i % 2 == 0 {
                ActivityCategory::Voice
            } else {
                ActivityCategory::Text
            };
            insert_activity_event(
                &conn,
                &NewActivityEvent {
                    id: Some(format!("evt-{}", i)),
                    trigger_id: None,
                    timestamp: Some(1_700_000_000 + i),
                    date: Some("2026-10-27".to_string()),
                    category: cat,
                    app_key: None,
                    word_count: 2,
                    time_saved_ms: 100,
                    content: Some(format!("output {}", i)),
                },
            )
            .unwrap();
        }

        let page1 = get_recent_activity_events(&conn, 2, 0, None).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].id, "evt-4");
        assert_eq!(page1[1].id, "evt-3");

        let voice_only =
            get_recent_activity_events(&conn, 10, 0, Some(ActivityCategory::Voice)).unwrap();
        assert_eq!(voice_only.len(), 3);
        assert_eq!(voice_only[0].id, "evt-4");
        assert_eq!(voice_only[1].id, "evt-2");
        assert_eq!(voice_only[2].id, "evt-0");
    }

    #[test]
    fn test_reconcile_cloud_activity_events() {
        let conn = setup_test_db();

        let remote_events = vec![
            CloudActivityEventPayload {
                user_id: None,
                device_id: "dev-laptop".to_string(),
                id: "remote-evt-1".to_string(),
                trigger_id: Some("trig-1".to_string()),
                timestamp: 1_700_000_100,
                date: "2026-10-28".to_string(),
                category: "text".to_string(),
                app_key: "code.exe".to_string(),
                word_count: 10,
                time_saved_ms: 2500,
                content: Some("expanded code snippet".to_string()),
                version: 1,
            },
            CloudActivityEventPayload {
                user_id: None,
                device_id: "dev-desktop".to_string(),
                id: "remote-evt-2".to_string(),
                trigger_id: None,
                timestamp: 1_700_000_200,
                date: "2026-10-28".to_string(),
                category: "voice".to_string(),
                app_key: "slack.exe".to_string(),
                word_count: 25,
                time_saved_ms: 5000,
                content: Some("dictated message from desktop".to_string()),
                version: 1,
            },
        ];

        let inserted = reconcile_cloud_activity_events(&conn, &remote_events).unwrap();
        assert_eq!(inserted, 2);

        let local_events = get_activity_events(&conn, 10, 0).unwrap();
        assert_eq!(local_events.len(), 2);
        assert_eq!(local_events[0].id, "remote-evt-2");
        assert!(local_events[0].is_synced);
        assert_eq!(
            local_events[0].content.as_deref(),
            Some("dictated message from desktop")
        );

        // Reconciling again is idempotent (no duplicate insertions)
        let inserted_again = reconcile_cloud_activity_events(&conn, &remote_events).unwrap();
        assert_eq!(inserted_again, 0);

        // Pulled events must not be queued as unsynced
        let unsynced = super::get_unsynced_activity_events(&conn, 10).unwrap();
        assert!(unsynced.is_empty());
    }
}
