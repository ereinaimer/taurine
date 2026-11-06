use crate::error::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub id: i64,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
    #[serde(default)]
    pub retry_count: i64,
    #[serde(default)]
    pub last_error: Option<String>,
}

pub fn enqueue_outbox(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    operation: &str,
    payload: serde_json::Value,
) -> Result<i64> {
    let payload_str = payload.to_string();

    // Check if an unattempted entry already exists for this entity to coalesce burst edits
    let existing_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM sync_outbox WHERE entity_type = ?1 AND entity_id = ?2 AND retry_count = 0 LIMIT 1",
            params![entity_type, entity_id],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing_id {
        let mut update_stmt = conn.prepare_cached(
            "UPDATE sync_outbox
             SET operation = ?1, payload = ?2, created_at = unixepoch()
             WHERE id = ?3",
        )?;
        update_stmt.execute(params![operation, payload_str, id])?;
        return Ok(id);
    }

    let mut stmt = conn.prepare_cached(
        "INSERT INTO sync_outbox (entity_type, entity_id, operation, payload)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    stmt.execute(params![entity_type, entity_id, operation, payload_str])?;
    Ok(conn.last_insert_rowid())
}

pub fn fetch_outbox_batch(conn: &Connection, limit: usize) -> Result<Vec<OutboxEntry>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, entity_type, entity_id, operation, payload, created_at, retry_count, last_error
         FROM sync_outbox ORDER BY retry_count ASC, id ASC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |row| {
        let payload_str: String = row.get(4)?;
        let payload = serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
        Ok(OutboxEntry {
            id: row.get(0)?,
            entity_type: row.get(1)?,
            entity_id: row.get(2)?,
            operation: row.get(3)?,
            payload,
            created_at: row.get(5)?,
            retry_count: row.get(6)?,
            last_error: row.get(7)?,
        })
    })?;
    let mut entries = Vec::new();
    for r in rows {
        entries.push(r?);
    }
    Ok(entries)
}

pub fn delete_outbox_batch(conn: &Connection, max_id: i64) -> Result<usize> {
    let mut stmt = conn.prepare_cached("DELETE FROM sync_outbox WHERE id <= ?1")?;
    Ok(stmt.execute([max_id])?)
}

pub fn delete_outbox_entry(conn: &Connection, id: i64) -> Result<bool> {
    let mut stmt = conn.prepare_cached("DELETE FROM sync_outbox WHERE id = ?1")?;
    Ok(stmt.execute([id])? > 0)
}

pub fn get_sync_checkpoint(conn: &Connection, entity_type: &str) -> Result<i64> {
    let mut stmt =
        conn.prepare_cached("SELECT last_synced_at FROM sync_checkpoints WHERE entity_type = ?1")?;
    let result = stmt.query_row([entity_type], |r| r.get(0));
    match result {
        Ok(ts) => Ok(ts),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

pub fn set_sync_checkpoint(conn: &Connection, entity_type: &str, ts: i64) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO sync_checkpoints (entity_type, last_synced_at)
         VALUES (?1, ?2)
         ON CONFLICT(entity_type) DO UPDATE SET last_synced_at = excluded.last_synced_at",
    )?;
    stmt.execute(params![entity_type, ts])?;
    Ok(())
}

/// Clears all stored synchronization watermarks from SQLite.
pub fn reset_sync_checkpoints(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM sync_checkpoints", [])?;
    Ok(())
}

pub fn record_outbox_failure(conn: &Connection, id: i64, error_msg: &str) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "UPDATE sync_outbox
         SET retry_count = retry_count + 1,
             last_error = ?2
         WHERE id = ?1",
    )?;
    stmt.execute(params![id, error_msg])?;
    Ok(())
}

