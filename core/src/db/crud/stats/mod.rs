pub mod activity_events;
pub mod app_stats;
mod recorder;
mod stat_delete;
mod stat_get;
mod stat_set;
mod stat_types;
pub mod streak;
pub mod sync_helpers;
pub mod trigger_daily_stats;

use crate::stats::TriggerStatKind;

pub use activity_events::{
    ActivityCategory, ActivityEventRow, NewActivityEvent, get_activity_events,
    get_recent_activity_events, get_unsynced_activity_events, insert_activity_event,
    mark_activity_events_synced, prune_expired_activity_events, reconcile_cloud_activity_events,
};
pub use app_stats::{
    AppStatRow, AppStatsSortBy, TopAppStat, format_app_display_name, get_top_app_stats_with_conn,
    upsert_app_stat_with_conn,
};
pub use recorder::{
    TriggerStatEvent, record_trigger_stat, record_trigger_stat_with_conn,
    record_voice_dictation_usage, record_voice_trigger_usage, resolve_trigger_id,
};
pub use stat_delete::delete_stat;
pub use stat_get::{get_stat, get_stat_counters};
pub use stat_set::increment_stat;
pub use stat_types::{StatDeltas, StatRow};
pub use streak::{StreakInfo, calculate_streaks};
pub use sync_helpers::{
    CloudActivityEventPayload, CloudTriggerDailyStatPayload, CloudUserAppStatPayload,
    CloudUserStatPayload, get_unsynced_app_stats, get_unsynced_stats, mark_app_stats_synced,
    mark_stats_synced, reconcile_cloud_app_stats, reconcile_cloud_trigger_daily_stats,
    reconcile_cloud_user_stats,
};
pub use trigger_daily_stats::{
    TopTriggerDailyStat, TriggerDailyStatRow, get_top_triggers_daily_with_conn,
    get_trigger_daily_stats_by_date, get_trigger_daily_stats_by_trigger,
    get_unsynced_trigger_daily_stats, mark_trigger_daily_stats_synced,
    upsert_trigger_daily_stat_with_conn,
};

