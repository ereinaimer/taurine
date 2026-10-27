use rusqlite::Connection;

use crate::db::crud::{
    ActivityCategory, StreakInfo, TriggerType, calculate_streaks, format_app_display_name,
    get_current_os_db_string,
};
use crate::engine::shell::decompress;

const DEFAULT_MOST_USED_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HomeStats {
    pub words_spoken: u64,
    pub words_expanded: u64,
    pub time_saved_ms: u64,
    pub expansions_run: u64,
    pub streak: StreakInfo,
    pub recent_activity: Vec<RecentActivity>,
    pub most_used_words: Vec<MostUsedTrigger>,
    pub most_used_hotkeys: Vec<MostUsedTrigger>,
    pub top_apps: Vec<crate::db::crud::TopAppStat>,
}

/// One row in the home page's recent activity pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentActivity {
    /// Unix timestamp in milliseconds, matching `activity_events.timestamp`.
    pub timestamp: i64,
    pub category: ActivityCategory,
    pub trigger: String,
    /// Trigger description, falling back to the expanded output when the
    /// trigger has no description. Empty for entries with no trigger.
    pub detail: String,
    pub app: String,
    pub word_count: u64,
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
    let (expansions_run, words_spoken, words_expanded, time_saved_ms) = conn.query_row(
        "SELECT
            COALESCE(SUM(executions + ai_executions + voice_executions), 0),
            COALESCE(SUM(words_spoken), 0),
            COALESCE(SUM(words_expanded), 0),
            COALESCE(SUM(time_saved_ms), 0)
         FROM stats",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        },
    )?;

    let os_str = get_current_os_db_string();
    let streak = calculate_streaks(conn, None)?;
    let recent_activity = load_recent_activity(conn)?;
    let most_used_words = fetch_most_used(conn, os_str, TriggerType::Word, limit)?;
    let most_used_hotkeys = fetch_most_used(conn, os_str, TriggerType::Hotkey, limit)?;
    let top_apps = crate::db::crud::get_top_app_stats_with_conn(
        conn,
        crate::db::crud::AppStatsSortBy::Executions,
        limit,
    )?;

    Ok(HomeStats {
        words_spoken: words_spoken.max(0) as u64,
        words_expanded: words_expanded.max(0) as u64,
        time_saved_ms: time_saved_ms.max(0) as u64,
        expansions_run: expansions_run.max(0) as u64,
        streak,
        recent_activity,
        most_used_words,
        most_used_hotkeys,
        top_apps,
    })
}

