use rusqlite::{Connection, types::Type};

use crate::db::crud::{TriggerType, get_current_os_db_string};

const DEFAULT_MOST_USED_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HomeStats {
    pub keystrokes_saved: u64,
    pub time_saved_ms: u64,
    pub expansions_run: u64,
    pub most_used_words: Vec<MostUsedTrigger>,
    pub most_used_hotkeys: Vec<MostUsedTrigger>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MostUsedTrigger {
    pub trigger: String,
    pub trigger_type: TriggerType,
    pub uses: u64,
}

pub fn load_home_stats(conn: &Connection) -> crate::Result<HomeStats> {
    load_home_stats_with_limit(conn, DEFAULT_MOST_USED_LIMIT)
}

pub fn load_home_stats_with_limit(conn: &Connection, limit: usize) -> crate::Result<HomeStats> {
    let (expansions_run, keystrokes_saved, time_saved_ms) = conn.query_row(
        "SELECT
            COALESCE(SUM(executions + ai_executions), 0),
            COALESCE(SUM(keystrokes_saved), 0),
            COALESCE(SUM(time_saved_ms), 0)
         FROM stats",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;

    let os_str = get_current_os_db_string();
    let most_used_words = fetch_most_used(conn, os_str, TriggerType::Word, limit)?;
    let most_used_hotkeys = fetch_most_used(conn, os_str, TriggerType::Hotkey, limit)?;

    Ok(HomeStats {
        keystrokes_saved: keystrokes_saved.max(0) as u64,
        time_saved_ms: time_saved_ms.max(0) as u64,
        expansions_run: expansions_run.max(0) as u64,
        most_used_words,
        most_used_hotkeys,
    })
}

fn fetch_most_used(
    conn: &Connection,
    os_str: &str,
    trigger_type: TriggerType,
    limit: usize,
) -> crate::Result<Vec<MostUsedTrigger>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
            a.trigger,
            a.trigger_type,
            a.usage_count
         FROM triggers a
         WHERE a.is_deleted = 0
           AND a.is_enabled = 1
           AND a.usage_count > 0
           AND a.trigger_type = ?1
           AND (a.target_os = 'all' OR a.target_os = ?2)
         ORDER BY a.usage_count DESC,
                  (a.target_os != 'all') DESC,
                  a.updated_at DESC,
                  LOWER(a.trigger) ASC,
                  a.trigger ASC
         LIMIT ?3",
    )?;

    let rows = stmt.query_map((trigger_type.as_db_str(), os_str, limit as i64), |row| {
        let trigger_type = TriggerType::parse_db(&row.get::<_, String>(1)?).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(1, Type::Text, Box::new(err))
        })?;

        Ok(MostUsedTrigger {
            trigger: row.get(0)?,
            trigger_type,
            uses: row.get::<_, i64>(2)?.max(0) as u64,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::{
        TriggerType, delete_trigger, increment_stat, upsert_trigger, upsert_trigger_with_type,
    };
    use crate::testing::{init_tracing_for_tests, open_test_db};

    #[test]
    fn empty_home_stats_returns_zero_totals_and_no_triggers() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let stats = load_home_stats(&conn).unwrap();

        assert_eq!(stats.keystrokes_saved, 0);
        assert_eq!(stats.time_saved_ms, 0);
        assert_eq!(stats.expansions_run, 0);
        assert!(stats.most_used_words.is_empty());
        assert!(stats.most_used_hotkeys.is_empty());
    }

    #[test]
    fn home_stats_aggregate_totals_and_sort_triggers() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_stat(&conn, "2026-04-01", 4, 0, 120, 180_000).unwrap();
        increment_stat(&conn, "2026-04-02", 2, 1, 30, 60_000).unwrap();

        upsert_trigger(
            &conn,
            "uuid-word",
            "Git Status",
            Some("git status"),
            "gs",
            "git status",
            "text",
            "all",
            "[]",
            12,
            None,
        )
        .unwrap();
        upsert_trigger_with_type(
            &conn,
            "uuid-hotkey",
            "Email Signature",
            Some("personal email signature"),
            TriggerType::Hotkey,
            "ralt+m",
            "[Script: powershell]",
            "script",
            "all",
            "[]",
            20,
            None,
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-deleted",
            "Old Trigger",
            None,
            "old",
            "old output",
            "text",
            "all",
            "[]",
            99,
            None,
        )
        .unwrap();
        delete_trigger(&conn, "uuid-deleted").unwrap();

        let stats = load_home_stats(&conn).unwrap();

        assert_eq!(stats.expansions_run, 7);
        assert_eq!(stats.keystrokes_saved, 150);
        assert_eq!(stats.time_saved_ms, 240_000);
        assert_eq!(stats.most_used_words.len(), 1);
        assert_eq!(stats.most_used_words[0].trigger, "gs");
        assert_eq!(stats.most_used_words[0].uses, 12);
        assert_eq!(stats.most_used_hotkeys.len(), 1);
        assert_eq!(stats.most_used_hotkeys[0].trigger, "ralt+m");
        assert_eq!(stats.most_used_hotkeys[0].uses, 20);
    }

    #[test]
    fn most_used_limit_is_applied() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        for index in 0..6 {
            let id = format!("uuid-{index}");
            let trigger = format!("t{index}");
            upsert_trigger(
                &conn,
                &id,
                &format!("Trigger {index}"),
                None,
                &trigger,
                &format!("Output {index}"),
                "text",
                "all",
                "[]",
                (10 - index) as i64,
                None,
            )
            .unwrap();
        }

        let stats = load_home_stats_with_limit(&conn, 5).unwrap();

        assert_eq!(stats.most_used_words.len(), 5);
        assert_eq!(stats.most_used_words[0].uses, 10);
        assert_eq!(stats.most_used_words[4].uses, 6);
    }
}
