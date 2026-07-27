use taurine_core::db::crud::{AddOutcome, TriggerType};
use taurine_core::db::init;
use tracing::info;

pub fn execute(
    trigger: String,
    output: String,
    os: String,
    use_hotkey: bool,
    include_apps: Option<String>,
    exclude_apps: Option<String>,
) -> taurine_core::error::Result<()> {
    execute_with_trigger_type(
        trigger,
        output,
        os,
        if use_hotkey {
            TriggerType::Hotkey
        } else {
            TriggerType::Word
        },
        include_apps,
        exclude_apps,
        None,
        None,
        None,
        false,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn execute_with_trigger_type(
    trigger: String,
    output: String,
    os: String,
    trigger_type: TriggerType,
    include_apps: Option<String>,
    exclude_apps: Option<String>,
    tags: Option<Vec<String>>,
    name: Option<String>,
    description: Option<String>,
    auto_case: bool,
    json: bool,
) -> taurine_core::error::Result<()> {
    use crate::commands::validate::format_trigger_log;
    use taurine_core::db::crud::{
        add_trigger_by_type_with_case, add_trigger_with_case, audit_payload_tags_with_trigger_type,
        prepare_trigger_with_type,
    };
    use taurine_core::engine::variables::system::validate_output;

    let trigger = if auto_case {
        trigger.to_lowercase()
    } else {
        trigger
    };

    audit_payload_tags_with_trigger_type(&output, trigger_type)?;

    if matches!(trigger_type, TriggerType::Regex) {
        regex::Regex::new(&trigger)
            .map_err(|e| taurine_core::Error::Config(format!("Invalid regular expression: {e}")))?;
    }

    let prepared = prepare_trigger_with_type(&trigger, trigger_type, &os)?;
    let stored_trigger = prepared.stored_trigger;

    validate_output(&output, Some(&stored_trigger))?;

    let conn = init::setup()?;
    let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
    if !settings.clipboard_history_enabled && output.contains("[clip") {
        tracing::warn!(
            "Warning: The trigger contains '[clip]' system variables, which won't work because clipboard history is disabled in the settings."
        );
    }
    // Case conflict check
    if auto_case {
        let conflict_exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM triggers
                    WHERE trigger_type = ?1
                      AND LOWER(trigger) = LOWER(?2)
                      AND target_os = ?3
                      AND COALESCE(only_apps, '') = ?4
                      AND COALESCE(except_apps, '') = ?5
                      AND is_deleted = 0
                 )",
                [
                    trigger_type.as_db_str(),
                    &stored_trigger,
                    &os,
                    include_apps.as_deref().unwrap_or(""),
                    exclude_apps.as_deref().unwrap_or(""),
                ],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if conflict_exists {
            return Err(taurine_core::Error::Config(format!(
                "Trigger conflict: A trigger matching '{}' case-insensitively already exists.",
                stored_trigger
            )));
        }
    } else {
        let conflict_exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM triggers
                    WHERE trigger_type = ?1
                      AND LOWER(trigger) = LOWER(?2)
                      AND target_os = ?3
                      AND COALESCE(only_apps, '') = ?4
                      AND COALESCE(except_apps, '') = ?5
                      AND auto_case = 1
                      AND is_deleted = 0
                 )",
                [
                    trigger_type.as_db_str(),
                    &stored_trigger,
                    &os,
                    include_apps.as_deref().unwrap_or(""),
                    exclude_apps.as_deref().unwrap_or(""),
                ],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if conflict_exists {
            return Err(taurine_core::Error::Config(format!(
                "Trigger conflict: A case-propagating trigger matching '{}' case-insensitively already exists.",
                stored_trigger
            )));
        }
    }

    let outcome = match prepared.trigger_type {
        TriggerType::Word => add_trigger_with_case(
            &conn,
            &stored_trigger,
            &output,
            &os,
            include_apps.as_deref(),
            exclude_apps.as_deref(),
            tags,
            name.as_deref(),
            description.as_deref(),
            auto_case,
        )?,
        TriggerType::Hotkey => add_trigger_by_type_with_case(
            &conn,
            TriggerType::Hotkey,
            &stored_trigger,
            &output,
            &os,
            include_apps.as_deref(),
            exclude_apps.as_deref(),
            tags,
            name.as_deref(),
            description.as_deref(),
            auto_case,
        )?,
        TriggerType::Regex => add_trigger_by_type_with_case(
            &conn,
            TriggerType::Regex,
            &stored_trigger,
            &output,
            &os,
            include_apps.as_deref(),
            exclude_apps.as_deref(),
            tags,
            name.as_deref(),
            description.as_deref(),
            auto_case,
        )?,
    };

    match outcome {
        AddOutcome::Created => {
            let log_msg = format_trigger_log(
                "Added",
                &stored_trigger,
                None,
                &os,
                include_apps.as_deref(),
                exclude_apps.as_deref(),
            );
            info!("{}", log_msg);
            taurine_core::rpc::notify_daemon_reload();
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "created", "trigger": stored_trigger})
                );
            }
        }
        AddOutcome::AlreadyExists => {
            let log_msg = format_trigger_log(
                "Trigger already exists for",
                &stored_trigger,
                None,
                &os,
                include_apps.as_deref(),
                exclude_apps.as_deref(),
            );
            info!("{}", log_msg);
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "exists", "trigger": stored_trigger})
                );
            }
        }
        AddOutcome::Updated => {
            let log_msg = format_trigger_log(
                "Updated",
                &stored_trigger,
                None,
                &os,
                include_apps.as_deref(),
                exclude_apps.as_deref(),
            );
            info!("{}", log_msg);
            taurine_core::rpc::notify_daemon_reload();
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "updated", "trigger": stored_trigger})
                );
            }
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
                None,
                None,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let stored: (String, String) = conn
                .query_row(
                    "SELECT trigger_type, trigger FROM triggers WHERE is_deleted = 0 LIMIT 1",
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
                None,
                None,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let stored: (String, String) = conn
                .query_row(
                    "SELECT trigger_type, trigger FROM triggers WHERE is_deleted = 0 LIMIT 1",
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
                None,
                None,
            )
            .unwrap();

            let error = execute(
                "ctrl+shift+g".to_string(),
                "two".to_string(),
                "win".to_string(),
                true,
                None,
                None,
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
                None,
                None,
            )
            .unwrap();
            execute(
                "ctrl+shift+g".to_string(),
                "linux".to_string(),
                "linux".to_string(),
                true,
                None,
                None,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM triggers WHERE trigger_type = 'hotkey' AND trigger = 'ctrl+shift+g' AND is_deleted = 0",
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
                None,
                None,
            )
            .unwrap();

            let error = execute(
                "ralt+m".to_string(),
                "two".to_string(),
                "win".to_string(),
                true,
                None,
                None,
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
                None,
                None,
            )
            .unwrap();
            execute(
                "ralt+m".to_string(),
                "right".to_string(),
                "all".to_string(),
                true,
                None,
                None,
            )
            .unwrap();

            let count: i64 = rusqlite::Connection::open(db_path)
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM triggers WHERE trigger_type = 'hotkey' AND is_deleted = 0",
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
                None,
                None,
            )
            .unwrap();

            // Adding same trigger + target_os but different output should update
            execute(
                "ctrl+shift+g".to_string(),
                "two".to_string(),
                "win".to_string(),
                true,
                None,
                None,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let mut stmt = conn.prepare("SELECT output FROM triggers WHERE trigger_type = 'hotkey' AND trigger = 'ctrl+shift+g' AND is_deleted = 0").unwrap();
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
                None,
                None,
            )
            .unwrap();

            execute(
                "gs".to_string(),
                "two".to_string(),
                "all".to_string(),
                false,
                None,
                None,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let mut stmt = conn.prepare("SELECT output FROM triggers WHERE trigger_type = 'word' AND trigger = 'gs' AND is_deleted = 0").unwrap();
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
                None,
                None,
            )
            .unwrap();

            // Should canonicalize to 'ctrl+shift+g' and update
            execute(
                "Shift + Ctrl + G".to_string(),
                "two".to_string(),
                "win".to_string(),
                true,
                None,
                None,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let mut stmt = conn.prepare("SELECT output FROM triggers WHERE trigger_type = 'hotkey' AND trigger = 'ctrl+shift+g' AND is_deleted = 0").unwrap();
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
                None,
                None,
            )
            .unwrap();
            execute(
                "tab".to_string(),
                "hotkey".to_string(),
                "all".to_string(),
                true,
                None,
                None,
            )
            .unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM triggers WHERE trigger = 'tab' AND is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 2);
        });
    }
}
