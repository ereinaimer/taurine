use taurine_core::db::crud::AddOutcome;
use taurine_core::db::init;
use tracing::info;

pub fn execute(
    trigger: String,
    output: String,
    os: String,
    use_hotkey: bool,
) -> taurine_core::error::Result<()> {
    use crate::commands::validate::{audit_payload_tags, prepare_trigger};
    use taurine_core::db::crud::{
        TriggerType, add_automation_by_trigger, add_automation_by_trigger_type,
        validate_trigger_not_reserved,
    };
    use taurine_core::engine::variables::system::validate_output;

    audit_payload_tags(&output)?;
    let prepared = prepare_trigger(&trigger, use_hotkey, &os)?;
    let stored_trigger = prepared.stored_trigger;

    // Validate the snippet output for potential issues (cursors, conflicts, etc.)
    // Warnings are printed to the console via tracing::warn!
    validate_output(&output, Some(&stored_trigger));

    let conn = init::setup()?;
    let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
    if !settings.clipboard_history_enabled && output.contains("[clip") {
        tracing::warn!(
            "Warning: The automation contains '[clip]' system variables, which won't work because clipboard history is disabled in the settings."
        );
    }
    validate_trigger_not_reserved(&conn, &stored_trigger)?;
    let outcome = match prepared.trigger_type {
        TriggerType::Word => add_automation_by_trigger(&conn, &stored_trigger, &output, &os)?,
        TriggerType::Hotkey => add_automation_by_trigger_type(
            &conn,
            TriggerType::Hotkey,
            &stored_trigger,
            &output,
            &os,
        )?,
    };

    match outcome {
        AddOutcome::Created => {
            info!("Added automation: {}", stored_trigger);
            taurine_core::rpc::notify_daemon_reload();
        }
        AddOutcome::AlreadyExists => {
            info!("Automation already exists: {}", stored_trigger);
        }
        AddOutcome::Updated => {
            info!("Updated automation: {}", stored_trigger);
            taurine_core::rpc::notify_daemon_reload();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use taurine_core::logs::init_tracing_for_tests;

    struct TestDbEnvGuard {
        path: PathBuf,
    }

    impl TestDbEnvGuard {
        fn new(path: PathBuf) -> Self {
            let db_path_str = path.to_string_lossy().to_string();
            unsafe { std::env::set_var("TAURINE_DB_PATH", &db_path_str) };
            Self { path }
        }

        fn db_path(&self) -> String {
            self.path.to_string_lossy().to_string()
        }
    }

    impl Drop for TestDbEnvGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("TAURINE_DB_PATH") };
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn with_test_db<T>(f: impl FnOnce(&str) -> T) -> T {
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new(
            std::env::temp_dir().join(format!("taurine-cli-add-{}.db", uuid::Uuid::new_v4())),
        );
        let db_path = db_guard.db_path();
        f(&db_path)
    }

    #[test]
    fn normal_add_still_creates_word_trigger_by_default() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute(
                "gs".to_string(),
                "git status".to_string(),
                "all".to_string(),
                false,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let stored: (String, String) = conn
                .query_row(
                    "SELECT trigger_type, trigger FROM automations WHERE is_deleted = 0 LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(stored.0, "word");
            assert_eq!(stored.1, "gs");
        });
    }

    #[test]
    fn add_hotkey_creates_canonical_hotkey_trigger() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute(
                "Shift + Ctrl + G".to_string(),
                "git status[key.enter]".to_string(),
                "all".to_string(),
                true,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let stored: (String, String) = conn
                .query_row(
                    "SELECT trigger_type, trigger FROM automations WHERE is_deleted = 0 LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(stored.0, "hotkey");
            assert_eq!(stored.1, "ctrl+shift+g");
        });
    }

    #[test]
    fn add_hotkey_rejects_overlapping_desktop_conflicts_and_allows_non_overlapping_ones() {
        init_tracing_for_tests();

        with_test_db(|_db_path| {
            execute(
                "ctrl+shift+g".to_string(),
                "one".to_string(),
                "all".to_string(),
                true,
            )
            .unwrap();

            let error = execute(
                "ctrl+shift+g".to_string(),
                "two".to_string(),
                "win".to_string(),
                true,
            )
            .unwrap_err();
            assert!(error.to_string().contains("Trigger conflict"));
        });

        with_test_db(|db_path| {
            execute(
                "ctrl+shift+g".to_string(),
                "windows".to_string(),
                "win".to_string(),
                true,
            )
            .unwrap();
            execute(
                "ctrl+shift+g".to_string(),
                "linux".to_string(),
                "linux".to_string(),
                true,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM automations WHERE trigger_type = 'hotkey' AND trigger = 'ctrl+shift+g' AND is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 2);
        });
    }

    #[test]
    fn add_hotkey_rejects_generic_vs_side_specific_overlap_and_allows_distinct_sides() {
        init_tracing_for_tests();

        with_test_db(|_db_path| {
            execute(
                "alt+m".to_string(),
                "one".to_string(),
                "all".to_string(),
                true,
            )
            .unwrap();

            let error = execute(
                "ralt+m".to_string(),
                "two".to_string(),
                "win".to_string(),
                true,
            )
            .unwrap_err();
            assert!(error.to_string().contains("Trigger conflict"));
        });

        with_test_db(|db_path| {
            execute(
                "lalt+m".to_string(),
                "left".to_string(),
                "all".to_string(),
                true,
            )
            .unwrap();
            execute(
                "ralt+m".to_string(),
                "right".to_string(),
                "all".to_string(),
                true,
            )
            .unwrap();

            let count: i64 = rusqlite::Connection::open(db_path)
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM automations WHERE trigger_type = 'hotkey' AND is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 2);
        });
    }

    #[test]
    fn add_hotkey_updates_exact_duplicate_registration() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute(
                "ctrl+shift+g".to_string(),
                "one".to_string(),
                "win".to_string(),
                true,
            )
            .unwrap();

            // Adding same trigger + target_os but different output should update
            execute(
                "ctrl+shift+g".to_string(),
                "two".to_string(),
                "win".to_string(),
                true,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let mut stmt = conn.prepare("SELECT output FROM automations WHERE trigger_type = 'hotkey' AND trigger = 'ctrl+shift+g' AND is_deleted = 0").unwrap();
            let mut rows = stmt.query([]).unwrap();

            let row = rows.next().unwrap().unwrap();
            assert_eq!(row.get::<_, String>(0).unwrap(), "two");
            assert!(
                rows.next().unwrap().is_none(),
                "Should not create duplicate rows"
            );
        });
    }

    #[test]
    fn add_word_updates_exact_duplicate_registration() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute(
                "gs".to_string(),
                "one".to_string(),
                "all".to_string(),
                false,
            )
            .unwrap();

            execute(
                "gs".to_string(),
                "two".to_string(),
                "all".to_string(),
                false,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let mut stmt = conn.prepare("SELECT output FROM automations WHERE trigger_type = 'word' AND trigger = 'gs' AND is_deleted = 0").unwrap();
            let mut rows = stmt.query([]).unwrap();

            let row = rows.next().unwrap().unwrap();
            assert_eq!(row.get::<_, String>(0).unwrap(), "two");
            assert!(
                rows.next().unwrap().is_none(),
                "Should not create duplicate rows"
            );
        });
    }

    #[test]
    fn hotkey_canonicalization_update_path_works() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute(
                "ctrl+shift+g".to_string(),
                "one".to_string(),
                "win".to_string(),
                true,
            )
            .unwrap();

            // Should canonicalize to 'ctrl+shift+g' and update
            execute(
                "Shift + Ctrl + G".to_string(),
                "two".to_string(),
                "win".to_string(),
                true,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let mut stmt = conn.prepare("SELECT output FROM automations WHERE trigger_type = 'hotkey' AND trigger = 'ctrl+shift+g' AND is_deleted = 0").unwrap();
            let mut rows = stmt.query([]).unwrap();

            let row = rows.next().unwrap().unwrap();
            assert_eq!(row.get::<_, String>(0).unwrap(), "two");
            assert!(
                rows.next().unwrap().is_none(),
                "Should not create duplicate rows"
            );
        });
    }

    #[test]
    fn same_trigger_text_is_allowed_across_word_and_hotkey_types() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute(
                "tab".to_string(),
                "word".to_string(),
                "all".to_string(),
                false,
            )
            .unwrap();
            execute(
                "tab".to_string(),
                "hotkey".to_string(),
                "all".to_string(),
                true,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM automations WHERE trigger = 'tab' AND is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 2);
        });
    }
}
