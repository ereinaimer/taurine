use std::io::Write;
use taurine_core::db::crud::{
    count_triggers_by_pattern, delete_triggers_by_pattern, delete_triggers_by_tag,
    delete_triggers_by_values,
};
use taurine_core::db::init;
use taurine_core::keys::normalize_hotkey;
use tracing::{info, warn};

pub fn execute(
    triggers: Vec<String>,
    tag: Option<String>,
    yes: bool,
    json: bool,
) -> taurine_core::error::Result<()> {
    if triggers.is_empty() && tag.is_none() {
        let diag =
            taurine_core::diagnostic::Diagnostic::problem("Missing trigger or tag to delete")
                .help("Specify one or more triggers or use --tag to delete triggers by tag:")
                .example("taurine delete :brb")
                .example("taurine delete --tag snippet");
        return Err(taurine_core::Error::Config(diag.render()));
    }

    let conn = init::setup()?;
    let is_glob = tag.is_none() && triggers.iter().any(|t| t.contains('*'));

    let removed_count = if let Some(ref t) = tag {
        delete_triggers_by_tag(&conn, t)?
    } else if is_glob {
        let mut total = 0;
        for pattern in &triggers {
            let matched = count_triggers_by_pattern(&conn, pattern)?;
            if matched == 0 {
                warn!("No active trigger matching pattern: {}", pattern);
                continue;
            }
            if matched > 1 && !yes {
                eprint!("This operation will remove {matched} triggers. Continue? [y/N] ");
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().to_lowercase() != "y" {
                    info!("Operation cancelled");
                    continue;
                }
            }
            let deleted = delete_triggers_by_pattern(&conn, pattern)?;
            info!("Removed {deleted} triggers matching pattern: {}", pattern);
            total += deleted;
        }
        total
    } else {
        let canonical: Vec<String> = triggers
            .iter()
            .map(|t| normalize_hotkey(t).unwrap_or_else(|_| t.clone()))
            .collect();
        delete_triggers_by_values(&conn, &canonical)?
    };

    if removed_count == 0 {
        if let Some(ref t) = tag {
            let mut stmt = conn.prepare("SELECT DISTINCT tag FROM trigger_tags")?;
            let existing_tags: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            let tag_refs: Vec<&str> = existing_tags.iter().map(|s| s.as_str()).collect();

            let diag = taurine_core::diagnostic::Diagnostic::problem(format!(
                "No active trigger found with tag {t}"
            ))
            .suggest(t, &tag_refs)
            .help("To view all active triggers and their tags, run: taurine list")
            .example("taurine list");
            warn!("{}", diag.render());

            if json {
                println!("{}", serde_json::json!({"status": "not_found", "tag": t}));
            }
        } else if !is_glob {
            let mut stmt = conn.prepare("SELECT trigger FROM triggers WHERE is_deleted = 0")?;
            let active_triggers: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            let active_refs: Vec<&str> = active_triggers.iter().map(|s| s.as_str()).collect();

            for trig in &triggers {
                let diag = taurine_core::diagnostic::Diagnostic::problem(format!(
                    "No active trigger matches {trig}"
                ))
                .suggest(trig, &active_refs)
                .help("To view all active triggers, run: taurine list")
                .example("taurine list");
                warn!("{}", diag.render());
            }

            if json {
                let triggers_str = triggers.join(", ");
                println!(
                    "{}",
                    serde_json::json!({"status": "not_found", "triggers": triggers_str})
                );
            }
        } else if json {
            println!("{}", serde_json::json!({"status": "deleted", "count": 0}));
        }
    } else {
        if let Some(ref t) = tag {
            info!("Removed {} triggers with tag: {}", removed_count, t);
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "deleted", "count": removed_count, "tag": t})
                );
            }
        } else if !is_glob {
            let triggers_str = triggers.join(", ");
            info!(
                "Removed {} triggers for triggers: {}",
                removed_count, triggers_str
            );
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "deleted", "count": removed_count, "triggers": triggers_str})
                );
            }
        } else if json {
            println!(
                "{}",
                serde_json::json!({"status": "deleted", "count": removed_count})
            );
        }
        taurine_core::rpc::notify_daemon_reload();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::db::crud::TriggerType;
    use taurine_core::db::crud::upsert_trigger_with_type;
    use taurine_core::logs::init_tracing_for_tests;

    struct TestDbEnvGuard {
        _dir: tempfile::TempDir,
    }

    impl TestDbEnvGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            unsafe { std::env::set_var("TAURINE_DATA_DIR", dir.path()) };
            Self { _dir: dir }
        }

        fn db_path(&self) -> String {
            self._dir
                .path()
                .join("taurine.db")
                .to_string_lossy()
                .to_string()
        }
    }

    impl Drop for TestDbEnvGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
        }
    }

    fn with_test_db<T>(f: impl FnOnce(&str) -> T) -> T {
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();
        f(&db_path)
    }

    #[test]
    fn delete_hotkey_with_non_canonical_order_still_matches() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let conn = rusqlite::Connection::open(db_path).unwrap();
            taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

            upsert_trigger_with_type(
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

            execute(vec!["alt+shift+2".to_string()], None, false, false).unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let is_deleted: bool = conn
                .query_row(
                    "SELECT is_deleted FROM triggers WHERE id = 'test-uuid-1'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(is_deleted, "trigger should be tombstoned");
        });
    }

    #[test]
    fn delete_text_trigger_is_unchanged_by_normalize_hotkey() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let conn = rusqlite::Connection::open(db_path).unwrap();
            taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

            upsert_trigger_with_type(
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

            execute(vec!["gs".to_string()], None, false, false).unwrap();

            let conn = rusqlite::Connection::open(db_path).unwrap();
            let is_deleted: bool = conn
                .query_row(
                    "SELECT is_deleted FROM triggers WHERE id = 'test-uuid-2'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(is_deleted, "text trigger should still be deleted");
        });
    }

    #[test]
    fn delete_with_glob_star_only_pattern_deletes_matching() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

        taurine_core::db::crud::upsert_trigger_with_type(
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
        taurine_core::db::crud::upsert_trigger_with_type(
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
        taurine_core::db::crud::upsert_trigger_with_type(
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
        execute(vec!["test_*".to_string()], None, true, false).unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        assert!(
            taurine_core::db::crud::get_trigger(&conn, "uuid-1")
                .unwrap()
                .unwrap()
                .is_deleted
        );
        assert!(
            taurine_core::db::crud::get_trigger(&conn, "uuid-2")
                .unwrap()
                .unwrap()
                .is_deleted
        );
        assert!(
            !taurine_core::db::crud::get_trigger(&conn, "uuid-3")
                .unwrap()
                .unwrap()
                .is_deleted
        );
    }

    #[test]
    fn delete_with_exact_trigger_unchanged_by_glob_flag() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

        taurine_core::db::crud::upsert_trigger_with_type(
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

        execute(vec!["gs".to_string()], None, true, false).unwrap();

        let conn = rusqlite::Connection::open(db_path).unwrap();
        assert!(
            taurine_core::db::crud::get_trigger(&conn, "uuid-1")
                .unwrap()
                .unwrap()
                .is_deleted
        );
    }

    #[test]
    fn delete_with_glob_star_only_matches_all() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

        taurine_core::db::crud::upsert_trigger_with_type(
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
        taurine_core::db::crud::upsert_trigger_with_type(
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
        execute(vec!["*".to_string()], None, true, false).unwrap();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        assert!(
            taurine_core::db::crud::get_trigger(&conn, "uuid-1")
                .unwrap()
                .unwrap()
                .is_deleted
        );
        assert!(
            taurine_core::db::crud::get_trigger(&conn, "uuid-2")
                .unwrap()
                .unwrap()
                .is_deleted
        );
    }

    #[test]
    fn delete_with_glob_no_match_warns_cleanly() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();
        drop(conn);

        // Should not error, just warn
        execute(vec!["nomatch_*".to_string()], None, true, false).unwrap();
    }

    #[test]
    fn test_delete_missing_args_diagnostic() {
        let result = execute(vec![], None, false, false);
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Missing trigger or tag to delete"),
            "Error was: {err}"
        );
        assert!(
            err.contains("Specify one or more triggers or use --tag to delete triggers by tag:"),
            "Error was: {err}"
        );
        assert!(err.contains("taurine delete :brb"), "Error was: {err}");
        assert!(!err.contains('`'), "Must not contain backticks: {err}");
        assert!(!err.contains('\''), "Must not contain single quotes: {err}");
    }
}
