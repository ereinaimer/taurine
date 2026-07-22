mod trigger_delete;
mod trigger_get;
mod trigger_set;
mod trigger_sync;
mod trigger_types;

pub use trigger_delete::{
    count_triggers_by_pattern, delete_trigger, delete_trigger_by_value, delete_triggers_by_pattern,
    delete_triggers_by_tag, delete_triggers_by_values,
};

pub use trigger_get::{
    get_action_by_trigger, get_active_word_trigger_history, get_all_active_hotkey_triggers,
    get_all_active_regex_triggers, get_all_active_triggers, get_trigger, get_triggers_list,
    search_triggers,
};
pub use trigger_set::{
    AddOutcome, ExistingTriggerUpdate, NewTrigger, PreparedTrigger, add_trigger,
    add_trigger_by_type, add_trigger_by_type_with_case, add_trigger_with_case, audit_payload_tags,
    audit_payload_tags_with_trigger_type, audit_script_payload_tags, create_trigger,
    find_trigger_overlap_conflict, increment_usage_count_by_trigger, normalize_tags,
    prepare_trigger, prepare_trigger_with_type, record_expansion_usage, target_os_values_overlap,
    update_existing_trigger, update_trigger_app_filters, upsert_script, upsert_trigger,
    upsert_trigger_with_type, upsert_trigger_with_type_and_case, validate_trigger_not_reserved,
    validate_trigger_target_os_conflict,
};
pub use trigger_sync::get_syncable_triggers;
pub use trigger_types::{
    TriggerAction, TriggerConflict, TriggerListItem, TriggerRow, TriggerSummary, TriggerType,
};

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, compress, decompress};
    use crate::settings::SettingsManager;
    use crate::testing::{init_tracing_for_tests, open_test_db};
    use rusqlite::ErrorCode;

    fn insert_raw_trigger(
        conn: &rusqlite::Connection,
        id: &str,
        trigger_type: &str,
        trigger: &str,
        target_os: &str,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO triggers
                (id, name, trigger_type, trigger, output, target_os, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                id,
                format!("Trigger {id}"),
                trigger_type,
                trigger,
                format!("payload-{id}"),
                target_os,
                1_700_000_000_i64,
                1_700_000_000_i64,
            ),
        )
    }

    #[test]
    fn get_trigger_returns_none_for_missing_id() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let result = get_trigger(&conn, "missing").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn upsert_trigger_inserts_new_row_with_version_1() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger(
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

        let row = get_trigger(&conn, "uuid-1").unwrap().unwrap();
        assert_eq!(row.id, "uuid-1");
        assert_eq!(row.name, "Good Morning");
        assert_eq!(row.description, None);
        assert_eq!(row.trigger_type, TriggerType::Word);
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
    fn upsert_trigger_increments_version_on_update() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger(
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

        upsert_trigger(
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

        let row = get_trigger(&conn, "uuid-1").unwrap().unwrap();
        assert_eq!(row.version, 2);
        assert_eq!(row.description.as_deref(), Some("description"));
        assert_eq!(row.trigger_type, TriggerType::Word);
        assert_eq!(row.output, "Good morning!!");
        assert_eq!(row.tags, r#"["morning","bright"]"#);
        assert_eq!(row.usage_count, 7);
        assert_eq!(row.last_used_at, Some(1_700_000_000_i64));
        assert!(!row.is_deleted);
        assert!(row.is_synced);
    }

    #[test]
    fn delete_trigger_tombstones_and_returns_true_once() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger(
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

        let deleted = delete_trigger(&conn, "uuid-1").unwrap();
        assert!(deleted);

        let row = get_trigger(&conn, "uuid-1").unwrap().unwrap();
        assert!(row.is_deleted);
        assert!(row.is_synced);
        let version_after_delete = row.version;

        let deleted_again = delete_trigger(&conn, "uuid-1").unwrap();
        assert!(!deleted_again, "already deleted rows shouldn't change");

        let row2 = get_trigger(&conn, "uuid-1").unwrap().unwrap();
        assert_eq!(row2.version, version_after_delete);
    }

    #[test]
    fn delete_trigger_returns_false_when_missing() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let deleted = delete_trigger(&conn, "ghost").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn delete_triggers_by_values_tombstones_matches() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger(
            &conn, "uuid-1", "A", None, "t1", "out", "text", "all", "[]", 0, None,
        )
        .unwrap();
        upsert_trigger(
            &conn, "uuid-2", "B", None, "t2", "out", "text", "all", "[]", 0, None,
        )
        .unwrap();
        upsert_trigger(
            &conn, "uuid-3", "C", None, "t3", "out", "text", "all", "[]", 0, None,
        )
        .unwrap();

        let triggers = vec!["t1".to_string(), "t3".to_string()];
        let affected = crate::db::crud::delete_triggers_by_values(&conn, &triggers).unwrap();
        assert_eq!(affected, 2);

        assert!(get_trigger(&conn, "uuid-1").unwrap().unwrap().is_deleted);
        assert!(!get_trigger(&conn, "uuid-2").unwrap().unwrap().is_deleted);
        assert!(get_trigger(&conn, "uuid-3").unwrap().unwrap().is_deleted);

        // Ensure returning 0 for empty triggers
        let affected_empty = crate::db::crud::delete_triggers_by_values(&conn, &[]).unwrap();
        assert_eq!(affected_empty, 0);
    }

    #[test]
    fn get_action_by_trigger_returns_none_for_missing_trigger() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        let result = get_action_by_trigger(&conn, "gm").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_all_active_triggers_ignores_deleted_rows() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
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

        upsert_trigger(
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

        // Tombstone one trigger; its trigger must not appear.
        delete_trigger(&conn, "uuid-2").unwrap();

        let rows = get_all_active_triggers(&conn).unwrap();
        let mut triggers: Vec<String> = rows.into_iter().map(|(t, _)| t).collect();
        triggers.sort();

        assert_eq!(triggers, vec!["gm".to_string()]);
    }

    #[test]
    fn get_all_active_triggers_filters_by_target_os() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        // 1. "all" should be loaded everywhere.
        upsert_trigger(
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
        upsert_trigger(
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
        upsert_trigger(
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

        let rows = get_all_active_triggers(&conn).unwrap();
        let mut triggers: Vec<String> = rows.into_iter().map(|(t, _)| t).collect();
        triggers.sort();

        // Should load "gm_all" and "gm_native", but drop "gm_fake".
        assert_eq!(
            triggers,
            vec!["gm_all".to_string(), "gm_native".to_string()]
        );
    }

    #[test]
    fn get_all_active_triggers_excludes_hotkey_triggers() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
            &conn,
            "uuid-word",
            "Word",
            None,
            "gm",
            "payload_word",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        upsert_trigger_with_type(
            &conn,
            "uuid-hotkey",
            "Hotkey",
            None,
            TriggerType::Hotkey,
            "ctrl+shift+g",
            "payload_hotkey",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        let rows = get_all_active_triggers(&conn).unwrap();
        let triggers: Vec<String> = rows.into_iter().map(|(trigger, _)| trigger).collect();
        assert_eq!(triggers, vec!["gm".to_string()]);
    }

    #[test]
    fn active_word_trigger_history_prefers_recency_then_usage_then_alphabetical() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
            &conn,
            "uuid-recent",
            "Recent",
            None,
            "gs",
            "git status",
            "text",
            "all",
            "[]",
            2,
            Some(1_700_000_100_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-older",
            "Older",
            None,
            "email",
            "team update",
            "text",
            "all",
            "[]",
            99,
            Some(1_700_000_050_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-usage",
            "Usage",
            None,
            "uuid",
            "1234",
            "text",
            "all",
            "[]",
            10,
            None,
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-alpha",
            "Alpha",
            None,
            "alpha",
            "abc",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        upsert_trigger_with_type(
            &conn,
            "uuid-hotkey",
            "Hotkey",
            None,
            TriggerType::Hotkey,
            "ctrl+shift+g",
            "git status",
            "text",
            "all",
            "[]",
            500,
            Some(1_700_000_200_i64),
        )
        .unwrap();

        assert_eq!(
            get_active_word_trigger_history(&conn).unwrap(),
            vec![
                "gs".to_string(),
                "email".to_string(),
                "uuid".to_string(),
                "alpha".to_string(),
            ]
        );
    }

    #[test]
    fn active_word_trigger_history_orders_by_last_used_at_desc() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
            &conn,
            "uuid-1",
            "Older",
            None,
            "older",
            "out",
            "text",
            "all",
            "[]",
            50,
            Some(1_700_000_001_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-2",
            "Newest",
            None,
            "newest",
            "out",
            "text",
            "all",
            "[]",
            1,
            Some(1_700_000_100_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-3",
            "Middle",
            None,
            "middle",
            "out",
            "text",
            "all",
            "[]",
            99,
            Some(1_700_000_050_i64),
        )
        .unwrap();

        assert_eq!(
            get_active_word_trigger_history(&conn).unwrap(),
            vec![
                "newest".to_string(),
                "middle".to_string(),
                "older".to_string(),
            ]
        );
    }

    #[test]
    fn active_word_trigger_history_uses_usage_count_when_recency_is_tied_or_absent() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
            &conn,
            "uuid-1",
            "High Usage",
            None,
            "high",
            "out",
            "text",
            "all",
            "[]",
            20,
            Some(1_700_000_100_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-2",
            "Low Usage",
            None,
            "low",
            "out",
            "text",
            "all",
            "[]",
            5,
            Some(1_700_000_100_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-3",
            "Null Recency High",
            None,
            "nullhigh",
            "out",
            "text",
            "all",
            "[]",
            15,
            None,
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-4",
            "Null Recency Low",
            None,
            "nulllow",
            "out",
            "text",
            "all",
            "[]",
            1,
            None,
        )
        .unwrap();

        assert_eq!(
            get_active_word_trigger_history(&conn).unwrap(),
            vec![
                "high".to_string(),
                "low".to_string(),
                "nullhigh".to_string(),
                "nulllow".to_string(),
            ]
        );
    }

    #[test]
    fn active_word_trigger_history_breaks_full_ties_alphabetically() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
            &conn,
            "uuid-1",
            "Zulu",
            None,
            "zulu",
            "out",
            "text",
            "all",
            "[]",
            3,
            Some(1_700_000_100_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-2",
            "Alpha",
            None,
            "alpha",
            "out",
            "text",
            "all",
            "[]",
            3,
            Some(1_700_000_100_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-3",
            "Beta",
            None,
            "Beta",
            "out",
            "text",
            "all",
            "[]",
            3,
            Some(1_700_000_100_i64),
        )
        .unwrap();

        assert_eq!(
            get_active_word_trigger_history(&conn).unwrap(),
            vec!["alpha".to_string(), "Beta".to_string(), "zulu".to_string(),]
        );
    }

    #[test]
    fn active_word_trigger_history_excludes_hotkeys_and_deleted_rows_and_includes_word_scripts() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
            &conn,
            "uuid-word",
            "Word",
            None,
            "gs",
            "git status",
            "text",
            "all",
            "[]",
            1,
            Some(1_700_000_010_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-script",
            "Script",
            None,
            "gbuild",
            "",
            "script",
            "all",
            "[]",
            2,
            Some(1_700_000_020_i64),
        )
        .unwrap();
        upsert_script(
            &conn,
            "uuid-script",
            ScriptInterpreter::Bash,
            ScriptBehavior::Inline,
            &compress("echo build").unwrap(),
        )
        .unwrap();
        upsert_trigger_with_type(
            &conn,
            "uuid-hotkey",
            "Hotkey",
            None,
            TriggerType::Hotkey,
            "ctrl+shift+g",
            "git status",
            "text",
            "all",
            "[]",
            999,
            Some(1_700_000_500_i64),
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-deleted",
            "Deleted",
            None,
            "old",
            "out",
            "text",
            "all",
            "[]",
            100,
            Some(1_700_000_300_i64),
        )
        .unwrap();
        delete_trigger(&conn, "uuid-deleted").unwrap();
        upsert_trigger(
            &conn,
            "uuid-disabled",
            "Disabled",
            None,
            "skipme",
            "out",
            "text",
            "all",
            "[]",
            50,
            Some(1_700_000_200_i64),
        )
        .unwrap();
        conn.execute(
            "UPDATE triggers SET is_enabled = 0 WHERE id = 'uuid-disabled'",
            [],
        )
        .unwrap();

        assert_eq!(
            get_active_word_trigger_history(&conn).unwrap(),
            vec!["gbuild".to_string(), "gs".to_string()]
        );
    }

    #[test]
    fn get_all_active_hotkey_triggers_loads_only_hotkeys() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
            &conn,
            "uuid-word",
            "Word",
            None,
            "gm",
            "payload_word",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        upsert_trigger_with_type(
            &conn,
            "uuid-hotkey",
            "Hotkey",
            None,
            TriggerType::Hotkey,
            "ctrl+shift+g",
            "payload_hotkey",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        let rows = get_all_active_hotkey_triggers(&conn).unwrap();
        let triggers: Vec<String> = rows.into_iter().map(|(trigger, _)| trigger).collect();
        assert_eq!(triggers, vec!["ctrl+shift+g".to_string()]);
    }

    #[test]
    fn upsert_trigger_with_type_round_trips_hotkey() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger_with_type(
            &conn,
            "uuid-hotkey-1",
            "Command Palette",
            None,
            TriggerType::Hotkey,
            "ctrl+shift+p",
            "Open palette",
            "text",
            "win",
            "[]",
            0,
            None,
        )
        .unwrap();

        let row = get_trigger(&conn, "uuid-hotkey-1").unwrap().unwrap();
        assert_eq!(row.trigger_type, TriggerType::Hotkey);
        assert_eq!(row.trigger, "ctrl+shift+p");
        assert_eq!(row.target_os, "win");
    }

    #[test]
    fn active_unique_index_enforces_trigger_type_trigger_and_target_os() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        insert_raw_trigger(&conn, "uuid-1", "word", "gm", "all").unwrap();
        let err = insert_raw_trigger(&conn, "uuid-2", "word", "gm", "all").unwrap_err();

        assert!(matches!(
            err,
            rusqlite::Error::SqliteFailure(ref failure, _)
                if failure.code == ErrorCode::ConstraintViolation
        ));

        insert_raw_trigger(&conn, "uuid-3", "hotkey", "gm", "all")
            .expect("different trigger_type should not hit the unique index");
    }

    #[test]
    fn validate_trigger_target_os_conflict_rejects_all_vs_specific_overlap() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger(
            &conn, "uuid-1", "Greeting", None, "gm", "hello", "text", "all", "[]", 0, None,
        )
        .unwrap();

        let err = validate_trigger_target_os_conflict(
            &conn,
            TriggerType::Word,
            "gm",
            "win",
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("overlaps existing target_os 'all'")
        );
    }

    #[test]
    fn different_trigger_types_do_not_conflict_for_same_trigger_and_target_os() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger(
            &conn,
            "uuid-word",
            "Greeting",
            None,
            "f12",
            "hello",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        upsert_trigger_with_type(
            &conn,
            "uuid-hotkey",
            "Hotkey Greeting",
            None,
            TriggerType::Hotkey,
            "f12",
            "hello hotkey",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        let hotkey_row = get_trigger(&conn, "uuid-hotkey").unwrap().unwrap();
        assert_eq!(hotkey_row.trigger_type, TriggerType::Hotkey);
    }

    #[test]
    fn same_trigger_type_allows_distinct_desktop_os_variants() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger_with_type(
            &conn,
            "uuid-win",
            "Windows Hotkey",
            None,
            TriggerType::Hotkey,
            "ctrl+shift+g",
            "git win",
            "text",
            "win",
            "[]",
            0,
            None,
        )
        .unwrap();

        upsert_trigger_with_type(
            &conn,
            "uuid-linux",
            "Linux Hotkey",
            None,
            TriggerType::Hotkey,
            "ctrl+shift+g",
            "git linux",
            "text",
            "linux",
            "[]",
            0,
            None,
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT COUNT(*)
                 FROM triggers
                 WHERE trigger_type = 'hotkey'
                   AND trigger = 'ctrl+shift+g'
                   AND is_deleted = 0",
            )
            .unwrap();
        let count: i64 = stmt.query_row([], |row| row.get(0)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn hotkey_overlap_validation_treats_generic_and_side_specific_modifiers_as_conflicts() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger_with_type(
            &conn,
            "uuid-alt",
            "Generic Alt",
            None,
            TriggerType::Hotkey,
            "alt+m",
            "generic",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        let err = validate_trigger_target_os_conflict(
            &conn,
            TriggerType::Hotkey,
            "ralt+m",
            "win",
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("Trigger conflict"),
            "unexpected error: {err}"
        );

        let err = validate_trigger_target_os_conflict(
            &conn,
            TriggerType::Hotkey,
            "lalt+m",
            "linux",
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("Trigger conflict"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn hotkey_overlap_validation_allows_distinct_modifier_sides() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger_with_type(
            &conn,
            "uuid-left-alt",
            "Left Alt",
            None,
            TriggerType::Hotkey,
            "lalt+m",
            "left",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        validate_trigger_target_os_conflict(
            &conn,
            TriggerType::Hotkey,
            "ralt+m",
            "all",
            None,
            None,
            None,
        )
        .unwrap();
    }

    #[test]
    fn hotkey_overlap_validation_preserves_target_os_overlap_rules() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_trigger_with_type(
            &conn,
            "uuid-right-alt-win",
            "Right Alt Windows",
            None,
            TriggerType::Hotkey,
            "ralt+m",
            "windows",
            "text",
            "win",
            "[]",
            0,
            None,
        )
        .unwrap();

        validate_trigger_target_os_conflict(
            &conn,
            TriggerType::Hotkey,
            "ralt+m",
            "linux",
            None,
            None,
            None,
        )
        .unwrap();

        let err = validate_trigger_target_os_conflict(
            &conn,
            TriggerType::Hotkey,
            "ralt+m",
            "all",
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("overlaps existing target_os 'win'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn add_trigger_allows_same_hotkey_with_distinct_app_filters() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        // First: ctrl+alt+p restricted to notepad
        let outcome = add_trigger_by_type(
            &conn,
            TriggerType::Hotkey,
            "ctrl+alt+p",
            "Action for Notepad",
            "all",
            Some("exe:notepad"),
            None,
            None,
        )
        .expect("first add should succeed");
        assert_eq!(outcome, AddOutcome::Created);

        // Second: same hotkey restricted to code — should NOT conflict
        let outcome2 = add_trigger_by_type(
            &conn,
            TriggerType::Hotkey,
            "ctrl+alt+p",
            "Action for VS Code",
            "all",
            Some("exe:code"),
            None,
            None,
        )
        .expect("second add with distinct app filter should succeed");
        assert_eq!(outcome2, AddOutcome::Created);
    }

    #[test]
    fn get_action_by_trigger_respects_target_os() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        // A trigger locked to a non-existent OS
        upsert_trigger(
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
        upsert_trigger(
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
    fn search_triggers_matches_name_and_trigger_and_sorts_by_usage() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
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

        upsert_trigger(
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
        upsert_trigger(
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
        delete_trigger(&conn, "uuid-3").unwrap();

        let results = search_triggers(&conn, "morning", 10).unwrap();
        assert_eq!(results.len(), 2);

        // Sorted by usage_count desc: uuid-2 (20) then uuid-1 (5).
        assert_eq!(results[0].id, "uuid-2");
        assert_eq!(results[1].id, "uuid-1");
    }

    #[test]
    fn get_triggers_list_includes_target_os() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        upsert_trigger(
            &conn,
            "uuid-list-1",
            "Windows Opener",
            Some("Open Reddit"),
            "ralt+r",
            "[Script: powershell]",
            "script",
            "all",
            r#"[]"#,
            6,
            None,
        )
        .unwrap();
        upsert_script(
            &conn,
            "uuid-list-1",
            ScriptInterpreter::PowerShell,
            ScriptBehavior::Silent,
            &compress("Start-Process https://reddit.com").unwrap(),
        )
        .unwrap();

        let items = get_triggers_list(&conn).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "uuid-list-1");
        assert_eq!(items[0].target_os, "all");
        assert_eq!(items[0].description.as_deref(), Some("Open Reddit"));
        assert_eq!(
            items[0].script_content.as_deref(),
            Some("Start-Process https://reddit.com")
        );
    }

    #[test]
    fn update_existing_trigger_updates_same_row_by_id() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_trigger(
            &conn,
            "uuid-edit-1",
            "GM",
            None,
            "gm",
            "hello",
            "text",
            "all",
            "[]",
            4,
            None,
        )
        .unwrap();

        update_existing_trigger(
            &mut conn,
            ExistingTriggerUpdate {
                id: "uuid-edit-1",
                name: "GM",
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "gm2",
                content: "hello again",
                action_type: "text",
                target_os: "all",
                tags_json: "[]",
                auto_case: false,
                usage_count: 4,
                last_used_at: None,
                interpreter: None,
                behavior: None,
            },
        )
        .unwrap();

        let row = get_trigger(&conn, "uuid-edit-1").unwrap().unwrap();
        assert_eq!(row.trigger, "gm2");
        assert_eq!(row.output, "hello again");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM triggers WHERE id = 'uuid-edit-1' AND is_deleted = 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn update_existing_script_preserves_script_metadata() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_trigger_with_type(
            &conn,
            "uuid-script-edit",
            "Script Edit",
            Some("Keep description"),
            TriggerType::Hotkey,
            "ralt+m",
            "[Script: powershell]",
            "script",
            "win",
            "[]",
            6,
            Some(10),
        )
        .unwrap();
        upsert_script(
            &conn,
            "uuid-script-edit",
            ScriptInterpreter::PowerShell,
            ScriptBehavior::Silent,
            &compress("Start-Process https://reddit.com").unwrap(),
        )
        .unwrap();

        update_existing_trigger(
            &mut conn,
            ExistingTriggerUpdate {
                id: "uuid-script-edit",
                name: "Script Edit",
                description: Some("Keep description"),
                trigger_type: TriggerType::Hotkey,
                trigger: "ralt+m",
                content: "Start-Process https://news.ycombinator.com",
                action_type: "script",
                target_os: "win",
                tags_json: "[]",
                auto_case: false,
                usage_count: 6,
                last_used_at: Some(10),
                interpreter: Some(ScriptInterpreter::PowerShell),
                behavior: Some(ScriptBehavior::Silent),
            },
        )
        .unwrap();

        let row = get_trigger(&conn, "uuid-script-edit").unwrap().unwrap();
        assert_eq!(row.description.as_deref(), Some("Keep description"));
        assert_eq!(row.output, "[Script: powershell]");
        assert_eq!(row.interpreter, Some(ScriptInterpreter::PowerShell));
        assert_eq!(row.behavior, Some(ScriptBehavior::Silent));
        assert_eq!(
            row.script_binary
                .as_deref()
                .map(|value| decompress(value).unwrap()),
            Some("Start-Process https://news.ycombinator.com".to_string())
        );
    }

    #[test]
    fn update_existing_trigger_removes_stale_script_when_switching_to_text() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_trigger_with_type(
            &conn,
            "uuid-switch-kind",
            "Switch Kind",
            None,
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "linux",
            "[]",
            0,
            None,
        )
        .unwrap();
        upsert_script(
            &conn,
            "uuid-switch-kind",
            ScriptInterpreter::Bash,
            ScriptBehavior::Inline,
            &compress("echo first").unwrap(),
        )
        .unwrap();

        update_existing_trigger(
            &mut conn,
            ExistingTriggerUpdate {
                id: "uuid-switch-kind",
                name: "Switch Kind",
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "deploy",
                content: "echo text now",
                action_type: "text",
                target_os: "linux",
                tags_json: "[]",
                auto_case: false,
                usage_count: 0,
                last_used_at: None,
                interpreter: Some(ScriptInterpreter::Bash),
                behavior: Some(ScriptBehavior::Inline),
            },
        )
        .unwrap();

        let row = get_trigger(&conn, "uuid-switch-kind").unwrap().unwrap();
        assert_eq!(row.action_type, "text");
        assert!(row.script_binary.is_none());
    }

    #[test]
    fn update_existing_trigger_rejects_conflicts_with_other_rows() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_trigger(
            &conn,
            "uuid-conflict-a",
            "A",
            None,
            "gm",
            "hello",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        upsert_trigger(
            &conn,
            "uuid-conflict-b",
            "B",
            None,
            "gs",
            "status",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        let error = update_existing_trigger(
            &mut conn,
            ExistingTriggerUpdate {
                id: "uuid-conflict-b",
                name: "B",
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "gm",
                content: "status",
                action_type: "text",
                target_os: "all",
                tags_json: "[]",
                auto_case: false,
                usage_count: 0,
                last_used_at: None,
                interpreter: None,
                behavior: None,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("Trigger conflict"));
    }

    #[test]
    fn create_trigger_creates_new_text_row() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        let id = create_trigger(
            &mut conn,
            NewTrigger {
                name: None,
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "gm",
                content: "Good Morning",
                action_type: "text",
                target_os: "all",
                tags_json: "[]",
                auto_case: false,
                interpreter: None,
                behavior: None,
            },
        )
        .unwrap();

        let row = get_trigger(&conn, &id).unwrap().unwrap();
        assert_eq!(row.trigger, "gm");
        assert_eq!(row.output, "Good Morning");
        assert_eq!(row.action_type, "text");
        assert_eq!(row.description, None);
    }

    #[test]
    fn create_trigger_creates_script_with_defaults() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        let id = create_trigger(
            &mut conn,
            NewTrigger {
                name: None,
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "deploy",
                content: "#!/usr/bin/env python3\nprint('hi')",
                action_type: "script",
                target_os: "all",
                tags_json: "[]",
                auto_case: false,
                interpreter: None,
                behavior: None,
            },
        )
        .unwrap();

        let row = get_trigger(&conn, &id).unwrap().unwrap();
        assert_eq!(row.action_type, "script");
        assert_eq!(row.output, "[Script: python]");
        assert_eq!(row.interpreter, Some(ScriptInterpreter::Python));
        assert_eq!(row.behavior, Some(ScriptBehavior::Inline));
        assert_eq!(
            decompress(row.script_binary.as_deref().unwrap()).unwrap(),
            "#!/usr/bin/env python3\nprint('hi')"
        );
    }

    #[test]
    fn create_trigger_rejects_conflicts_without_updating_existing_rows() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        let original_id = create_trigger(
            &mut conn,
            NewTrigger {
                name: None,
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "gm",
                content: "Good Morning",
                action_type: "text",
                target_os: "all",
                tags_json: "[]",
                auto_case: false,
                interpreter: None,
                behavior: None,
            },
        )
        .unwrap();

        let error = create_trigger(
            &mut conn,
            NewTrigger {
                name: None,
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "gm",
                content: "Different output",
                action_type: "text",
                target_os: "win",
                tags_json: "[]",
                auto_case: false,
                interpreter: None,
                behavior: None,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("Trigger conflict"));
        let row = get_trigger(&conn, &original_id).unwrap().unwrap();
        assert_eq!(row.output, "Good Morning");
    }

    #[test]
    fn create_trigger_rejects_empty_trigger() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        let error = create_trigger(
            &mut conn,
            NewTrigger {
                name: None,
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "   ",
                content: "Good Morning",
                action_type: "text",
                target_os: "all",
                tags_json: "[]",
                auto_case: false,
                interpreter: None,
                behavior: None,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("Trigger cannot be empty"));
    }

    #[test]
    fn get_syncable_triggers_returns_only_sync_enabled_rows() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        // Standard upserts default to is_synced = 1
        upsert_trigger(
            &conn, "uuid-1", "A1", None, "t1", "p1", "text", "all", r#"[]"#, 0, None,
        )
        .unwrap();

        // Force one to is_synced = 0 manually to test the filter
        upsert_trigger(
            &conn, "uuid-2", "A2", None, "t2", "p2", "text", "all", r#"[]"#, 0, None,
        )
        .unwrap();
        conn.execute("UPDATE triggers SET is_synced = 0 WHERE id = 'uuid-2'", [])
            .unwrap();

        let syncable = get_syncable_triggers(&conn).unwrap();
        let ids: Vec<String> = syncable.into_iter().map(|a| a.id).collect();
        assert_eq!(ids, vec!["uuid-1".to_string()]);
    }

    #[test]
    fn test_record_expansion_usage_updates_trigger_and_stats() {
        let _guard = crate::testing::TEST_LOCK.lock().unwrap();
        init_tracing_for_tests();
        let (dir, conn) = open_test_db();
        let db_path = dir.path().join("test_taurine.db");

        // Set the path for the helper being tested
        unsafe { std::env::set_var("TAURINE_DB_PATH", &db_path) };

        // 1. Setup a trigger
        upsert_trigger(
            &conn,
            "uuid-stats-1",
            "Test Stats",
            None,
            "m",
            "Stats worked!",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        // 2. Call record_expansion_usage
        // trigger="m" (len 1), output="Stats worked!" (len 13), delete_count=3 (">m "), cursors=2
        record_expansion_usage("m", 13, 3, 2);

        // 3. Verify trigger usage_count
        let row = get_trigger(&conn, "uuid-stats-1").unwrap().unwrap();
        assert_eq!(row.usage_count, 1);

        // 4. Verify stats
        let date = crate::stats::get_current_date_string();
        let (executions, ai_executions, saved, time_saved_ms) =
            crate::db::crud::get_stat_counters(&conn, &date)
                .unwrap()
                .unwrap();
        assert_eq!(executions, 1);
        assert_eq!(ai_executions, 0);
        assert_eq!(saved, 12);
        assert!(time_saved_ms > 0);

        // Cleanup
        unsafe { std::env::remove_var("TAURINE_DB_PATH") };
    }

    #[test]
    fn validate_trigger_not_reserved_rejects_ai_keyword() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let err = validate_trigger_not_reserved(&conn, "ai").unwrap_err();
        assert!(
            err.to_string()
                .contains("reserved for Taurine Inline AI Copilot"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_trigger_not_reserved_rejects_prefixed_ai_for_current_trigger_setting() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);
        manager.update_setting("trigger_char", "/").unwrap();

        let err = validate_trigger_not_reserved(&conn, "/ai").unwrap_err();
        assert!(
            err.to_string()
                .contains("reserved for Taurine Inline AI Copilot"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn upsert_trigger_rejects_reserved_ai_trigger() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let err = upsert_trigger(
            &conn,
            "uuid-ai-1",
            "AI",
            None,
            "ai",
            "payload",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("reserved for Taurine Inline AI Copilot"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn add_trigger_rejects_reserved_prefixed_ai_trigger() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);
        manager.update_setting("trigger_char", "#").unwrap();

        let err = add_trigger(&conn, "#ai", "payload", "all", None, None, None).unwrap_err();
        assert!(
            err.to_string()
                .contains("reserved for Taurine Inline AI Copilot"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_add_and_delete_with_tags() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        // Add with tags
        let outcome = add_trigger(
            &conn,
            "t1",
            "output1",
            "all",
            None,
            None,
            Some(vec!["test-tag".to_string(), "shared-tag".to_string()]),
        )
        .unwrap();
        assert_eq!(outcome, AddOutcome::Created);

        let outcome2 = add_trigger(
            &conn,
            "t2",
            "output2",
            "all",
            None,
            None,
            Some(vec!["other-tag".to_string(), "shared-tag".to_string()]),
        )
        .unwrap();
        assert_eq!(outcome2, AddOutcome::Created);

        // Retrieve and check tags
        let tags: String = conn
            .query_row(
                "SELECT tags FROM triggers WHERE trigger = 't1' AND is_deleted = 0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tags, r#"["test-tag","shared-tag"]"#);

        // Delete by shared-tag
        let deleted = delete_triggers_by_tag(&conn, "shared-tag").unwrap();
        assert_eq!(deleted, 2);

        // Retrieve and verify tombstoned
        let is_deleted: bool = conn
            .query_row(
                "SELECT is_deleted FROM triggers WHERE trigger = 't1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(is_deleted);
    }

    #[test]
    fn test_add_and_retrieve_with_auto_case() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        // Add with auto_case: true
        let outcome =
            add_trigger_with_case(&conn, "btw", "by the way", "all", None, None, None, true)
                .unwrap();
        assert_eq!(outcome, AddOutcome::Created);

        // Retrieve and check auto_case
        let action = get_action_by_trigger(&conn, "btw").unwrap().unwrap();
        assert!(action.auto_case);

        // Retrieve row and check auto_case
        let row_id: String = conn
            .query_row(
                "SELECT id FROM triggers WHERE trigger = 'btw' AND is_deleted = 0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let row = get_trigger(&conn, &row_id).unwrap().unwrap();
        assert!(row.auto_case);
    }

    #[test]
    fn delete_triggers_by_pattern_matches_glob_prefix() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        insert_raw_trigger(&conn, "uuid-1", "word", "test_foo", "all").unwrap();
        insert_raw_trigger(&conn, "uuid-2", "word", "test_bar", "all").unwrap();
        insert_raw_trigger(&conn, "uuid-3", "word", "other", "all").unwrap();

        let count = crate::db::crud::count_triggers_by_pattern(&conn, "test_*").unwrap();
        assert_eq!(count, 2);

        let deleted = crate::db::crud::delete_triggers_by_pattern(&conn, "test_*").unwrap();
        assert_eq!(deleted, 2);

        assert!(
            crate::db::crud::get_trigger(&conn, "uuid-1")
                .unwrap()
                .unwrap()
                .is_deleted
        );
        assert!(
            crate::db::crud::get_trigger(&conn, "uuid-2")
                .unwrap()
                .unwrap()
                .is_deleted
        );
        assert!(
            !crate::db::crud::get_trigger(&conn, "uuid-3")
                .unwrap()
                .unwrap()
                .is_deleted
        );
    }

    #[test]
    fn count_triggers_by_pattern_returns_zero_for_no_match() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        insert_raw_trigger(&conn, "uuid-1", "word", "test_foo", "all").unwrap();

        let count = crate::db::crud::count_triggers_by_pattern(&conn, "nomatch_*").unwrap();
        assert_eq!(count, 0);

        let deleted = crate::db::crud::delete_triggers_by_pattern(&conn, "nomatch_*").unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn delete_triggers_by_pattern_ignores_already_deleted() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        conn.execute("DELETE FROM triggers", []).unwrap();

        insert_raw_trigger(&conn, "uuid-1", "word", "test_foo", "all").unwrap();
        crate::db::crud::delete_trigger(&conn, "uuid-1").unwrap();

        let count = crate::db::crud::count_triggers_by_pattern(&conn, "test_*").unwrap();
        assert_eq!(count, 0);

        let deleted = crate::db::crud::delete_triggers_by_pattern(&conn, "test_*").unwrap();
        assert_eq!(deleted, 0);
    }
}