/// High-level function to record stats for a mathematical calculation.
///
/// Unlike `record_expansion_usage`, this does not touch the `triggers` table.
pub fn record_calculation_usage(output_len: usize, delete_count: usize, left_arrow_count: usize) {
    let trigger_chars = delete_count
        .saturating_sub(left_arrow_count)
        .saturating_sub(2);
    record_trigger_stat(TriggerStatEvent {
        trigger_id: None,
        trigger: None,
        trigger_chars,
        success: output_len > 0,
        output_chars: output_len,
        kind: TriggerStatKind::Calculation,
        wpm: None,
        app: None,
        words_count: None,
        content: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::TriggerStatKind;
    use crate::testing::{init_tracing_for_tests, open_test_db};

    #[test]
    fn get_stat_returns_none_for_missing_date() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let result = get_stat(&conn, "2099-01-01").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_stat_counters_returns_none_for_missing_date() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let result = get_stat_counters(&conn, "2099-01-01").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn increment_stat_inserts_new_row_with_version_1() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_stat(
            &conn,
            "2026-03-30",
            &StatDeltas {
                executions: 42,
                ai_executions: 3,
                voice_executions: 5,
                words_spoken: 50,
                words_expanded: 500,
                time_saved_ms: 60000,
            },
        )
        .unwrap();

        let row = get_stat(&conn, "2026-03-30").unwrap().unwrap();
        assert_eq!(row.date, "2026-03-30");
        assert_eq!(row.executions, 42);
        assert_eq!(row.ai_executions, 3);
        assert_eq!(row.voice_executions, 5);
        assert_eq!(row.words_spoken, 50);
        assert_eq!(row.words_expanded, 500);
        assert_eq!(row.time_saved_ms, 60000);
        assert_eq!(row.version, 1);
        assert!(row.updated_at > 0);
    }

    #[test]
    fn increment_stat_updates_counters_and_increments_version() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_stat(
            &conn,
            "2026-03-30",
            &StatDeltas {
                executions: 1,
                ai_executions: 1,
                words_expanded: 10,
                time_saved_ms: 1000,
                ..Default::default()
            },
        )
        .unwrap();
        increment_stat(
            &conn,
            "2026-03-30",
            &StatDeltas {
                executions: 2,
                ai_executions: 3,
                voice_executions: 2,
                words_spoken: 20,
                words_expanded: 20,
                time_saved_ms: 2000,
            },
        )
        .unwrap();

        let row = get_stat(&conn, "2026-03-30").unwrap().unwrap();
        assert_eq!(row.version, 2);
        assert_eq!(row.executions, 3);
        assert_eq!(row.ai_executions, 4);
        assert_eq!(row.voice_executions, 2);
        assert_eq!(row.words_spoken, 20);
        assert_eq!(row.words_expanded, 30);
        assert_eq!(row.time_saved_ms, 3000);
    }

    #[test]
    fn get_stat_counters_returns_updated_values() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_stat(
            &conn,
            "2026-03-30",
            &StatDeltas {
                executions: 5,
                ai_executions: 1,
                words_expanded: 123,
                time_saved_ms: 4567,
                ..Default::default()
            },
        )
        .unwrap();

        let counters = get_stat_counters(&conn, "2026-03-30").unwrap().unwrap();
        assert_eq!(counters, (5, 1, 123, 4567));
    }

    #[test]
    fn delete_stat_returns_true_when_date_exists() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_stat(
            &conn,
            "2026-03-30",
            &StatDeltas {
                executions: 42,
                words_expanded: 500,
                time_saved_ms: 12345,
                ..Default::default()
            },
        )
        .unwrap();

        let deleted = delete_stat(&conn, "2026-03-30").unwrap();
        assert!(deleted);
    }

    #[test]
    fn delete_stat_returns_false_when_date_missing() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let deleted = delete_stat(&conn, "ghost").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn delete_stat_actually_removes_the_row() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_stat(
            &conn,
            "2026-03-30",
            &StatDeltas {
                executions: 42,
                words_expanded: 500,
                time_saved_ms: 12345,
                ..Default::default()
            },
        )
        .unwrap();
        delete_stat(&conn, "2026-03-30").unwrap();

        let result = get_stat(&conn, "2026-03-30").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn recorder_tracks_ai_events_separately() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        record_trigger_stat_with_conn(
            &mut conn,
            &TriggerStatEvent {
                trigger_id: None,
                trigger: None,
                trigger_chars: 0,
                success: true,
                output_chars: 12,
                kind: TriggerStatKind::InlineAi,
                wpm: Some(60),
                app: None,
                words_count: Some(2),
                content: None,
            },
        )
        .unwrap();

        let row = get_stat(&conn, &crate::stats::get_current_date_string())
            .unwrap()
            .unwrap();
        assert_eq!(row.executions, 1);
        assert_eq!(row.ai_executions, 1);
        assert_eq!(row.words_expanded, 2);
        assert!(row.time_saved_ms > 0);
    }

    #[test]
    fn recorder_updates_usage_count_and_expanded_words() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        crate::db::crud::upsert_trigger(
            &conn,
            "uuid-stat-recorder",
            None,
            "gm",
            "Good morning!",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        record_trigger_stat_with_conn(
            &mut conn,
            &TriggerStatEvent {
                trigger_id: None,
                trigger: Some("gm".to_string()),
                trigger_chars: 2,
                success: true,
                output_chars: 100,
                kind: TriggerStatKind::Snippet,
                wpm: Some(60),
                app: None,
                words_count: Some(20),
                content: None,
            },
        )
        .unwrap();

        let row = get_stat(&conn, &crate::stats::get_current_date_string())
            .unwrap()
            .unwrap();
        assert_eq!(row.executions, 1);
        assert_eq!(row.ai_executions, 0);
        assert_eq!(row.words_expanded, 20);
        assert!(row.time_saved_ms > 0);

        let trigger = crate::db::crud::get_trigger(&conn, "uuid-stat-recorder")
            .unwrap()
            .unwrap();
        assert_eq!(trigger.usage_count, 1);
    }

    #[test]
    fn recorder_tracks_hotkey_text_expansions_with_proper_savings() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        // Hotkey that injects text should record words and time saved
        record_trigger_stat_with_conn(
            &mut conn,
            &TriggerStatEvent {
                trigger_id: None,
                trigger: None,
                trigger_chars: 0,
                success: true,
                output_chars: 150,
                kind: TriggerStatKind::Hotkey,
                wpm: Some(60),
                app: None,
                words_count: Some(30),
                content: None,
            },
        )
        .unwrap();

        // Script that has 0 output should record 0 words and 0 time saved
        record_trigger_stat_with_conn(
            &mut conn,
            &TriggerStatEvent {
                trigger_id: None,
                trigger: None,
                trigger_chars: 0,
                success: true,
                output_chars: 0,
                kind: TriggerStatKind::Script,
                wpm: Some(60),
                app: None,
                words_count: Some(0),
                content: None,
            },
        )
        .unwrap();

        let row = get_stat(&conn, &crate::stats::get_current_date_string())
            .unwrap()
            .unwrap();
        assert_eq!(row.executions, 2);
        assert_eq!(row.ai_executions, 0);
        assert_eq!(row.words_expanded, 30);
        assert!(row.time_saved_ms > 0);
    }

    #[test]
    fn recorder_records_app_stat_in_same_transaction() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        record_trigger_stat_with_conn(
            &mut conn,
            &TriggerStatEvent {
                trigger_id: None,
                trigger: None,
                trigger_chars: 2,
                success: true,
                output_chars: 50,
                kind: TriggerStatKind::Snippet,
                wpm: Some(60),
                app: Some("google-chrome.exe".to_string()),
                words_count: Some(10),
                content: None,
            },
        )
        .unwrap();

        let top = get_top_app_stats_with_conn(&conn, AppStatsSortBy::Executions, 10).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].app_key, "google-chrome.exe");
        assert_eq!(top[0].display_name, "Google Chrome");
        assert_eq!(top[0].executions, 1);
        assert_eq!(top[0].words_expanded, 10);
    }

    #[test]
    fn recorder_tracks_voice_dictation_and_trigger_stats() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        // 1. Record voice trigger (expands snippet)
        record_trigger_stat_with_conn(
            &mut conn,
            &TriggerStatEvent {
                trigger_id: None,
                trigger: Some("launch notes".to_string()),
                trigger_chars: 12,
                success: true,
                output_chars: 50,
                kind: TriggerStatKind::VoiceTrigger,
                wpm: Some(60),
                app: None,
                words_count: Some(10),
                content: None,
            },
        )
        .unwrap();

        // 2. Record voice dictation (speaks words)
        record_trigger_stat_with_conn(
            &mut conn,
            &TriggerStatEvent {
                trigger_id: None,
                trigger: None,
                trigger_chars: 0,
                success: true,
                output_chars: 120,
                kind: TriggerStatKind::VoiceDictation,
                wpm: Some(60),
                app: None,
                words_count: Some(25),
                content: None,
            },
        )
        .unwrap();

        let row = get_stat(&conn, &crate::stats::get_current_date_string())
            .unwrap()
            .unwrap();
        assert_eq!(row.executions, 0);
        assert_eq!(row.ai_executions, 0);
        assert_eq!(row.voice_executions, 2);
        assert_eq!(row.words_spoken, 25);
        assert_eq!(row.words_expanded, 10);
        assert!(row.time_saved_ms > 0);
    }
}
