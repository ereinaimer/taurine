mod metric_delete;
mod metric_get;
mod metric_set;
mod metric_types;

use rusqlite::Connection;

pub use metric_delete::delete_metric;
pub use metric_get::{get_metric, get_metric_counters};
pub use metric_set::increment_metric;
pub use metric_types::MetricRow;

/// High-level function to record metrics for a mathematical calculation.
///
/// Unlike `record_expansion_usage`, this does not touch the `automations` table.
pub fn record_calculation_usage(output_len: usize, delete_count: usize, left_arrow_count: usize) {
    match Connection::open(crate::paths::get_db_path()) {
        Ok(conn) => {
            let date = crate::metrics::get_current_date_string();
            let saved = crate::metrics::calculate_saved_keystrokes(
                output_len,
                delete_count,
                left_arrow_count,
            );

            if let Err(e) = increment_metric(&conn, &date, 1, saved) {
                tracing::warn!(
                    error = %e,
                    "record_calculation_usage: failed to increment metric"
                );
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "record_calculation_usage: could not open DB");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{init_tracing_for_tests, open_test_db};

    #[test]
    fn get_metric_returns_none_for_missing_date() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let result = get_metric(&conn, "2099-01-01").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_metric_counters_returns_none_for_missing_date() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let result = get_metric_counters(&conn, "2099-01-01").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn increment_metric_inserts_new_row_with_version_1() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_metric(&conn, "2026-03-30", 42, 500).unwrap();

        let row = get_metric(&conn, "2026-03-30").unwrap().unwrap();
        assert_eq!(row.date, "2026-03-30");
        assert_eq!(row.executions, 42);
        assert_eq!(row.keystrokes_saved, 500);
        assert_eq!(row.version, 1);
        assert!(row.updated_at > 0);
    }

    #[test]
    fn increment_metric_updates_counters_and_increments_version() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_metric(&conn, "2026-03-30", 1, 10).unwrap();
        increment_metric(&conn, "2026-03-30", 2, 20).unwrap();

        let row = get_metric(&conn, "2026-03-30").unwrap().unwrap();
        assert_eq!(row.version, 2);
        assert_eq!(row.executions, 3);
        assert_eq!(row.keystrokes_saved, 30);
    }

    #[test]
    fn get_metric_counters_returns_updated_values() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_metric(&conn, "2026-03-30", 5, 123).unwrap();

        let counters = get_metric_counters(&conn, "2026-03-30").unwrap().unwrap();
        assert_eq!(counters, (5, 123));
    }

    #[test]
    fn delete_metric_returns_true_when_date_exists() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_metric(&conn, "2026-03-30", 42, 500).unwrap();

        let deleted = delete_metric(&conn, "2026-03-30").unwrap();
        assert!(deleted);
    }

    #[test]
    fn delete_metric_returns_false_when_date_missing() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let deleted = delete_metric(&conn, "ghost").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn delete_metric_actually_removes_the_row() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_metric(&conn, "2026-03-30", 42, 500).unwrap();
        delete_metric(&conn, "2026-03-30").unwrap();

        let result = get_metric(&conn, "2026-03-30").unwrap();
        assert!(result.is_none());
    }
}
