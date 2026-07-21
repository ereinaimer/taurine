use std::io::Write;
use taurine_core::db::crud::{
    count_automations_by_pattern, delete_automations_by_pattern, delete_automations_by_tag,
    delete_automations_by_triggers,
};
use taurine_core::db::init;
use taurine_core::keys::normalize_hotkey;
use tracing::{info, warn};

pub fn execute(
    triggers: Vec<String>,
    tag: Option<String>,
    yes: bool,
) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let is_glob = tag.is_none() && triggers.iter().any(|t| t.contains('*'));

    let removed_count = if let Some(ref t) = tag {
        delete_automations_by_tag(&conn, t)?
    } else if is_glob {
        let mut total = 0;
        for pattern in &triggers {
            let matched = count_automations_by_pattern(&conn, pattern)?;
            if matched == 0 {
                warn!("No active automation matching pattern: {}", pattern);
                continue;
            }
            if matched > 1 && !yes {
                eprint!("This operation will remove {matched} automations. Continue? [y/N] ");
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().to_lowercase() != "y" {
                    info!("Operation cancelled");
                    continue;
                }
            }
            let deleted = delete_automations_by_pattern(&conn, pattern)?;
            info!(
                "Removed {deleted} automations matching pattern: {}",
                pattern
            );
            total += deleted;
        }
        total
    } else {
        let canonical: Vec<String> = triggers
            .iter()
            .map(|t| normalize_hotkey(t).unwrap_or_else(|_| t.clone()))
            .collect();
        delete_automations_by_triggers(&conn, &canonical)?
    };

    if removed_count == 0 {
        if let Some(ref t) = tag {
            warn!("No active automation found with tag: {}", t);
        } else if !is_glob {
            let triggers_str = triggers.join(", ");
            warn!("No active automation found for triggers: {}", triggers_str);
        }
    } else {
        if let Some(ref t) = tag {
            info!("Removed {} automations with tag: {}", removed_count, t);
        } else if !is_glob {
            let triggers_str = triggers.join(", ");
            info!(
                "Removed {} automations for triggers: {}",
                removed_count, triggers_str
            );
        }
        taurine_core::rpc::notify_daemon_reload();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use taurine_core::db::crud::TriggerType;
    use taurine_core::db::crud::upsert_automation_with_trigger_type;
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
            std::env::temp_dir().join(format!("taurine-cli-delete-{}.db", uuid::Uuid::new_v4())),
        );
        let db_path = db_guard.db_path();
        f(&db_path)
    }

    #[test]
    fn delete_hotkey_with_non_canonical_order_still_matches() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let conn = rusqlite::Connection::open(db_path).unwrap();
            taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

            upsert_automation_with_trigger_type(
                &conn,
                "test-uuid-1",
                "test",
                None,
                TriggerType::Hotkey,
                "shift+alt+2",
                "echo hello",
                "text",
                "all",
                "[]",
                0,
                None,
            )
            .unwrap();

            drop(conn);

            execute(vec!["alt+shift+2".to_string()], None, false).unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let is_deleted: bool = conn
                .query_row(
                    "SELECT is_deleted FROM automations WHERE id = 'test-uuid-1'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(is_deleted, "automation should be tombstoned");
        });
    }

    #[test]
    fn delete_text_trigger_is_unchanged_by_normalize_hotkey() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let conn = rusqlite::Connection::open(db_path).unwrap();
            taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

            upsert_automation_with_trigger_type(
                &conn,
                "test-uuid-2",
                "test",
                None,
                TriggerType::Word,
                "gs",
                "echo hello",
                "text",
                "all",
                "[]",
                0,
                None,
            )
            .unwrap();

            drop(conn);

            execute(vec!["gs".to_string()], None, false).unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let is_deleted: bool = conn
                .query_row(
                    "SELECT is_deleted FROM automations WHERE id = 'test-uuid-2'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                is_deleted,
                "text trigger automation should still be deleted"
            );
        });
    }

    #[test]
    fn delete_with_glob_star_only_pattern_deletes_matching() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new(std::env::temp_dir().join(format!(
            "taurine-cli-delete-glob-{}.db",
            uuid::Uuid::new_v4()
        )));
        let db_path = db_guard.db_path();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

        taurine_core::db::crud::upsert_automation_with_trigger_type(
            &conn,
            "uuid-1",
            "test",
            None,
            taurine_core::db::crud::TriggerType::Word,
            "test_foo",
            "echo 1",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        taurine_core::db::crud::upsert_automation_with_trigger_type(
            &conn,
            "uuid-2",
            "test",
            None,
            taurine_core::db::crud::TriggerType::Word,
            "test_bar",
            "echo 2",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        taurine_core::db::crud::upsert_automation_with_trigger_type(
            &conn,
            "uuid-3",
            "other",
            None,
            taurine_core::db::crud::TriggerType::Word,
            "other",
            "echo 3",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        drop(conn);

        // yes=true skips the prompt — only way to test non-interactively
        execute(vec!["test_*".to_string()], None, true).unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        assert!(
            taurine_core::db::crud::get_automation(&conn, "uuid-1")
                .unwrap()
                .unwrap()
                .is_deleted
        );
        assert!(
            taurine_core::db::crud::get_automation(&conn, "uuid-2")
                .unwrap()
                .unwrap()
                .is_deleted
        );
        assert!(
            !taurine_core::db::crud::get_automation(&conn, "uuid-3")
                .unwrap()
                .unwrap()
                .is_deleted
        );
    }

    #[test]
    fn delete_with_exact_trigger_unchanged_by_glob_flag() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new(std::env::temp_dir().join(format!(
            "taurine-cli-delete-exact-{}.db",
            uuid::Uuid::new_v4()
        )));
        let db_path = db_guard.db_path();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

        taurine_core::db::crud::upsert_automation_with_trigger_type(
            &conn,
            "uuid-1",
            "test",
            None,
            taurine_core::db::crud::TriggerType::Word,
            "gs",
            "echo 1",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        drop(conn);

        execute(vec!["gs".to_string()], None, true).unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        assert!(
            taurine_core::db::crud::get_automation(&conn, "uuid-1")
                .unwrap()
                .unwrap()
                .is_deleted
        );
    }

    #[test]
    fn delete_with_glob_star_only_matches_all() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new(std::env::temp_dir().join(format!(
            "taurine-cli-delete-star-{}.db",
            uuid::Uuid::new_v4()
        )));
        let db_path = db_guard.db_path();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

        taurine_core::db::crud::upsert_automation_with_trigger_type(
            &conn,
            "uuid-1",
            "A",
            None,
            taurine_core::db::crud::TriggerType::Word,
            "a",
            "out",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        taurine_core::db::crud::upsert_automation_with_trigger_type(
            &conn,
            "uuid-2",
            "B",
            None,
            taurine_core::db::crud::TriggerType::Word,
            "b",
            "out",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();
        drop(conn);

        // yes=true to skip prompt
        execute(vec!["*".to_string()], None, true).unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        assert!(
            taurine_core::db::crud::get_automation(&conn, "uuid-1")
                .unwrap()
                .unwrap()
                .is_deleted
        );
        assert!(
            taurine_core::db::crud::get_automation(&conn, "uuid-2")
                .unwrap()
                .unwrap()
                .is_deleted
        );
    }

    #[test]
    fn delete_with_glob_no_match_warns_cleanly() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new(std::env::temp_dir().join(format!(
            "taurine-cli-delete-nomatch-{}.db",
            uuid::Uuid::new_v4()
        )));
        let db_path = db_guard.db_path();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();
        drop(conn);

        // Should not error, just warn
        execute(vec!["nomatch_*".to_string()], None, true).unwrap();
    }
}
