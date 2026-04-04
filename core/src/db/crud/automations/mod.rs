mod automation_delete;
mod automation_get;
mod automation_set;
mod automation_sync;
mod automation_types;

pub use automation_delete::delete_automation;
pub use automation_get::{
    get_action_by_trigger, get_all_active_automations, get_automation, search_automations,
};
pub use automation_set::upsert_automation;
pub use automation_sync::get_syncable_automations;
pub use automation_types::{AutomationAction, AutomationRow, AutomationSummary};

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_test_db;

    #[test]
    fn get_automation_returns_none_for_missing_id() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let result = get_automation(&conn, "missing").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn upsert_automation_inserts_new_row_with_version_1() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_automation(
            &conn,
            "uuid-1",
            "Good Morning",
            None,
            "gm",
            "Good morning!",
            "text",
            false,
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
        assert_eq!(row.payload, "Good morning!");
        assert_eq!(row.action_type, "text");
        assert!(!row.is_regex);
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
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_automation(
            &conn,
            "uuid-1",
            "Good Morning",
            None,
            "gm",
            "Good morning!",
            "text",
            false,
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
            false,
            "all",
            r#"["morning","bright"]"#,
            7,
            Some(1_700_000_000_i64),
        )
        .unwrap();

        let row = get_automation(&conn, "uuid-1").unwrap().unwrap();
        assert_eq!(row.version, 2);
        assert_eq!(row.description.as_deref(), Some("description"));
        assert_eq!(row.payload, "Good morning!!");
        assert_eq!(row.tags, r#"["morning","bright"]"#);
        assert_eq!(row.usage_count, 7);
        assert_eq!(row.last_used_at, Some(1_700_000_000_i64));
        assert!(!row.is_deleted);
        assert!(row.is_synced);
    }

    #[test]
    fn delete_automation_tombstones_and_returns_true_once() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_automation(
            &conn,
            "uuid-1",
            "Good Morning",
            None,
            "gm",
            "Good morning!",
            "text",
            false,
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
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let deleted = delete_automation(&conn, "ghost").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn get_action_by_trigger_returns_none_for_missing_trigger() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let result = get_action_by_trigger(&conn, "gm").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_all_active_automations_ignores_deleted_rows() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_automation(
            &conn,
            "uuid-1",
            "GM One",
            None,
            "gm",
            "Good morning one!",
            "text",
            false,
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
            false,
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
    fn get_action_by_trigger_picks_active_most_used_automation() {
        crate::logs::init_tracing_for_tests();
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
            false,
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
            false,
            "all",
            r#"[]"#,
            10,
            None,
        )
        .unwrap();

        let action = get_action_by_trigger(&conn, "gm").unwrap().unwrap();
        assert_eq!(action.payload, "Good morning two!");
        assert_eq!(action.action_type, "text");
    }

    #[test]
    fn search_automations_matches_name_and_trigger_and_sorts_by_usage() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_automation(
            &conn,
            "uuid-1",
            "Good Morning",
            Some("Say good morning"),
            "gm",
            "Good morning!",
            "text",
            false,
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
            false,
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
            false,
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

        // Standard upserts default to is_synced = 1
        upsert_automation(
            &conn, "uuid-1", "A1", None, "t1", "p1", "text", false, "all", r#"[]"#, 0, None,
        )
        .unwrap();

        // Force one to is_synced = 0 manually to test the filter
        upsert_automation(
            &conn, "uuid-2", "A2", None, "t2", "p2", "text", false, "all", r#"[]"#, 0, None,
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
}
