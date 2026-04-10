mod automation_delete;
mod automation_get;
mod automation_set;
mod automation_sync;
mod automation_types;

pub use automation_delete::{delete_automation, delete_automation_by_trigger};
pub use automation_get::{
    get_action_by_trigger, get_all_active_automations, get_automation, search_automations,
};
pub use automation_set::{
    AddOutcome, add_automation_by_trigger, increment_usage_count_by_trigger,
    record_expansion_usage, upsert_automation,
};
pub use automation_sync::get_syncable_automations;
pub use automation_types::{AutomationAction, AutomationRow, AutomationSummary};

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{init_tracing_for_tests, open_test_db};

    #[test]
    fn get_automation_returns_none_for_missing_id() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let result = get_automation(&conn, "missing").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn upsert_automation_inserts_new_row_with_version_1() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_automation(
            &conn,
            "uuid-1",
            "Good Morning",
            None,
            "gm",
            "Good morning!",
            "text",
            "all",
            r#"["morning"]"#,
            0,
            None,
        )
        .unwrap();

        let row = get_automation(&conn, "uuid-1").unwrap().unwrap();
        assert_eq!(row.id, "uuid-1");
        assert_eq!(row.name, "Good Morning");
        assert_eq!(row.description, None);
        assert_eq!(row.trigger, "gm");
        assert_eq!(row.output, "Good morning!");
        assert_eq!(row.action_type, "text");
        assert_eq!(row.target_os, "all");
        assert_eq!(row.tags, r#"["morning"]"#);
        assert_eq!(row.usage_count, 0);
        assert_eq!(row.last_used_at, None);
        assert!(row.created_at > 0);
        assert!(row.updated_at > 0);
        assert_eq!(row.version, 1);
        assert!(!row.is_deleted);
        assert!(row.is_synced); // default configuration should be syncable
    }

    #[test]
    fn upsert_automation_increments_version_on_update() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_automation(
            &conn,
            "uuid-1",
            "Good Morning",
            None,
            "gm",
            "Good morning!",
            "text",
            "all",
            r#"["morning"]"#,
            0,
            None,
        )
        .unwrap();

        upsert_automation(
            &conn,
            "uuid-1",
            "Good Morning",
            Some("description"),
            "gm",
            "Good morning!!",
            "text",
            "all",
            r#"["morning","bright"]"#,
            7,
            Some(1_700_000_000_i64),
        )
        .unwrap();

        let row = get_automation(&conn, "uuid-1").unwrap().unwrap();
        assert_eq!(row.version, 2);
        assert_eq!(row.description.as_deref(), Some("description"));
        assert_eq!(row.output, "Good morning!!");
        assert_eq!(row.tags, r#"["morning","bright"]"#);
        assert_eq!(row.usage_count, 7);
        assert_eq!(row.last_used_at, Some(1_700_000_000_i64));
        assert!(!row.is_deleted);
        assert!(row.is_synced);
    }

    #[test]
    fn delete_automation_tombstones_and_returns_true_once() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_automation(
            &conn,
            "uuid-1",
            "Good Morning",
            None,
            "gm",
            "Good morning!",
            "text",
            "all",
            r#"["morning"]"#,
            0,
            None,
        )
        .unwrap();

        let deleted = delete_automation(&conn, "uuid-1").unwrap();
        assert!(deleted);

        let row = get_automation(&conn, "uuid-1").unwrap().unwrap();
        assert!(row.is_deleted);
        assert!(row.is_synced);
        let version_after_delete = row.version;

        let deleted_again = delete_automation(&conn, "uuid-1").unwrap();
        assert!(!deleted_again, "already deleted rows shouldn't change");

        let row2 = get_automation(&conn, "uuid-1").unwrap().unwrap();
        assert_eq!(row2.version, version_after_delete);
    }

    #[test]
    fn delete_automation_returns_false_when_missing() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let deleted = delete_automation(&conn, "ghost").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn get_action_by_trigger_returns_none_for_missing_trigger() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM automations", []).unwrap();

        let result = get_action_by_trigger(&conn, "gm").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_all_active_automations_ignores_deleted_rows() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM automations", []).unwrap();

        upsert_automation(
            &conn,
            "uuid-1",
            "GM One",
            None,
            "gm",
            "Good morning one!",
            "text",
            "all",
            r#"[]"#,
            1,
            None,
        )
        .unwrap();

        upsert_automation(
            &conn,
            "uuid-2",
            "GM Two",
            None,
            "gm2",
            "Good morning two!",
            "text",
            "all",
            r#"[]"#,
            1,
            None,
        )
        .unwrap();

        // Tombstone one automation; its trigger must not appear.
        delete_automation(&conn, "uuid-2").unwrap();

        let rows = get_all_active_automations(&conn).unwrap();
        let mut triggers: Vec<String> = rows.into_iter().map(|(t, _)| t).collect();
        triggers.sort();

        assert_eq!(triggers, vec!["gm".to_string()]);
    }

    #[test]
    fn get_all_active_automations_filters_by_target_os() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM automations", []).unwrap();

        // 1. "all" should be loaded everywhere.
        upsert_automation(
            &conn,
            "uuid-1",
            "GM All",
            None,
            "gm_all",
            "payload_all",
            "text",
            "all",
            "[]",
            1,
            None,
        )
        .unwrap();

        // 2. A completely unrecognized/fake OS should never be loaded on any platform.
        upsert_automation(
            &conn,
            "uuid-2",
            "GM Fake OS",
            None,
            "gm_fake",
            "payload_fake",
            "text",
            "fake_test_os",
            "[]",
            1,
            None,
        )
        .unwrap();

        // 3. The exact match for current platform. We evaluate it ourselves to insert an exact match.
        let current_os = match std::env::consts::OS {
            "windows" => "win",
            "macos" => "mac",
            "linux" => "linux",
            "android" => "android",
            "ios" => "ios",
            _ => "unknown",
        };
        upsert_automation(
            &conn,
            "uuid-3",
            "GM Native",
            None,
            "gm_native",
            "payload_native",
            "text",
            current_os,
            "[]",
            1,
            None,
        )
        .unwrap();

        let rows = get_all_active_automations(&conn).unwrap();
        let mut triggers: Vec<String> = rows.into_iter().map(|(t, _)| t).collect();
        triggers.sort();

        // Should load "gm_all" and "gm_native", but drop "gm_fake".
        assert_eq!(
            triggers,
            vec!["gm_all".to_string(), "gm_native".to_string()]
        );
    }

    #[test]
    fn get_action_by_trigger_picks_active_most_used_automation() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        // Two automations with the same trigger; the one with higher usage_count should win.
        upsert_automation(
            &conn,
            "uuid-1",
            "GM One",
            None,
            "gm",
            "Good morning one!",
            "text",
            "all",
            r#"[]"#,
            1,
            None,
        )
        .unwrap();

        upsert_automation(
            &conn,
            "uuid-2",
            "GM Two",
            None,
            "gm",
            "Good morning two!",
            "text",
            "all",
            r#"[]"#,
            10,
            None,
        )
        .unwrap();

        let action = get_action_by_trigger(&conn, "gm").unwrap().unwrap();
        assert_eq!(action.output, "Good morning two!");
        assert_eq!(action.action_type, "text");
    }

    #[test]
    fn get_action_by_trigger_respects_target_os() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM automations", []).unwrap();

        // A trigger locked to a non-existent OS
        upsert_automation(
            &conn,
            "uuid-1",
            "Mac specific",
            None,
            "t_mac",
            "Apple",
            "text",
            "fake_test_mac",
            "[]",
            1,
            None,
        )
        .unwrap();

        // A universal trigger
        upsert_automation(
            &conn,
            "uuid-2",
            "Universal",
            None,
            "t_all",
            "World",
            "text",
            "all",
            "[]",
            1,
            None,
        )
        .unwrap();

        let action_bad = get_action_by_trigger(&conn, "t_mac").unwrap();
        assert!(
            action_bad.is_none(),
            "Should not return action for mismatched target_os"
        );

        let action_good = get_action_by_trigger(&conn, "t_all").unwrap().unwrap();
        assert_eq!(action_good.output, "World");
    }

    #[test]
    fn search_automations_matches_name_and_trigger_and_sorts_by_usage() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM automations", []).unwrap();

        upsert_automation(
            &conn,
            "uuid-1",
            "Good Morning",
            Some("Say good morning"),
            "gm",
            "Good morning!",
            "text",
            "all",
            r#"[]"#,
            5,
            None,
        )
        .unwrap();

        upsert_automation(
            &conn,
            "uuid-2",
            "Morning Standup",
            Some("Daily standup snippet"),
            "standup",
            "Standup notes",
            "text",
            "all",
            r#"[]"#,
            20,
            None,
        )
        .unwrap();

        // Tombstoned rows must not appear in search results.
        upsert_automation(
            &conn,
            "uuid-3",
            "Old Morning Thing",
            None,
            "oldgm",
            "Old",
            "text",
            "all",
            r#"[]"#,
            100,
            None,
        )
        .unwrap();
        delete_automation(&conn, "uuid-3").unwrap();

        let results = search_automations(&conn, "morning", 10).unwrap();
        assert_eq!(results.len(), 2);

        // Sorted by usage_count desc: uuid-2 (20) then uuid-1 (5).
        assert_eq!(results[0].id, "uuid-2");
        assert_eq!(results[1].id, "uuid-1");
    }

    #[test]
    fn get_syncable_automations_returns_only_sync_enabled_rows() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM automations", []).unwrap();

        // Standard upserts default to is_synced = 1
        upsert_automation(
            &conn, "uuid-1", "A1", None, "t1", "p1", "text", "all", r#"[]"#, 0, None,
        )
        .unwrap();

        // Force one to is_synced = 0 manually to test the filter
        upsert_automation(
            &conn, "uuid-2", "A2", None, "t2", "p2", "text", "all", r#"[]"#, 0, None,
        )
        .unwrap();
        conn.execute(
            "UPDATE automations SET is_synced = 0 WHERE id = 'uuid-2'",
            [],
        )
        .unwrap();

        let syncable = get_syncable_automations(&conn).unwrap();
        let ids: Vec<String> = syncable.into_iter().map(|a| a.id).collect();
        assert_eq!(ids, vec!["uuid-1".to_string()]);
    }

    #[test]
    fn test_record_expansion_usage_updates_automation_and_metrics() {
        init_tracing_for_tests();
        let (dir, conn) = open_test_db();
        let db_path = dir.path().join("test_taurine.db");

        // Set the path for the helper being tested
        unsafe { std::env::set_var("TAURINE_DB_PATH", &db_path) };

        // 1. Setup an automation
        upsert_automation(
            &conn,
            "uuid-metrics-1",
            "Test Metrics",
            None,
            "m",
            "Metrics worked!",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        // 2. Call record_expansion_usage
        // trigger="m" (len 1), output="Metrics worked!" (len 15), delete_count=3 (">m "), cursors=2
        record_expansion_usage("m", 15, 3, 2);

        // 3. Verify automation usage_count
        let row = get_automation(&conn, "uuid-metrics-1").unwrap().unwrap();
        assert_eq!(row.usage_count, 1);

        // 4. Verify metrics
        let date = crate::metrics::get_current_date_string();
        let (executions, saved) = crate::db::crud::get_metric_counters(&conn, &date)
            .unwrap()
            .unwrap();
        assert_eq!(executions, 1);
        assert_eq!(saved, 14); // (15 + 2) - 3 = 14

        // Cleanup
        unsafe { std::env::remove_var("TAURINE_DB_PATH") };
    }
}
