use super::script::*;
use std::path::Path;
use std::path::PathBuf;
use taurine_core::db::crud::TriggerType;
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

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
        std::env::temp_dir().join(format!("taurine-cli-script-{}.db", uuid::Uuid::new_v4())),
    );
    let db_path = db_guard.db_path();
    f(&db_path)
}

#[test]
fn test_inference_by_extension() {
    assert_eq!(
        infer_interpreter(Some(Path::new("test.sh")), ""),
        Some(ScriptInterpreter::Bash)
    );
    assert_eq!(
        infer_interpreter(Some(Path::new("test.ps1")), ""),
        Some(ScriptInterpreter::PowerShell)
    );
    assert_eq!(
        infer_interpreter(Some(Path::new("test.py")), ""),
        Some(ScriptInterpreter::Python)
    );
    assert_eq!(
        infer_interpreter(Some(Path::new("test.js")), ""),
        Some(ScriptInterpreter::Node)
    );
    assert_eq!(
        infer_interpreter(Some(Path::new("test.cjs")), ""),
        Some(ScriptInterpreter::Node)
    );
    assert_eq!(infer_interpreter(Some(Path::new("test.mjs")), ""), None);
    assert_eq!(
        infer_interpreter(Some(Path::new("test.bat")), ""),
        Some(ScriptInterpreter::Cmd)
    );
    assert_eq!(
        infer_interpreter(Some(Path::new("test.cmd")), ""),
        Some(ScriptInterpreter::Cmd)
    );
}

#[test]
fn test_inference_by_shebang() {
    assert_eq!(
        infer_interpreter(None, "#!/bin/bash\necho hello"),
        Some(ScriptInterpreter::Bash)
    );
    assert_eq!(
        infer_interpreter(None, "#!/usr/bin/env python3\nprint(1)"),
        Some(ScriptInterpreter::Python)
    );
    assert_eq!(
        infer_interpreter(None, "#!/usr/bin/env node\nconsole.log(1)"),
        Some(ScriptInterpreter::Node)
    );
    assert_eq!(
        infer_interpreter(None, "#!/usr/bin/env node\nimport fs from 'fs'"),
        Some(ScriptInterpreter::Node)
    );
    assert_eq!(
        infer_interpreter(None, "#!/bin/sh\nls"),
        Some(ScriptInterpreter::Bash)
    );
}

#[test]
fn test_inference_extension_over_shebang() {
    // Extension should be checked first or at least be high priority
    assert_eq!(
        infer_interpreter(Some(Path::new("test.py")), "#!/bin/bash"),
        Some(ScriptInterpreter::Python)
    );
}

#[test]
fn test_inference_fallback() {
    assert_eq!(infer_interpreter(None, "just some text"), None);
    assert_eq!(
        infer_interpreter(Some(Path::new("test.unknown")), "no shebang"),
        None
    );
}

#[test]
fn script_auto_case_does_not_lowercase_regex() {
    init_tracing_for_tests();

    with_test_db(|db_path| {
        execute_with_trigger_type(
            "['A-Z']".to_string(),
            TriggerType::Regex,
            Some("echo hi".to_string()),
            None,
            Some(ScriptInterpreter::Bash),
            ScriptBehavior::Inline,
            "all".to_string(),
            None,
            None,
            None,
            None,
            None,
            true,
            false,
        )
        .unwrap();

        let conn = rusqlite::Connection::open(db_path).unwrap();
        let stored: String = conn
            .query_row(
                "SELECT trigger FROM triggers WHERE action_type = 'script'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            stored, "['A-Z']",
            "auto_case must not lowercase regex trigger"
        );
    });
}

