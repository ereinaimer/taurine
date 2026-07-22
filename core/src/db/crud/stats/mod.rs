mod recorder;
mod stat_delete;
mod stat_get;
mod stat_set;
mod stat_types;

use crate::stats::TriggerStatKind;

pub use recorder::{TriggerStatEvent, record_trigger_stat, record_trigger_stat_with_conn};
pub use stat_delete::delete_stat;
pub use stat_get::{get_stat, get_stat_counters};
pub use stat_set::increment_stat;
pub use stat_types::StatRow;

/// High-level function to record stats for a mathematical calculation.
///
/// Unlike `record_expansion_usage`, this does not touch the `triggers` table.
pub fn record_calculation_usage(output_len: usize, delete_count: usize, left_arrow_count: usize) {
    let trigger_chars = delete_count
        .saturating_sub(left_arrow_count)
        .saturating_sub(2);
    record_trigger_stat(TriggerStatEvent {
        trigger: None,
        trigger_chars,
        success: output_len > 0,
        output_chars: output_len,
        kind: TriggerStatKind::Calculation,
        wpm: None,
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

        increment_stat(&conn, "2026-03-30", 42, 3, 500, 60000).unwrap();

        let row = get_stat(&conn, "2026-03-30").unwrap().unwrap();
        assert_eq!(row.date, "2026-03-30");
        assert_eq!(row.executions, 42);
        assert_eq!(row.ai_executions, 3);
        assert_eq!(row.keystrokes_saved, 500);
        assert_eq!(row.time_saved_ms, 60000);
        assert_eq!(row.version, 1);
        assert!(row.updated_at > 0);
    }

    #[test]
    fn increment_stat_updates_counters_and_increments_version() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_stat(&conn, "2026-03-30", 1, 1, 10, 1000).unwrap();
        increment_stat(&conn, "2026-03-30", 2, 3, 20, 2000).unwrap();

        let row = get_stat(&conn, "2026-03-30").unwrap().unwrap();
        assert_eq!(row.version, 2);
        assert_eq!(row.executions, 3);
        assert_eq!(row.ai_executions, 4);
        assert_eq!(row.keystrokes_saved, 30);
        assert_eq!(row.time_saved_ms, 3000);
    }

    #[test]
    fn get_stat_counters_returns_updated_values() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_stat(&conn, "2026-03-30", 5, 1, 123, 4567).unwrap();

        let counters = get_stat_counters(&conn, "2026-03-30").unwrap().unwrap();
        assert_eq!(counters, (5, 1, 123, 4567));
    }

    #[test]
    fn delete_stat_returns_true_when_date_exists() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_stat(&conn, "2026-03-30", 42, 0, 500, 12345).unwrap();

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

        increment_stat(&conn, "2026-03-30", 42, 0, 500, 12345).unwrap();
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
                trigger: None,
                trigger_chars: 0,
                success: true,
                output_chars: 12,
                kind: TriggerStatKind::InlineAi,
                wpm: Some(60),
            },
        )
        .unwrap();

        let row = get_stat(&conn, &crate::stats::get_current_date_string())
            .unwrap()
            .unwrap();
        assert_eq!(row.executions, 0);
        assert_eq!(row.ai_executions, 1);
        assert_eq!(row.keystrokes_saved, 0);
        assert_eq!(row.time_saved_ms, 0);
    }

    #[test]
    fn recorder_updates_usage_count_and_typed_savings() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        crate::db::crud::upsert_trigger(
            &conn,
            "uuid-stat-recorder",
            "Greeting",
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
                trigger: Some("gm".to_string()),
                trigger_chars: 2,
                success: true,
                output_chars: 100,
                kind: TriggerStatKind::Snippet,
                wpm: Some(60),
            },
        )
        .unwrap();

        let row = get_stat(&conn, &crate::stats::get_current_date_string())
            .unwrap()
            .unwrap();
        assert_eq!(row.executions, 1);
        assert_eq!(row.ai_executions, 0);
        assert_eq!(row.keystrokes_saved, 98);
        assert!(row.time_saved_ms > 0);

        let trigger = crate::db::crud::get_trigger(&conn, "uuid-stat-recorder")
            .unwrap()
            .unwrap();
        assert_eq!(trigger.usage_count, 1);
    }

    #[test]
    fn recorder_tracks_hotkey_and_script_events_with_zero_savings() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        record_trigger_stat_with_conn(
            &mut conn,
            &TriggerStatEvent {
                trigger: None,
                trigger_chars: 0,
                success: true,
                output_chars: 150,
                kind: TriggerStatKind::Hotkey,
                wpm: Some(60),
            },
        )
        .unwrap();

        record_trigger_stat_with_conn(
            &mut conn,
            &TriggerStatEvent {
                trigger: None,
                trigger_chars: 0,
                success: true,
                output_chars: 200,
                kind: TriggerStatKind::Script,
                wpm: Some(60),
            },
        )
        .unwrap();

        let row = get_stat(&conn, &crate::stats::get_current_date_string())
            .unwrap()
            .unwrap();
        assert_eq!(row.executions, 2);
        assert_eq!(row.ai_executions, 0);
        assert_eq!(row.keystrokes_saved, 0);
        assert_eq!(row.time_saved_ms, 0);
    }
}
