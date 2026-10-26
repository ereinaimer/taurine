use rusqlite::{Connection, params};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TriggerDailyStatRow {
    pub trigger_id: String,
    pub date: String,
    pub app_key: String,
    pub executions: u64,
    pub keystrokes_saved: u64,
    pub time_saved_ms: u64,
    pub version: u64,
    pub is_synced: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TopTriggerDailyStat {
    pub trigger_id: String,
    pub executions: u64,
    pub keystrokes_saved: u64,
    pub time_saved_ms: u64,
}

fn map_trigger_daily_stat_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TriggerDailyStatRow> {
    let executions: i64 = row.get(3)?;
    let keystrokes_saved: i64 = row.get(4)?;
    let time_saved_ms: i64 = row.get(5)?;
    let version: i64 = row.get(6)?;
    Ok(TriggerDailyStatRow {
        trigger_id: row.get(0)?,
        date: row.get(1)?,
        app_key: row.get(2)?,
        executions: executions.max(0) as u64,
        keystrokes_saved: keystrokes_saved.max(0) as u64,
        time_saved_ms: time_saved_ms.max(0) as u64,
        version: version.max(0) as u64,
        is_synced: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub fn upsert_trigger_daily_stat_with_conn(
    conn: &Connection,
    trigger_id: &str,
    date: &str,
    app_key: &str,
    executions: u64,
    keystrokes_saved: u64,
    time_saved_ms: u64,
) -> rusqlite::Result<()> {
    let now = crate::db::now_unix_secs();
    conn.execute(
        "INSERT INTO trigger_daily_stats (
            trigger_id, date, app_key, executions, keystrokes_saved, time_saved_ms, version, is_synced, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 0, ?7)
        ON CONFLICT(trigger_id, date, app_key) DO UPDATE SET
            executions       = trigger_daily_stats.executions + excluded.executions,
            keystrokes_saved = trigger_daily_stats.keystrokes_saved + excluded.keystrokes_saved,
            time_saved_ms    = trigger_daily_stats.time_saved_ms + excluded.time_saved_ms,
            version          = trigger_daily_stats.version + 1,
            is_synced        = 0,
            updated_at       = excluded.updated_at",
        params![
            trigger_id,
            date,
            app_key,
            executions as i64,
            keystrokes_saved as i64,
            time_saved_ms as i64,
            now,
        ],
    )?;
    Ok(())
}

pub fn get_trigger_daily_stats_by_trigger(
    conn: &Connection,
    trigger_id: &str,
    limit: usize,
) -> rusqlite::Result<Vec<TriggerDailyStatRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT trigger_id, date, app_key, executions, keystrokes_saved, time_saved_ms, version, is_synced, updated_at
         FROM trigger_daily_stats
         WHERE trigger_id = ?1
         ORDER BY date DESC, executions DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(
        params![trigger_id, limit as i64],
        map_trigger_daily_stat_row,
    )?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn get_trigger_daily_stats_by_date(
    conn: &Connection,
    date: &str,
) -> rusqlite::Result<Vec<TriggerDailyStatRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT trigger_id, date, app_key, executions, keystrokes_saved, time_saved_ms, version, is_synced, updated_at
         FROM trigger_daily_stats
         WHERE date = ?1
         ORDER BY executions DESC",
    )?;
    let rows = stmt.query_map(params![date], map_trigger_daily_stat_row)?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn get_top_triggers_daily_with_conn(
    conn: &Connection,
    date_range: Option<(&str, &str)>,
    limit: usize,
) -> rusqlite::Result<Vec<TopTriggerDailyStat>> {
    let (start_date, end_date) = match date_range {
        Some((start, end)) => (Some(start), Some(end)),
        None => (None, None),
    };

    let mut stmt = conn.prepare_cached(
        "SELECT trigger_id, SUM(executions) as executions, SUM(keystrokes_saved) as keystrokes_saved, SUM(time_saved_ms) as time_saved_ms
         FROM trigger_daily_stats
         WHERE (?1 IS NULL OR date >= ?1) AND (?2 IS NULL OR date <= ?2)
         GROUP BY trigger_id
         ORDER BY executions DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![start_date, end_date, limit as i64], |row| {
        let executions: i64 = row.get(1)?;
        let keystrokes_saved: i64 = row.get(2)?;
        let time_saved_ms: i64 = row.get(3)?;
        Ok(TopTriggerDailyStat {
            trigger_id: row.get(0)?,
            executions: executions.max(0) as u64,
            keystrokes_saved: keystrokes_saved.max(0) as u64,
            time_saved_ms: time_saved_ms.max(0) as u64,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn get_unsynced_trigger_daily_stats(
    conn: &Connection,
    limit: usize,
) -> rusqlite::Result<Vec<TriggerDailyStatRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT trigger_id, date, app_key, executions, keystrokes_saved, time_saved_ms, version, is_synced, updated_at
         FROM trigger_daily_stats
         WHERE is_synced = 0
         ORDER BY updated_at ASC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], map_trigger_daily_stat_row)?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

pub fn mark_trigger_daily_stats_synced(
    conn: &Connection,
    items: &[(&str, &str, &str)],
) -> rusqlite::Result<()> {
    if items.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare_cached(
        "UPDATE trigger_daily_stats SET is_synced = 1 WHERE trigger_id = ?1 AND date = ?2 AND app_key = ?3",
    )?;
    for (trigger_id, date, app_key) in items {
        stmt.execute(params![trigger_id, date, app_key])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init::migrate::run_migrations;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // Insert dummy triggers for foreign key references
        conn.execute(
            "INSERT INTO triggers (id, description, output, created_at, updated_at) VALUES ('trig-1', 'test_trig', 'output', 100, 100)",
            [],
        ).unwrap();
        conn
    }

    #[test]
    fn test_upsert_and_retrieve_trigger_daily_stat() {
        let conn = setup_test_db();

        upsert_trigger_daily_stat_with_conn(&conn, "trig-1", "2026-10-05", "code.exe", 1, 20, 1500)
            .unwrap();

        let rows = get_trigger_daily_stats_by_trigger(&conn, "trig-1", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trigger_id, "trig-1");
        assert_eq!(rows[0].date, "2026-10-05");
        assert_eq!(rows[0].app_key, "code.exe");
        assert_eq!(rows[0].executions, 1);
        assert_eq!(rows[0].keystrokes_saved, 20);
        assert_eq!(rows[0].time_saved_ms, 1500);
        assert_eq!(rows[0].version, 1);
        assert!(!rows[0].is_synced);

        // Second upsert accumulates counters and increments version
        upsert_trigger_daily_stat_with_conn(&conn, "trig-1", "2026-10-05", "code.exe", 2, 40, 3000)
            .unwrap();

        let rows = get_trigger_daily_stats_by_trigger(&conn, "trig-1", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].executions, 3);
        assert_eq!(rows[0].keystrokes_saved, 60);
        assert_eq!(rows[0].time_saved_ms, 4500);
        assert_eq!(rows[0].version, 2);
    }

    #[test]
    fn test_top_triggers_daily_aggregation() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO triggers (id, description, output, created_at, updated_at) VALUES ('trig-2', 'second_trig', 'output2', 100, 100)",
            [],
        ).unwrap();

        upsert_trigger_daily_stat_with_conn(
            &conn,
            "trig-1",
            "2026-10-05",
            "chrome.exe",
            10,
            100,
            5000,
        )
        .unwrap();
        upsert_trigger_daily_stat_with_conn(&conn, "trig-1", "2026-10-05", "code.exe", 5, 50, 2500)
            .unwrap();
        upsert_trigger_daily_stat_with_conn(&conn, "trig-2", "2026-10-05", "code.exe", 2, 20, 1000)
            .unwrap();

        let top = get_top_triggers_daily_with_conn(&conn, None, 10).unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].trigger_id, "trig-1");
        assert_eq!(top[0].executions, 15);
        assert_eq!(top[0].keystrokes_saved, 150);
        assert_eq!(top[0].time_saved_ms, 7500);

        assert_eq!(top[1].trigger_id, "trig-2");
        assert_eq!(top[1].executions, 2);
    }

    #[test]
    fn test_top_triggers_daily_with_date_range() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO triggers (id, description, output, created_at, updated_at) VALUES ('trig-2', 'second_trig', 'output2', 100, 100)",
            [],
        ).unwrap();

        upsert_trigger_daily_stat_with_conn(
            &conn,
            "trig-1",
            "2026-10-01",
            "chrome.exe",
            10,
            100,
            5000,
        )
        .unwrap();
        upsert_trigger_daily_stat_with_conn(&conn, "trig-1", "2026-10-05", "code.exe", 5, 50, 2500)
            .unwrap();
        upsert_trigger_daily_stat_with_conn(&conn, "trig-2", "2026-10-03", "code.exe", 8, 80, 4000)
            .unwrap();

        // Range covering only 2026-10-01 to 2026-10-03
        let top = get_top_triggers_daily_with_conn(&conn, Some(("2026-10-01", "2026-10-03")), 10)
            .unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].trigger_id, "trig-1");
        assert_eq!(top[0].executions, 10);
        assert_eq!(top[1].trigger_id, "trig-2");
        assert_eq!(top[1].executions, 8);

        // Range covering only 2026-10-04 to 2026-10-06
        let top_later =
            get_top_triggers_daily_with_conn(&conn, Some(("2026-10-04", "2026-10-06")), 10)
                .unwrap();
        assert_eq!(top_later.len(), 1);
        assert_eq!(top_later[0].trigger_id, "trig-1");
        assert_eq!(top_later[0].executions, 5);
    }

    #[test]
    fn test_get_trigger_daily_stats_by_date() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO triggers (id, description, output, created_at, updated_at) VALUES ('trig-2', 'second_trig', 'output2', 100, 100)",
            [],
        ).unwrap();

        upsert_trigger_daily_stat_with_conn(
            &conn,
            "trig-1",
            "2026-10-05",
            "code.exe",
            10,
            100,
            5000,
        )
        .unwrap();
        upsert_trigger_daily_stat_with_conn(
            &conn,
            "trig-2",
            "2026-10-05",
            "chrome.exe",
            25,
            250,
            12500,
        )
        .unwrap();
        upsert_trigger_daily_stat_with_conn(
            &conn,
            "trig-1",
            "2026-10-04",
            "code.exe",
            30,
            300,
            15000,
        )
        .unwrap();

        let rows = get_trigger_daily_stats_by_date(&conn, "2026-10-05").unwrap();
        assert_eq!(rows.len(), 2);
        // Ordered by executions DESC
        assert_eq!(rows[0].trigger_id, "trig-2");
        assert_eq!(rows[0].executions, 25);
        assert_eq!(rows[1].trigger_id, "trig-1");
        assert_eq!(rows[1].executions, 10);
    }

    #[test]
    fn test_unsynced_and_mark_synced() {
        let conn = setup_test_db();
        upsert_trigger_daily_stat_with_conn(&conn, "trig-1", "2026-10-05", "code.exe", 1, 10, 500)
            .unwrap();

        let unsynced = get_unsynced_trigger_daily_stats(&conn, 10).unwrap();
        assert_eq!(unsynced.len(), 1);

        mark_trigger_daily_stats_synced(&conn, &[("trig-1", "2026-10-05", "code.exe")]).unwrap();

        let unsynced_after = get_unsynced_trigger_daily_stats(&conn, 10).unwrap();
        assert_eq!(unsynced_after.len(), 0);

        // Calling mark_trigger_daily_stats_synced with empty slice is no-op
        mark_trigger_daily_stats_synced(&conn, &[]).unwrap();
    }
}
