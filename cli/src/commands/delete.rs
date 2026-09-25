use std::io::Write;
use taurine_core::db::crud::{
    InvocationType, count_aliases, count_triggers_by_pattern, delete_triggers_by_pattern,
    delete_triggers_by_tag, delete_triggers_by_values, find_parent_by_invocation, get_trigger,
    list_active_voice_invocations,
};
use taurine_core::db::init;
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

    // Parent ids holding a requested value before deletion, for the
    // remaining-alias report below.
    let mut affected: Vec<String> = Vec::new();
    if !is_glob && tag.is_none() {
        for value in &triggers {
            for invocation_type in [
                InvocationType::Word,
                InvocationType::Hotkey,
                InvocationType::Regex,
                InvocationType::Voice,
            ] {
                if let Some(pid) = find_parent_by_invocation(&conn, invocation_type, value)?
                    && !affected.contains(&pid)
                {
                    affected.push(pid);
                }
            }
        }
    }

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
        // One alias row per value; a parent whose last alias goes is
        // tombstoned inside the delete path.
        delete_triggers_by_values(&conn, &triggers)?
    };

    // Aliases still live on affected entries after a value delete.
    let mut remaining = 0;
    if !is_glob && tag.is_none() {
        for pid in &affected {
            if let Some(row) = get_trigger(&conn, pid)?
                && !row.is_deleted
            {
                remaining += count_aliases(&conn, pid)?;
            }
        }
    }

    if removed_count == 0 {
        if let Some(ref t) = tag {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT json_each.value FROM triggers, json_each(triggers.tags) WHERE triggers.is_deleted = 0",
            )?;
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
            let mut stmt = conn.prepare(
                "SELECT DISTINCT al.invocation FROM trigger_aliases al
                  JOIN triggers t ON t.id = al.trigger_id WHERE t.is_deleted = 0",
            )?;
            let mut active_triggers: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            if let Ok(voice_invs) = list_active_voice_invocations(&conn) {
                for inv in voice_invs {
                    if !active_triggers.contains(&inv.invocation) {
                        active_triggers.push(inv.invocation);
                    }
                }
            }
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
                "Removed {} triggers for triggers: {} ({} aliases remaining)",
                removed_count, triggers_str, remaining
            );
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "deleted", "count": removed_count, "triggers": triggers_str, "remaining": remaining})
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
        crate::commands::test_keyring::use_shared_test_keyring();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();
        f(&db_path)
    }

    fn open_keyed_db(db_path: &str) -> rusqlite::Connection {
        crate::commands::test_keyring::use_shared_test_keyring();
        taurine_core::db::key::open_keyed_connection(std::path::Path::new(db_path)).unwrap()
    }

    fn seed_word_entry(conn: &rusqlite::Connection, invocation: &str, output: &str) {
        seed_word_entry_with_tags(conn, invocation, output, "[]");
    }

    fn seed_word_entry_with_tags(
        conn: &rusqlite::Connection,
        invocation: &str,
        output: &str,
        tags_json: &str,
    ) {
        taurine_core::db::crud::create_entry(
            conn,
            taurine_core::db::crud::NewEntry {
                name: String::new(),
                description: None,
                content: output.to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                tags_json: tags_json.to_string(),
                auto_case: false,
                interpreter: None,
                behavior: None,
                invocations: vec![(InvocationType::Word, invocation.to_string(), false)],
            },
        )
        .unwrap();
    }
    fn assert_live(conn: &rusqlite::Connection, invocation: &str, live: bool) {
        let pid = find_parent_by_invocation(conn, InvocationType::Word, invocation).unwrap();
        match (pid, live) {
            (Some(id), true) => assert!(
                !get_trigger(conn, &id).unwrap().unwrap().is_deleted,
                "{invocation} should stay live"
            ),
            (None, false) => {}
            (Some(id), false) => panic!("{invocation} should be gone (parent {id})"),
            (None, true) => panic!("{invocation} should stay live"),
        }
    }

    #[test]
    fn delete_hotkey_with_non_canonical_order_still_matches() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let conn = open_keyed_db(db_path);
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

            let conn = open_keyed_db(db_path);
            let pid =
                find_parent_by_invocation(&conn, InvocationType::Hotkey, "alt+shift+2").unwrap();
            // The removed alias resolves nowhere; confirm via the sibling
            // canonical form that no live holder remains.
            assert!(pid.is_none(), "removed alias should resolve nowhere");
            let tombstoned: bool = conn
                .query_row(
                    "SELECT COUNT(*) = 0 FROM triggers WHERE is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(tombstoned, "trigger should be tombstoned");
        });
    }

    #[test]
    fn delete_text_trigger_is_unchanged_by_normalize_hotkey() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let conn = open_keyed_db(db_path);
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

            let conn = open_keyed_db(db_path);
            assert!(
                find_parent_by_invocation(&conn, InvocationType::Word, "gs")
                    .unwrap()
                    .is_none(),
                "removed alias should resolve nowhere"
            );
            let tombstoned: bool = conn
                .query_row(
                    "SELECT COUNT(*) = 0 FROM triggers WHERE is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(tombstoned, "text trigger should still be deleted");
        });
    }

    #[test]
    fn delete_one_alias_keeps_parent_with_remaining_count() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let conn = open_keyed_db(db_path);
            taurine_core::db::init::migrate::run_migrations(&conn).unwrap();
            taurine_core::db::crud::create_entry(
                &conn,
                taurine_core::db::crud::NewEntry {
                    name: String::new(),
                    description: None,
                    content: "Hello!".to_string(),
                    action_type: "text".to_string(),
                    target_os: "all".to_string(),
                    only_apps: None,
                    except_apps: None,
                    tags_json: "[]".to_string(),
                    auto_case: false,
                    interpreter: None,
                    behavior: None,
                    invocations: vec![
                        (InvocationType::Word, "hi".to_string(), false),
                        (InvocationType::Word, "hello".to_string(), false),
                    ],
                },
            )
            .unwrap();
            drop(conn);

            execute(vec!["hi".to_string()], None, false, false).unwrap();

            let conn = open_keyed_db(db_path);
            let pid = find_parent_by_invocation(&conn, InvocationType::Word, "hello")
                .unwrap()
                .expect("sibling alias keeps the parent alive");
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert!(!row.is_deleted);
            assert_eq!(count_aliases(&conn, &pid).unwrap(), 1);
            assert!(
                find_parent_by_invocation(&conn, InvocationType::Word, "hi")
                    .unwrap()
                    .is_none()
            );
        });
    }

    #[test]
    fn delete_with_glob_star_only_pattern_deletes_matching() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();

        let conn = open_keyed_db(&db_path);
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

        seed_word_entry(&conn, "test_foo", "echo 1");
        seed_word_entry(&conn, "test_bar", "echo 2");
        seed_word_entry(&conn, "other", "echo 3");
        drop(conn);

        // yes=true skips the prompt — only way to test non-interactively
        execute(vec!["test_*".to_string()], None, true, false).unwrap();

        let conn = open_keyed_db(&db_path);
        assert_live(&conn, "test_foo", false);
        assert_live(&conn, "test_bar", false);
        assert_live(&conn, "other", true);
    }

    #[test]
    fn delete_with_exact_trigger_unchanged_by_glob_flag() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();

        let conn = open_keyed_db(&db_path);
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

        seed_word_entry(&conn, "gs", "echo 1");
        drop(conn);

        execute(vec!["gs".to_string()], None, true, false).unwrap();

        let conn = open_keyed_db(&db_path);
        assert_live(&conn, "gs", false);
    }

    #[test]
    fn delete_with_glob_star_only_matches_all() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();

        let conn = open_keyed_db(&db_path);
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

        seed_word_entry(&conn, "a", "out");
        seed_word_entry(&conn, "b", "out");
        drop(conn);

        // yes=true to skip prompt
        execute(vec!["*".to_string()], None, true, false).unwrap();

        let conn = open_keyed_db(&db_path);
        assert_live(&conn, "a", false);
        assert_live(&conn, "b", false);
    }

    #[test]
    fn delete_with_glob_no_match_warns_cleanly() {
        init_tracing_for_tests();
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_guard = TestDbEnvGuard::new();
        let db_path = db_guard.db_path();

        let conn = open_keyed_db(&db_path);
        taurine_core::db::init::migrate::run_migrations(&conn).unwrap();
        drop(conn);

        // Should not error, just warn
        execute(vec!["nomatch_*".to_string()], None, true, false).unwrap();
    }

    #[test]
    fn delete_by_tag_removes_tagged_entries_only() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let conn = open_keyed_db(db_path);
            taurine_core::db::init::migrate::run_migrations(&conn).unwrap();

            seed_word_entry_with_tags(&conn, "tagged_one", "echo 1", r#"["work"]"#);
            seed_word_entry_with_tags(&conn, "tagged_two", "echo 2", r#"["work"]"#);
            seed_word_entry(&conn, "untagged", "echo 3");
            drop(conn);

            execute(vec![], Some("work".to_string()), true, false).unwrap();

            let conn = open_keyed_db(db_path);
            assert_live(&conn, "tagged_one", false);
            assert_live(&conn, "tagged_two", false);
            assert_live(&conn, "untagged", true);
        });
    }

    #[test]
    fn delete_by_missing_tag_warns_cleanly() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let conn = open_keyed_db(db_path);
            taurine_core::db::init::migrate::run_migrations(&conn).unwrap();
            seed_word_entry_with_tags(&conn, "tagged", "echo 1", r#"["work"]"#);
            drop(conn);

            // Should not error, just warn (suggestion lookup must not hit a missing table)
            execute(vec![], Some("nonexistent".to_string()), true, false).unwrap();

            let conn = open_keyed_db(db_path);
            assert_live(&conn, "tagged", true);
        });
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
