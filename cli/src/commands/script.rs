use crate::commands::validate::format_trigger_log;
use std::fs;
use std::path::PathBuf;
use taurine_core::db::crud::{
    TriggerType, audit_script_payload_tags, prepare_trigger_with_type, upsert_script,
    validate_trigger_not_reserved,
};
use taurine_core::db::init;
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter, compress};

#[allow(clippy::too_many_arguments)]
pub fn execute(
    trigger: String,
    use_hotkey: bool,
    content: Option<String>,
    file_path: Option<PathBuf>,
    lang: Option<ScriptInterpreter>,
    mode: ScriptBehavior,
    os: String,
    include_apps: Option<String>,
    exclude_apps: Option<String>,
) -> taurine_core::error::Result<()> {
    let trigger_type = if use_hotkey {
        TriggerType::Hotkey
    } else {
        TriggerType::Word
    };
    execute_with_trigger_type(
        trigger,
        trigger_type,
        content,
        file_path,
        lang,
        mode,
        os,
        include_apps,
        exclude_apps,
        None,
        false,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn execute_with_trigger_type(
    trigger: String,
    trigger_type: TriggerType,
    content: Option<String>,
    file_path: Option<PathBuf>,
    lang: Option<ScriptInterpreter>,
    mode: ScriptBehavior,
    os: String,
    include_apps: Option<String>,
    exclude_apps: Option<String>,
    tags: Option<Vec<String>>,
    auto_case: bool,
    json: bool,
) -> taurine_core::error::Result<()> {
    // 1. Resolve content and source description
    let (content, source_desc) = if let Some(ref path) = file_path {
        if !path.exists() {
            return Err(taurine_core::error::Error::NotFound(format!(
                "Script file not found: {}",
                path.display()
            )));
        }
        let text = fs::read_to_string(path).map_err(|e| {
            taurine_core::error::Error::Service(format!("Failed to read script file: {}", e))
        })?;
        (text, format!("File: {}", path.display()))
    } else if let Some(text) = content {
        (text, "CLI argument".to_string())
    } else {
        // unreachable due to clap constraints (required_unless_present)
        return Err(taurine_core::error::Error::Service(
            "Neither script file nor content provided".to_string(),
        ));
    };

    let trigger = if auto_case {
        trigger.to_lowercase()
    } else {
        trigger
    };

    audit_script_payload_tags(&content, trigger_type)?;

    if matches!(trigger_type, TriggerType::Regex) {
        regex::Regex::new(&trigger)
            .map_err(|e| taurine_core::Error::Config(format!("Invalid regular expression: {e}")))?;
    }

    let prepared = prepare_trigger_with_type(&trigger, trigger_type, &os)?;
    let stored_trigger = prepared.stored_trigger;

    // 2. Infer interpreter if not provided
    let lang = match lang {
        Some(i) => i,
        None => infer_interpreter(file_path.as_deref(), &content).ok_or_else(|| {
            taurine_core::error::Error::Service(
                "Could not infer script language. Please specify with --lang".to_string(),
            )
        })?,
    };

    let conn = init::setup()?;
    validate_trigger_not_reserved(&conn, &stored_trigger)?;

    let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
    if !settings.scripts_enabled {
        tracing::warn!(
            "Warning: Global script execution is currently disabled. This script trigger will not trigger until `scripts_enabled` is set to true."
        );
    }

    // Check for an existing active trigger with the same trigger tuple and app filters.
    let existing_record: Option<(String, i64, Option<i64>)> = conn
        .query_row(
            "SELECT id, usage_count, last_used_at
          FROM triggers
          WHERE trigger_type = ?1
            AND trigger = ?2
            AND target_os = ?3
            AND COALESCE(only_apps, '') = ?4
            AND COALESCE(except_apps, '') = ?5
            AND is_deleted = 0
          ORDER BY updated_at DESC
          LIMIT 1",
            rusqlite::params![
                prepared.trigger_type.as_db_str(),
                stored_trigger.as_str(),
                os.as_str(),
                include_apps.as_deref().unwrap_or(""),
                exclude_apps.as_deref().unwrap_or(""),
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();

    let (id, usage_count, last_used_at, is_update) = match existing_record {
        Some((existing_id, existing_usage, existing_last_used)) => {
            (existing_id, existing_usage, existing_last_used, true)
        }
        None => {
            taurine_core::db::crud::validate_trigger_target_os_conflict(
                &conn,
                prepared.trigger_type,
                &stored_trigger,
                &os,
                include_apps.as_deref(),
                exclude_apps.as_deref(),
                None,
            )?;
            (uuid::Uuid::new_v4().to_string(), 0, None, false)
        }
    };

    let action = if is_update { "Updated" } else { "Added" };
    let log_msg = format_trigger_log(
        action,
        &stored_trigger,
        Some((mode, lang)),
        &os,
        include_apps.as_deref(),
        exclude_apps.as_deref(),
    );
    tracing::info!("{}", log_msg);
    if json {
        let status = if is_update { "updated" } else { "created" };
        println!(
            "{}",
            serde_json::json!({"status": status, "trigger": stored_trigger, "action_type": "script"})
        );
    }

    // 3. Compress the script
    let compressed = compress(&content)?;

    let tags_str = if let Some(ref t) = tags {
        serde_json::to_string(t).map_err(|e| taurine_core::Error::Config(e.to_string()))?
    } else if is_update {
        conn.query_row("SELECT tags FROM triggers WHERE id = ?1", [&id], |r| {
            r.get(0)
        })
        .unwrap_or_else(|_| "[]".to_string())
    } else {
        "[]".to_string()
    };

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
                      AND id != ?6
                 )",
                [
                    prepared.trigger_type.as_db_str(),
                    &stored_trigger,
                    &os,
                    include_apps.as_deref().unwrap_or(""),
                    exclude_apps.as_deref().unwrap_or(""),
                    &id,
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
                      AND id != ?6
                 )",
                [
                    prepared.trigger_type.as_db_str(),
                    &stored_trigger,
                    &os,
                    include_apps.as_deref().unwrap_or(""),
                    exclude_apps.as_deref().unwrap_or(""),
                    &id,
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

    // 4. Upsert trigger row (type = "script")
    match prepared.trigger_type {
        TriggerType::Word => {
            taurine_core::db::crud::upsert_trigger_with_type_and_case(
                &conn,
                &id,
                &stored_trigger,
                Some(&format!("Shell script ({})", source_desc)),
                TriggerType::Word,
                &stored_trigger,
                &format!("[Script: {}]", lang_to_str(lang)),
                "script",
                &os,
                &tags_str,
                usage_count,
                last_used_at,
                auto_case,
            )?;
        }
        TriggerType::Hotkey => {
            taurine_core::db::crud::upsert_trigger_with_type_and_case(
                &conn,
                &id,
                &stored_trigger,
                Some(&format!("Shell script ({})", source_desc)),
                TriggerType::Hotkey,
                &stored_trigger,
                &format!("[Script: {}]", lang_to_str(lang)),
                "script",
                &os,
                &tags_str,
                usage_count,
                last_used_at,
                auto_case,
            )?;
        }
        TriggerType::Regex => {
            taurine_core::db::crud::upsert_trigger_with_type_and_case(
                &conn,
                &id,
                &stored_trigger,
                Some(&format!("Shell script ({})", source_desc)),
                TriggerType::Regex,
                &stored_trigger,
                &format!("[Script: {}]", lang_to_str(lang)),
                "script",
                &os,
                &tags_str,
                usage_count,
                last_used_at,
                auto_case,
            )?;
        }
    }

    // 5. Upsert script attachment
    upsert_script(&conn, &id, lang, mode, &compressed)?;

    taurine_core::db::crud::update_trigger_app_filters(&conn, &id, include_apps, exclude_apps)?;

    taurine_core::rpc::notify_daemon_reload();

    Ok(())
}

fn infer_interpreter(path: Option<&std::path::Path>, content: &str) -> Option<ScriptInterpreter> {
    taurine_core::engine::shell::infer_interpreter(path, content)
}

fn lang_to_str(i: ScriptInterpreter) -> &'static str {
    match i {
        ScriptInterpreter::Bash => "bash",
        ScriptInterpreter::PowerShell => "powershell",
        ScriptInterpreter::Python => "python",
        ScriptInterpreter::Node => "node",
        ScriptInterpreter::NodeEsm => "node-esm",
        ScriptInterpreter::Cmd => "cmd",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
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
        assert_eq!(
            infer_interpreter(Some(Path::new("test.mjs")), ""),
            Some(ScriptInterpreter::NodeEsm)
        );
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
            Some(ScriptInterpreter::NodeEsm)
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
}