#[test]
fn script_add_still_creates_word_trigger_by_default() {
    init_tracing_for_tests();

    with_test_db(|db_path| {
        execute(
            "deploy".to_string(),
            false,
            Some("echo hi".to_string()),
            None,
            Some(ScriptInterpreter::Bash),
            ScriptBehavior::Inline,
            "linux".to_string(),
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
        assert_eq!(stored.1, "deploy");
    });
}

#[test]
fn script_add_hotkey_creates_canonical_hotkey_trigger() {
    init_tracing_for_tests();

    with_test_db(|db_path| {
        execute(
            "Control + Shift + W".to_string(),
            true,
            Some("winget install [0=package]".to_string()),
            None,
            Some(ScriptInterpreter::PowerShell),
            ScriptBehavior::Inline,
            "win".to_string(),
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
        assert_eq!(stored.1, "ctrl+shift+w");
    });
}

#[test]
fn script_word_trigger_duplicate_updates_existing_row() {
    init_tracing_for_tests();

    with_test_db(|db_path| {
        // First add
        execute(
            "deploy".to_string(),
            false,
            Some("echo first".to_string()),
            None,
            Some(ScriptInterpreter::Bash),
            ScriptBehavior::Inline,
            "linux".to_string(),
            None,
            None,
        )
        .unwrap();

        // Second add with same trigger identity, new script content
        execute(
            "deploy".to_string(),
            false,
            Some("echo second".to_string()),
            None,
            Some(ScriptInterpreter::Bash),
            ScriptBehavior::Inline,
            "linux".to_string(),
            None,
            None,
        )
        .unwrap();

        let conn = rusqlite::Connection::open(db_path).unwrap();

        // Exactly one active row
        let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM triggers WHERE trigger_type = 'word' AND trigger = 'deploy' AND is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(count, 1, "Should have exactly one trigger row");

        // The script attachment also has exactly one row
        let script_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM scripts WHERE trigger_id = (SELECT id FROM triggers WHERE trigger = 'deploy' AND is_deleted = 0 LIMIT 1)",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(script_count, 1, "Should have exactly one script row");
    });
}

#[test]
fn script_hotkey_trigger_canonicalization_updates_existing_row() {
    init_tracing_for_tests();

    with_test_db(|db_path| {
        // First add with lowercase canonical form
        execute(
            "ctrl+shift+g".to_string(),
            true,
            Some("echo first".to_string()),
            None,
            Some(ScriptInterpreter::Bash),
            ScriptBehavior::Inline,
            "linux".to_string(),
            None,
            None,
        )
        .unwrap();

        // Second add with mixed-case non-canonical form (should normalize to same key)
        execute(
            "Shift + Ctrl + G".to_string(),
            true,
            Some("echo second".to_string()),
            None,
            Some(ScriptInterpreter::Bash),
            ScriptBehavior::Inline,
            "linux".to_string(),
            None,
            None,
        )
        .unwrap();

        let conn = rusqlite::Connection::open(db_path).unwrap();

        // Exactly one active row for the canonical hotkey
        let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM triggers WHERE trigger_type = 'hotkey' AND trigger = 'ctrl+shift+g' AND is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(
            count, 1,
            "Should have exactly one trigger row after canonicalized update"
        );
    });
}

#[test]
fn script_to_text_update_clears_stale_script_row() {
    init_tracing_for_tests();

    with_test_db(|db_path| {
        // Step 1: create a script trigger
        execute(
            "gs".to_string(),
            false,
            Some("echo git status".to_string()),
            None,
            Some(ScriptInterpreter::Bash),
            ScriptBehavior::Inline,
            "all".to_string(),
            None,
            None,
        )
        .unwrap();

        // Confirm the script row exists
        let conn = rusqlite::Connection::open(db_path).unwrap();
        let script_count_before: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM scripts WHERE trigger_id = (SELECT id FROM triggers WHERE trigger = 'gs' AND is_deleted = 0 LIMIT 1)",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(
            script_count_before, 1,
            "Script row should exist after script add"
        );
        drop(conn);

        // Step 2: switch to a plain text trigger for the same trigger identity
        crate::commands::add::execute(
            "gs".to_string(),
            "git status".to_string(),
            "all".to_string(),
            false,
            None,
            None,
        )
        .unwrap();

        let conn = rusqlite::Connection::open(db_path).unwrap();

        // Only one active trigger row
        let auto_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM triggers WHERE trigger_type = 'word' AND trigger = 'gs' AND is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(auto_count, 1, "Should still have exactly one trigger row");

        // action_type must now be 'text'
        let action_type: String = conn
            .query_row(
                "SELECT action_type FROM triggers WHERE trigger = 'gs' AND is_deleted = 0 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            action_type, "text",
            "action_type should be 'text' after text update"
        );

        // Stale script row must be gone
        let script_count_after: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM scripts WHERE trigger_id = (SELECT id FROM triggers WHERE trigger = 'gs' AND is_deleted = 0 LIMIT 1)",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(
            script_count_after, 0,
            "Stale script row should be deleted after text update"
        );
    });
}

#[test]
fn text_to_script_update_creates_script_attachment() {
    init_tracing_for_tests();

    with_test_db(|db_path| {
        // Step 1: create a plain text trigger
        crate::commands::add::execute(
            "gs".to_string(),
            "git status".to_string(),
            "all".to_string(),
            false,
            None,
            None,
        )
        .unwrap();

        // Confirm no script row yet
        let conn = rusqlite::Connection::open(db_path).unwrap();
        let script_count_before: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM scripts WHERE trigger_id = (SELECT id FROM triggers WHERE trigger = 'gs' AND is_deleted = 0 LIMIT 1)",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(
            script_count_before, 0,
            "No script row should exist for a text trigger"
        );
        drop(conn);

        // Step 2: switch to a script trigger for the same trigger identity
        execute(
            "gs".to_string(),
            false,
            Some("echo git status".to_string()),
            None,
            Some(ScriptInterpreter::Bash),
            ScriptBehavior::Inline,
            "all".to_string(),
            None,
            None,
        )
        .unwrap();

        let conn = rusqlite::Connection::open(db_path).unwrap();

        // Only one active trigger row
        let auto_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM triggers WHERE trigger_type = 'word' AND trigger = 'gs' AND is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(auto_count, 1, "Should still have exactly one trigger row");

        // action_type must now be 'script'
        let action_type: String = conn
            .query_row(
                "SELECT action_type FROM triggers WHERE trigger = 'gs' AND is_deleted = 0 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            action_type, "script",
            "action_type should be 'script' after script update"
        );

        // Script attachment must exist
        let script_count_after: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM scripts WHERE trigger_id = (SELECT id FROM triggers WHERE trigger = 'gs' AND is_deleted = 0 LIMIT 1)",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(
            script_count_after, 1,
            "Script row should be created after script update"
        );
    });
}
