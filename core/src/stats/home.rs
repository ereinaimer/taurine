use rusqlite::Connection;

use crate::db::crud::{TriggerType, get_current_os_db_string};

const DEFAULT_MOST_USED_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HomeStats {
    pub keystrokes_saved: u64,
    pub time_saved_ms: u64,
    pub expansions_run: u64,
    pub most_used_words: Vec<MostUsedTrigger>,
    pub most_used_hotkeys: Vec<MostUsedTrigger>,
    pub top_apps: Vec<crate::db::crud::TopAppStat>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MostUsedTrigger {
    pub display: String,
    pub trigger_type: TriggerType,
    pub uses: u64,
    pub alias_count: u64,
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
    let top_apps = crate::db::crud::get_top_app_stats_with_conn(
        conn,
        crate::db::crud::AppStatsSortBy::Executions,
        limit,
    )?;

    Ok(HomeStats {
        keystrokes_saved: keystrokes_saved.max(0) as u64,
        time_saved_ms: time_saved_ms.max(0) as u64,
        expansions_run: expansions_run.max(0) as u64,
        most_used_words,
        most_used_hotkeys,
        top_apps,
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
            t.name,
            t.usage_count,
            (SELECT COUNT(*) FROM trigger_aliases al
              WHERE al.trigger_id = t.id),
            (SELECT w.invocation FROM trigger_aliases w
              WHERE w.trigger_id = t.id AND w.invocation_type = 'word'
              ORDER BY w.rowid LIMIT 1),
            (SELECT f.invocation FROM trigger_aliases f
              WHERE f.trigger_id = t.id
              ORDER BY f.rowid LIMIT 1)
         FROM triggers t
         WHERE t.is_deleted = 0
           AND t.is_enabled = 1
           AND t.usage_count > 0
           AND (t.target_os = 'all' OR t.target_os = ?2)
           AND EXISTS (SELECT 1 FROM trigger_aliases al
                       WHERE al.trigger_id = t.id AND al.invocation_type = ?1)
         ORDER BY t.usage_count DESC,
                  (t.target_os != 'all') DESC,
                  t.updated_at DESC
         LIMIT ?3",
    )?;

    let rows = stmt.query_map((trigger_type.as_db_str(), os_str, limit as i64), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?.max(0) as u64,
            row.get::<_, i64>(2)?.max(0) as u64,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;

    let mut result = Vec::new();
    for row in rows {
        let (name, uses, alias_count, first_word, first_any) = row?;
        let named = !name.is_empty();
        // Display rule mirrored from CLI list.rs entry_display: the base is
        // the name when explicit, else the first word invocation, else the
        // first invocation; (+N) counts aliases beyond the display string.
        let base = if named {
            name
        } else {
            first_word.or(first_any).unwrap_or_default()
        };
        let extra = if named {
            alias_count
        } else {
            alias_count.saturating_sub(1)
        };
        let display = if extra == 0 {
            base
        } else {
            format!("{base} (+{extra})")
        };
        result.push(MostUsedTrigger {
            display,
            trigger_type,
            uses,
            alias_count,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::{
        InvocationType, NewEntry, StatDeltas, add_alias, create_entry, delete_alias, increment_stat,
    };
    use crate::testing::{init_tracing_for_tests, open_test_db};

    fn text_entry(name: &str, content: &str, invocation: (InvocationType, &str)) -> NewEntry {
        NewEntry {
            name: name.to_string(),
            description: None,
            content: content.to_string(),
            action_type: "text".to_string(),
            target_os: "all".to_string(),
            only_apps: None,
            except_apps: None,
            tags_json: "[]".to_string(),
            auto_case: false,
            interpreter: None,
            behavior: None,
            invocations: vec![(invocation.0, invocation.1.to_string(), false)],
        }
    }

    fn set_usage(conn: &Connection, id: &str, usage: i64) {
        conn.execute(
            "UPDATE triggers SET usage_count = ?1 WHERE id = ?2",
            rusqlite::params![usage, id],
        )
        .unwrap();
    }

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

        increment_stat(
            &conn,
            "2026-04-01",
            &StatDeltas {
                executions: 4,
                keystrokes_saved: 120,
                time_saved_ms: 180_000,
                ..Default::default()
            },
        )
        .unwrap();
        increment_stat(
            &conn,
            "2026-04-02",
            &StatDeltas {
                executions: 2,
                ai_executions: 1,
                keystrokes_saved: 30,
                time_saved_ms: 60_000,
                ..Default::default()
            },
        )
        .unwrap();

        let (word_id, _) = create_entry(
            &conn,
            text_entry("", "git status", (InvocationType::Word, "gs")),
        )
        .unwrap();
        set_usage(&conn, &word_id, 12);
        let (hotkey_id, _) = create_entry(
            &conn,
            text_entry(
                "",
                "personal email signature",
                (InvocationType::Hotkey, "ralt+m"),
            ),
        )
        .unwrap();
        set_usage(&conn, &hotkey_id, 20);
        let (deleted_id, _) = create_entry(
            &conn,
            text_entry("", "old output", (InvocationType::Word, "old")),
        )
        .unwrap();
        set_usage(&conn, &deleted_id, 99);
        delete_alias(&conn, &deleted_id, InvocationType::Word, "old").unwrap();

        let stats = load_home_stats(&conn).unwrap();

        assert_eq!(stats.expansions_run, 7);
        assert_eq!(stats.keystrokes_saved, 150);
        assert_eq!(stats.time_saved_ms, 240_000);
        assert_eq!(stats.most_used_words.len(), 1);
        assert_eq!(stats.most_used_words[0].display, "gs");
        assert_eq!(stats.most_used_words[0].uses, 12);
        assert_eq!(stats.most_used_words[0].alias_count, 1);
        assert_eq!(stats.most_used_hotkeys.len(), 1);
        assert_eq!(stats.most_used_hotkeys[0].display, "ralt+m");
        assert_eq!(stats.most_used_hotkeys[0].uses, 20);
        assert_eq!(stats.most_used_hotkeys[0].alias_count, 1);
    }

    #[test]
    fn most_used_limit_is_applied() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        for index in 0..6 {
            let trigger = format!("t{index}");
            let (id, _) = create_entry(
                &conn,
                text_entry(
                    "",
                    &format!("Output {index}"),
                    (InvocationType::Word, &trigger),
                ),
            )
            .unwrap();
            set_usage(&conn, &id, (10 - index) as i64);
        }

        let stats = load_home_stats_with_limit(&conn, 5).unwrap();

        assert_eq!(stats.most_used_words.len(), 5);
        assert_eq!(stats.most_used_words[0].uses, 10);
        assert_eq!(stats.most_used_words[4].uses, 6);
    }

    #[test]
    fn home_most_used_renders_grouped_display_with_alias_count() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let (auto_id, _) = create_entry(
            &conn,
            text_entry("", "git status", (InvocationType::Word, "gs")),
        )
        .unwrap();
        add_alias(&conn, &auto_id, InvocationType::Word, "gstatus", false).unwrap();
        set_usage(&conn, &auto_id, 5);

        let (named_id, _) = create_entry(
            &conn,
            text_entry("Deploy", "deploy!", (InvocationType::Word, "dpl")),
        )
        .unwrap();
        add_alias(&conn, &named_id, InvocationType::Word, "dply", false).unwrap();
        set_usage(&conn, &named_id, 3);

        let stats = load_home_stats(&conn).unwrap();

        assert_eq!(stats.most_used_words.len(), 2);
        assert_eq!(stats.most_used_words[0].display, "gs (+1)");
        assert_eq!(stats.most_used_words[0].uses, 5);
        assert_eq!(stats.most_used_words[0].alias_count, 2);
        assert_eq!(stats.most_used_words[1].display, "Deploy (+2)");
        assert_eq!(stats.most_used_words[1].uses, 3);
        assert_eq!(stats.most_used_words[1].alias_count, 2);
    }
}