pub fn quarantine_outbox_entry(conn: &Connection, id: i64, reason: &str) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT OR REPLACE INTO sync_outbox_quarantine (id, entity_type, entity_id, operation, payload, retry_count, last_error)
         SELECT id, entity_type, entity_id, operation, payload, retry_count, ?2
         FROM sync_outbox
         WHERE id = ?1",
    )?;
    stmt.execute(params![id, reason])?;
    delete_outbox_entry(conn, id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_outbox_enqueue_fetch_and_ack() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init::ensure_schema(&conn).unwrap();

        let id1 = enqueue_outbox(
            &conn,
            "snippet",
            "trig-1",
            "upsert",
            serde_json::json!({"output": "hello"}),
        )
        .unwrap();
        let id2 = enqueue_outbox(
            &conn,
            "setting",
            "wpm",
            "upsert",
            serde_json::json!({"value": 120}),
        )
        .unwrap();

        let batch = fetch_outbox_batch(&conn, 10).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].id, id1);
        assert_eq!(batch[0].entity_type, "snippet");
        assert_eq!(batch[1].id, id2);
        assert_eq!(batch[1].entity_type, "setting");

        delete_outbox_batch(&conn, id1).unwrap();
        let remaining = fetch_outbox_batch(&conn, 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, id2);
    }

    #[test]
    fn test_enqueue_outbox_coalesces_unattempted_entry() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init::ensure_schema(&conn).unwrap();

        let id1 = enqueue_outbox(
            &conn,
            "snippet",
            "trig-coalesce",
            "upsert",
            serde_json::json!({"output": "v1"}),
        )
        .unwrap();

        let id2 = enqueue_outbox(
            &conn,
            "snippet",
            "trig-coalesce",
            "upsert",
            serde_json::json!({"output": "v2"}),
        )
        .unwrap();

        assert_eq!(id1, id2, "Coalesced outbox row must reuse same id");

        let batch = fetch_outbox_batch(&conn, 10).unwrap();
        assert_eq!(
            batch.len(),
            1,
            "Must have exactly 1 coalesced row in outbox"
        );
        assert_eq!(
            batch[0].payload.get("output").and_then(|v| v.as_str()),
            Some("v2")
        );
    }

    #[test]
    fn test_outbox_fifo_ordering() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init::ensure_schema(&conn).unwrap();

        for i in 1..=5 {
            enqueue_outbox(
                &conn,
                "item",
                &format!("id-{}", i),
                "insert",
                serde_json::json!({"val": i}),
            )
            .unwrap();
        }

        let batch1 = fetch_outbox_batch(&conn, 2).unwrap();
        assert_eq!(batch1.len(), 2);
        assert_eq!(batch1[0].entity_id, "id-1");
        assert_eq!(batch1[1].entity_id, "id-2");

        delete_outbox_batch(&conn, batch1[1].id).unwrap();

        let batch2 = fetch_outbox_batch(&conn, 10).unwrap();
        assert_eq!(batch2.len(), 3);
        assert_eq!(batch2[0].entity_id, "id-3");
        assert_eq!(batch2[1].entity_id, "id-4");
        assert_eq!(batch2[2].entity_id, "id-5");
    }

    #[test]
    fn test_outbox_delete_single_entry() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init::ensure_schema(&conn).unwrap();

        let id1 = enqueue_outbox(
            &conn,
            "snippet",
            "trig-1",
            "upsert",
            serde_json::json!({"output": "hello"}),
        )
        .unwrap();
        let id2 = enqueue_outbox(
            &conn,
            "snippet",
            "trig-2",
            "upsert",
            serde_json::json!({"output": "world"}),
        )
        .unwrap();

        assert!(delete_outbox_entry(&conn, id1).unwrap());
        assert!(!delete_outbox_entry(&conn, id1).unwrap());

        let remaining = fetch_outbox_batch(&conn, 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, id2);
    }

    #[test]
    fn test_sync_checkpoint_lifecycle() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init::ensure_schema(&conn).unwrap();

        let initial = get_sync_checkpoint(&conn, "snippets").unwrap();
        assert_eq!(initial, 0);

        set_sync_checkpoint(&conn, "snippets", 1700000000).unwrap();
        let updated = get_sync_checkpoint(&conn, "snippets").unwrap();
        assert_eq!(updated, 1700000000);

        set_sync_checkpoint(&conn, "snippets", 1700005000).unwrap();
        let updated2 = get_sync_checkpoint(&conn, "snippets").unwrap();
        assert_eq!(updated2, 1700005000);

        let other = get_sync_checkpoint(&conn, "settings").unwrap();
        assert_eq!(other, 0);
    }
}