fn load_recent_activity(conn: &Connection) -> crate::Result<Vec<RecentActivity>> {
    let mut stmt = conn.prepare_cached(
        "SELECT a.timestamp,
                a.category,
                a.app_key,
                a.word_count,
                (SELECT f.invocation FROM trigger_aliases f
                  WHERE f.trigger_id = a.trigger_id
                  ORDER BY (f.invocation_type != 'word'), f.rowid
                  LIMIT 1),
                t.description,
                t.output,
                s.compressed_content
         FROM activity_events a
         LEFT JOIN triggers t ON t.id = a.trigger_id
         LEFT JOIN scripts s ON s.trigger_id = a.trigger_id
         ORDER BY a.timestamp DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        let category_text: String = row.get(1)?;
        let category =
            ActivityCategory::parse_str(&category_text).unwrap_or(ActivityCategory::Text);
        let app_key: String = row.get(2)?;
        let word_count: i64 = row.get(3)?;
        let alias: Option<String> = row.get(4)?;
        let description: Option<String> = row.get(5)?;
        let output: Option<String> = row.get(6)?;
        let compressed: Option<Vec<u8>> = row.get(7)?;

        Ok(RecentActivity {
            timestamp: row.get(0)?,
            category,
            trigger: alias.unwrap_or_else(|| activity_fallback_label(category).to_string()),
            detail: activity_detail(description, output, compressed),
            app: if app_key.is_empty() {
                String::new()
            } else {
                format_app_display_name(&app_key)
            },
            word_count: word_count.max(0) as u64,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Description first. Scripts store their source compressed and put a
/// `[Script: interpreter]` placeholder in the trigger output, so the
/// source wins over that placeholder; other triggers fall back to output.
fn activity_detail(
    description: Option<String>,
    output: Option<String>,
    compressed: Option<Vec<u8>>,
) -> String {
    if let Some(description) = description {
        let trimmed = description.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(compressed) = compressed {
        return decompress(&compressed).unwrap_or_default();
    }
    let output = output.unwrap_or_default();
    if output.trim_start().starts_with("[Script:") {
        String::new()
    } else {
        output
    }
}

const fn activity_fallback_label(category: ActivityCategory) -> &'static str {
    match category {
        ActivityCategory::Text => "Snippet",
        ActivityCategory::Voice => "Dictation",
        ActivityCategory::Hotkey => "Hotkey",
        ActivityCategory::Script => "Script",
    }
}

fn fetch_most_used(
    conn: &Connection,
    os_str: &str,
    trigger_type: TriggerType,
    limit: usize,
) -> crate::Result<Vec<MostUsedTrigger>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
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
            row.get::<_, i64>(0)?.max(0) as u64,
            row.get::<_, i64>(1)?.max(0) as u64,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;

    let mut result = Vec::new();
    for row in rows {
        let (uses, alias_count, first_word, first_any) = row?;
        // Display rule: the base is the first word invocation, else the
        // first invocation; (+N) counts aliases beyond the display string.
        let base = first_word.or(first_any).unwrap_or_default();
        let extra = alias_count.saturating_sub(1);
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
        ActivityCategory, InvocationType, NewActivityEvent, NewEntry, StatDeltas, add_alias,
        create_entry, delete_alias, increment_stat, insert_activity_event,
    };
    use crate::testing::{init_tracing_for_tests, open_test_db};

    fn text_entry(content: &str, invocation: (InvocationType, &str)) -> NewEntry {
        NewEntry {
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

        assert_eq!(stats.words_spoken, 0);
        assert_eq!(stats.words_expanded, 0);
        assert_eq!(stats.time_saved_ms, 0);
        assert_eq!(stats.expansions_run, 0);
        assert_eq!(stats.streak.current_streak, 0);
        assert!(stats.recent_activity.is_empty());
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
                words_spoken: 50,
                words_expanded: 120,
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
                words_spoken: 10,
                words_expanded: 30,
                time_saved_ms: 60_000,
                ..Default::default()
            },
        )
        .unwrap();

        let (word_id, _) = create_entry(
            &conn,
            text_entry("git status", (InvocationType::Word, "gs")),
        )
        .unwrap();
        set_usage(&conn, &word_id, 12);
        let (hotkey_id, _) = create_entry(
            &conn,
            text_entry(
                "personal email signature",
                (InvocationType::Hotkey, "ralt+m"),
            ),
        )
        .unwrap();
        set_usage(&conn, &hotkey_id, 20);
        let (deleted_id, _) = create_entry(
            &conn,
            text_entry("old output", (InvocationType::Word, "old")),
        )
        .unwrap();
        set_usage(&conn, &deleted_id, 99);
        delete_alias(&conn, &deleted_id, InvocationType::Word, "old").unwrap();

        let stats = load_home_stats(&conn).unwrap();

        assert_eq!(stats.expansions_run, 7);
        assert_eq!(stats.words_spoken, 60);
        assert_eq!(stats.words_expanded, 150);
        assert_eq!(stats.time_saved_ms, 240_000);
        assert_eq!(stats.streak.longest_streak, 2);
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
    fn home_stats_include_recent_activity_with_trigger_display() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let (trigger_id, _) = create_entry(
            &conn,
            text_entry("git status", (InvocationType::Word, "gs")),
        )
        .unwrap();

        insert_activity_event(
            &conn,
            &NewActivityEvent {
                id: None,
                trigger_id: Some(trigger_id),
                timestamp: Some(1_700_000_000_000),
                date: Some("2026-04-01".to_string()),
                category: ActivityCategory::Text,
                app_key: Some("windowsterminal.exe".to_string()),
                word_count: 42,
                time_saved_ms: 5_000,
                content: None,
            },
        )
        .unwrap();
        insert_activity_event(
            &conn,
            &NewActivityEvent {
                id: None,
                trigger_id: None,
                timestamp: Some(1_700_000_060_000),
                date: Some("2026-04-01".to_string()),
                category: ActivityCategory::Voice,
                app_key: Some(String::new()),
                word_count: 9,
                time_saved_ms: 0,
                content: None,
            },
        )
        .unwrap();

        let stats = load_home_stats(&conn).unwrap();

        assert_eq!(stats.recent_activity.len(), 2);
        let latest = &stats.recent_activity[0];
        assert_eq!(latest.category, ActivityCategory::Voice);
        assert_eq!(latest.trigger, "Dictation");
        assert_eq!(latest.detail, "");
        assert_eq!(latest.app, "");
        assert_eq!(latest.word_count, 9);

        let snippet = &stats.recent_activity[1];
        assert_eq!(snippet.category, ActivityCategory::Text);
        assert_eq!(snippet.trigger, "gs");
        assert_eq!(snippet.detail, "git status");
        assert_eq!(snippet.app, "Windowsterminal");
        assert_eq!(snippet.word_count, 42);
    }

    #[test]
    fn recent_activity_prefers_description_over_content() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let mut described = text_entry("git status", (InvocationType::Word, "gs"));
        described.description = Some("Show working tree".to_string());
        let (described_id, _) = create_entry(&conn, described).unwrap();
        let (plain_id, _) = create_entry(
            &conn,
            text_entry("deploy now", (InvocationType::Word, "dpl")),
        )
        .unwrap();

        for (index, trigger_id) in [described_id, plain_id].into_iter().enumerate() {
            insert_activity_event(
                &conn,
                &NewActivityEvent {
                    id: None,
                    trigger_id: Some(trigger_id),
                    timestamp: Some(1_700_000_000_000 + index as i64),
                    date: Some("2026-04-01".to_string()),
                    category: ActivityCategory::Text,
                    app_key: Some(String::new()),
                    word_count: 1,
                    time_saved_ms: 0,
                    content: None,
                },
            )
            .unwrap();
        }

        let stats = load_home_stats(&conn).unwrap();

        assert_eq!(stats.recent_activity[0].detail, "deploy now");
        assert_eq!(stats.recent_activity[1].detail, "Show working tree");
    }

    #[test]
    fn recent_activity_uses_script_source_not_the_placeholder() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let (script_id, _) = create_entry(
            &conn,
            NewEntry {
                description: None,
                content: "Write-Host 'deploy now'".to_string(),
                action_type: "script".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                tags_json: "[]".to_string(),
                auto_case: false,
                interpreter: Some(crate::engine::shell::ScriptInterpreter::PowerShell),
                behavior: Some(crate::engine::shell::ScriptBehavior::Inline),
                invocations: vec![(InvocationType::Word, "deploy".to_string(), false)],
            },
        )
        .unwrap();

        insert_activity_event(
            &conn,
            &NewActivityEvent {
                id: None,
                trigger_id: Some(script_id),
                timestamp: Some(1_700_000_000_000),
                date: Some("2026-04-01".to_string()),
                category: ActivityCategory::Script,
                app_key: Some(String::new()),
                word_count: 0,
                time_saved_ms: 0,
                content: None,
            },
        )
        .unwrap();

        let stats = load_home_stats(&conn).unwrap();

        let event = &stats.recent_activity[0];
        assert_eq!(event.trigger, "deploy");
        assert_eq!(event.detail, "Write-Host 'deploy now'");
        assert!(!event.detail.contains("[Script:"));
    }

    #[test]
    fn script_placeholder_never_becomes_the_detail() {
        assert_eq!(
            activity_detail(None, Some("[Script: powershell]".to_string()), None),
            ""
        );
        assert_eq!(
            activity_detail(None, Some("plain output".to_string()), None),
            "plain output"
        );
    }

    #[test]
    fn recent_activity_keeps_every_event() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        for index in 0..20 {
            insert_activity_event(
                &conn,
                &NewActivityEvent {
                    id: None,
                    trigger_id: None,
                    timestamp: Some(1_700_000_000_000 + index),
                    date: Some("2026-04-01".to_string()),
                    category: ActivityCategory::Voice,
                    app_key: Some(String::new()),
                    word_count: 0,
                    time_saved_ms: 0,
                    content: None,
                },
            )
            .unwrap();
        }

        let stats = load_home_stats(&conn).unwrap();

        assert_eq!(stats.recent_activity.len(), 20);
        assert!(
            stats.recent_activity[0].timestamp > stats.recent_activity[19].timestamp,
            "newest first"
        );
    }

    #[test]
    fn most_used_limit_is_applied() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        for index in 0..6 {
            let trigger = format!("t{index}");
            let (id, _) = create_entry(
                &conn,
                text_entry(&format!("Output {index}"), (InvocationType::Word, &trigger)),
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
            text_entry("git status", (InvocationType::Word, "gs")),
        )
        .unwrap();
        add_alias(&conn, &auto_id, InvocationType::Word, "gstatus", false).unwrap();
        set_usage(&conn, &auto_id, 5);

        let (named_id, _) =
            create_entry(&conn, text_entry("deploy!", (InvocationType::Word, "dpl"))).unwrap();
        add_alias(&conn, &named_id, InvocationType::Word, "dply", false).unwrap();
        set_usage(&conn, &named_id, 3);

        let stats = load_home_stats(&conn).unwrap();

        assert_eq!(stats.most_used_words.len(), 2);
        assert_eq!(stats.most_used_words[0].display, "gs (+1)");
        assert_eq!(stats.most_used_words[0].uses, 5);
        assert_eq!(stats.most_used_words[0].alias_count, 2);
        assert_eq!(stats.most_used_words[1].display, "dpl (+1)");
        assert_eq!(stats.most_used_words[1].uses, 3);
        assert_eq!(stats.most_used_words[1].alias_count, 2);
    }
}
