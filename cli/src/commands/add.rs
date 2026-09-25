use crate::args::AddArgs;
use taurine_core::db::crud::{
    AddOutcome, InvocationType, NewEntry, TriggerAliasRow, TriggerType, app_filters_overlap,
    audit_payload_tags_with_trigger_type, find_parent_by_invocation, get_trigger,
    target_os_values_overlap, upsert_entry_full,
};
use taurine_core::db::init;
use tracing::info;

/// Maximum invocations (word + typed) per add command (§0.14).
const MAX_INVOCATIONS: usize = 20;

pub fn execute_args(args: AddArgs, json: bool) -> taurine_core::error::Result<()> {
    if args.sub.is_some() {
        return crate::commands::script::execute_args(args, json);
    }

    let typed_total = args.hotkey.len() + args.regex.len() + args.voice.len();
    let (words, output) = match args.positional.len() {
        0 => {
            let diag = taurine_core::diagnostic::Diagnostic::problem(
                "Missing trigger and replacement output",
            )
            .help("Specify both the trigger and the replacement output, or add a script trigger:")
            .example("taurine add :brb Be right back!")
            .example("taurine add script ./greet.sh :greet --lang bash");
            return Err(taurine_core::Error::Config(diag.render()));
        }
        1 if typed_total == 0 => {
            let diag = taurine_core::diagnostic::Diagnostic::problem("no trigger specified")
                .help("Specify at least one trigger plus the replacement output:")
                .example("taurine add :brb Be right back!")
                .example("taurine add --hotkey ctrl+h Be right back!");
            return Err(taurine_core::Error::Config(diag.render()));
        }
        n => (
            args.positional[..n - 1].to_vec(),
            args.positional[n - 1].clone(),
        ),
    };

    let words: Vec<String> = if args.auto_case {
        words.into_iter().map(|w| w.to_lowercase()).collect()
    } else {
        words
    };
    let mut invocations: Vec<(InvocationType, String, bool)> = Vec::new();
    invocations.extend(words.into_iter().map(|w| (InvocationType::Word, w, false)));
    invocations.extend(
        args.hotkey
            .into_iter()
            .map(|h| (InvocationType::Hotkey, h, false)),
    );
    invocations.extend(
        args.regex
            .into_iter()
            .map(|r| (InvocationType::Regex, r, false)),
    );
    invocations.extend(
        args.voice
            .into_iter()
            .map(|v| (InvocationType::Voice, v, false)),
    );
    if invocations.len() > MAX_INVOCATIONS {
        return Err(taurine_core::Error::Config(format!(
            "Too many invocations ({}); maximum is {} per add",
            invocations.len(),
            MAX_INVOCATIONS
        )));
    }

    let conn = init::setup()?;
    let os = args
        .os
        .to_db_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| taurine_core::db::get_current_os_db_string().to_string());

    // Single audit + output validation, keyed on the first word invocation
    // (first invocation of any type when there are no words). Runs before any
    // DB write so pure-diagnostic paths stay hermetic.
    let audit_type = if invocations
        .iter()
        .any(|(t, _, _)| *t == InvocationType::Regex)
    {
        TriggerType::Regex
    } else {
        TriggerType::Word
    };
    audit_payload_tags_with_trigger_type(&output, audit_type)?;
    let first = invocations
        .iter()
        .find(|(t, _, _)| *t == InvocationType::Word)
        .or_else(|| invocations.first())
        .map(|(_, s, _)| s.as_str());
    taurine_core::engine::variables::system::validate_output(&output, first)?;

    // R2: an upsert onto an existing entry reuses its name/description/tags
    // for flags the user did not pass; new entries default to ""/None/'[]'.
    let (reuse_name, reuse_description, reuse_tags) = existing_entry_defaults(
        &conn,
        &invocations,
        &os,
        args.include_apps.as_deref(),
        args.exclude_apps.as_deref(),
    )?;
    let name = args.name.unwrap_or(reuse_name);
    let description = args.description.or(reuse_description);
    let tags_json = match args.tag {
        Some(tags) => {
            serde_json::to_string(&tags).map_err(|e| taurine_core::Error::Config(e.to_string()))?
        }
        None => reuse_tags,
    };

    let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
    if !settings.clipboard_history_enabled && output.contains("[clip") {
        tracing::warn!(
            "Warning: The trigger contains '[clip]' system variables, which won't work because clipboard history is disabled in the settings."
        );
    }

    let outcome = upsert_entry_full(
        &conn,
        NewEntry {
            name: name.clone(),
            description,
            content: output,
            action_type: "text".to_string(),
            target_os: os.clone(),
            only_apps: args.include_apps.clone(),
            except_apps: args.exclude_apps.clone(),
            tags_json,
            auto_case: args.auto_case,
            interpreter: None,
            behavior: None,
            invocations: invocations.clone(),
        },
    )?;

    let (display, aliases) = resolve_display(&conn, &name, &invocations);
    report_outcome(
        outcome,
        &display,
        &aliases,
        &os,
        args.include_apps.as_deref(),
        args.exclude_apps.as_deref(),
        json,
    );
    Ok(())
}

pub fn execute(
    trigger: String,
    output: String,
    os: String,
    use_hotkey: bool,
    include_apps: Option<String>,
    exclude_apps: Option<String>,
) -> taurine_core::error::Result<()> {
    let invocation_type = if use_hotkey {
        InvocationType::Hotkey
    } else {
        InvocationType::Word
    };
    let trigger_type = if use_hotkey {
        TriggerType::Hotkey
    } else {
        TriggerType::Word
    };
    let conn = init::setup()?;
    let invocations = vec![(invocation_type, trigger.clone(), false)];
    audit_payload_tags_with_trigger_type(&output, trigger_type)?;
    taurine_core::engine::variables::system::validate_output(&output, Some(&trigger))?;
    let (name, description, tags_json) = existing_entry_defaults(
        &conn,
        &invocations,
        &os,
        include_apps.as_deref(),
        exclude_apps.as_deref(),
    )?;
    let outcome = upsert_entry_full(
        &conn,
        NewEntry {
            name: name.clone(),
            description,
            content: output,
            action_type: "text".to_string(),
            target_os: os.clone(),
            only_apps: include_apps.clone(),
            except_apps: exclude_apps.clone(),
            tags_json,
            auto_case: false,
            interpreter: None,
            behavior: None,
            invocations: invocations.clone(),
        },
    )?;

    let (display, aliases) = resolve_display(&conn, &name, &invocations);
    report_outcome(
        outcome,
        &display,
        &aliases,
        &os,
        include_apps.as_deref(),
        exclude_apps.as_deref(),
        false,
    );
    Ok(())
}

/// R2: when the requested invocations hit exactly one scope-overlapping live
/// parent, return its (name, description, tags) for flag-absent reuse.
/// Otherwise (no hit, or a disjoint scope that upsert will create fresh)
/// return the new-entry defaults "" / None / '[]'.
pub(crate) fn existing_entry_defaults(
    conn: &rusqlite::Connection,
    invocations: &[(InvocationType, String, bool)],
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
) -> taurine_core::error::Result<(String, Option<String>, String)> {
    for (invocation_type, invocation, _) in invocations {
        let Some(pid) = find_parent_by_invocation(conn, *invocation_type, invocation)? else {
            continue;
        };
        let Some(row) = get_trigger(conn, &pid)? else {
            continue;
        };
        if row.is_deleted {
            continue;
        }
        if !target_os_values_overlap(target_os, &row.target_os) {
            continue;
        }
        if !app_filters_overlap(
            only_apps,
            except_apps,
            row.only_apps.as_deref(),
            row.except_apps.as_deref(),
        ) {
            continue;
        }
        return Ok((row.name, row.description, row.tags));
    }
    Ok((String::new(), None, "[]".to_string()))
}

/// Display string + stored aliases for the just-upserted entry, looked up
/// through the requested invocations (first hit wins).
pub(crate) fn resolve_display(
    conn: &rusqlite::Connection,
    name: &str,
    invocations: &[(InvocationType, String, bool)],
) -> (String, Vec<TriggerAliasRow>) {
    for (invocation_type, invocation, _) in invocations {
        if let Ok(Some(pid)) = find_parent_by_invocation(conn, *invocation_type, invocation)
            && let Ok(Some(row)) = get_trigger(conn, &pid)
        {
            return (row.display, row.invocations);
        }
    }
    let fallback = if name.is_empty() {
        invocations
            .first()
            .map(|(_, s, _)| s.clone())
            .unwrap_or_default()
    } else {
        name.to_string()
    };
    (fallback, Vec::new())
}

fn report_outcome(
    outcome: AddOutcome,
    display: &str,
    aliases: &[TriggerAliasRow],
    os: &str,
    include_apps: Option<&str>,
    exclude_apps: Option<&str>,
    json: bool,
) {
    use crate::commands::validate::format_trigger_log;
    let invocations: Vec<serde_json::Value> = aliases
        .iter()
        .map(|a| {
            serde_json::json!({"type": a.invocation_type.as_db_str(), "invocation": a.invocation})
        })
        .collect();
    match outcome {
        AddOutcome::Created => {
            let log_msg =
                format_trigger_log("Added", display, None, os, include_apps, exclude_apps);
            info!("{}", log_msg);
            taurine_core::rpc::notify_daemon_reload();
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "created", "trigger": display, "invocations": invocations})
                );
            }
        }
        AddOutcome::AlreadyExists => {
            let log_msg = format_trigger_log(
                "Trigger already exists for",
                display,
                None,
                os,
                include_apps,
                exclude_apps,
            );
            info!("{}", log_msg);
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "exists", "trigger": display, "invocations": invocations})
                );
            }
        }
        AddOutcome::Updated => {
            let log_msg =
                format_trigger_log("Updated", display, None, os, include_apps, exclude_apps);
            info!("{}", log_msg);
            taurine_core::rpc::notify_daemon_reload();
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "updated", "trigger": display, "invocations": invocations})
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::TargetOsCli;
    use taurine_core::db::crud::{count_aliases, list_aliases};
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

    fn add_args(
        positional: Vec<&str>,
        hotkey: Vec<&str>,
        regex: Vec<&str>,
        voice: Vec<&str>,
    ) -> AddArgs {
        AddArgs {
            sub: None,
            positional: positional.into_iter().map(str::to_string).collect(),
            hotkey: hotkey.into_iter().map(str::to_string).collect(),
            regex: regex.into_iter().map(str::to_string).collect(),
            voice: voice.into_iter().map(str::to_string).collect(),
            include_apps: None,
            exclude_apps: None,
            os: TargetOsCli::All,
            tag: None,
            name: None,
            description: None,
            auto_case: false,
        }
    }

    fn parent_for(
        conn: &rusqlite::Connection,
        invocation_type: InvocationType,
        invocation: &str,
    ) -> String {
        find_parent_by_invocation(conn, invocation_type, invocation)
            .unwrap()
            .unwrap_or_else(|| panic!("no parent for {invocation}"))
    }

    #[test]
    fn multi_alias_single_entry_groups_all_types() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute_args(
                add_args(
                    vec!["hi", "hello", "Hello!"],
                    vec!["ctrl+h"],
                    vec![],
                    vec!["say hi"],
                ),
                false,
            )
            .unwrap();

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Word, "hi");
            assert_eq!(parent_for(&conn, InvocationType::Word, "hello"), pid);
            assert_eq!(parent_for(&conn, InvocationType::Hotkey, "ctrl+h"), pid);
            assert_eq!(parent_for(&conn, InvocationType::Voice, "say hi"), pid);
            assert_eq!(count_aliases(&conn, &pid).unwrap(), 4);
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert_eq!(row.output, "Hello!");
            assert_eq!(row.display, "hi");
        });
    }

    #[test]
    fn new_entry_name_defaults_empty_for_display_rule() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute_args(
                add_args(vec!["gs", "git status"], vec![], vec![], vec![]),
                false,
            )
            .unwrap();

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Word, "gs");
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert_eq!(row.name, "");
            assert_eq!(row.display, "gs");
        });
    }

    #[test]
    fn upsert_preserves_name_description_tags_when_flags_absent() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let mut first = add_args(vec!["gs", "one"], vec![], vec![], vec![]);
            first.name = Some("My entry".to_string());
            first.description = Some("desc".to_string());
            first.tag = Some(vec!["dev".to_string()]);
            execute_args(first, false).unwrap();

            execute_args(add_args(vec!["gs", "two"], vec![], vec![], vec![]), false).unwrap();

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Word, "gs");
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert_eq!(row.output, "two");
            assert_eq!(row.name, "My entry");
            assert_eq!(row.description.as_deref(), Some("desc"));
            assert_eq!(row.tags, "[\"dev\"]");
            assert_eq!(count_aliases(&conn, &pid).unwrap(), 1);
        });
    }

    #[test]
    fn hotkey_only_with_single_positional() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute_args(
                add_args(vec!["Do it"], vec!["ctrl+h"], vec![], vec![]),
                false,
            )
            .unwrap();

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Hotkey, "ctrl+h");
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert_eq!(row.output, "Do it");
            assert_eq!(row.invocations.len(), 1);
            assert_eq!(row.display, "ctrl+h");
        });
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

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Word, "gs");
            assert_eq!(list_aliases(&conn, &pid).unwrap().len(), 1);
        });
    }

    #[test]
    fn add_hotkey_creates_canonical_hotkey_trigger() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute(
                "Shift + Ctrl + G".to_string(),
                "git status[key(enter)]".to_string(),
                "all".to_string(),
                true,
                None,
                None,
            )
            .unwrap();

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Hotkey, "ctrl+shift+g");
            assert_eq!(
                list_aliases(&conn, &pid).unwrap()[0].invocation,
                "ctrl+shift+g"
            );
        });
    }

    #[test]
    fn same_hotkey_overlapping_scope_updates_entry() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute(
                "ctrl+shift+g".to_string(),
                "one".to_string(),
                "all".to_string(),
                true,
                None,
                None,
            )
            .unwrap();

            // Same invocation on an overlapping scope updates the entry
            // (§0.9 single-entry-wins) instead of erroring.
            execute(
                "ctrl+shift+g".to_string(),
                "two".to_string(),
                "win".to_string(),
                true,
                None,
                None,
            )
            .unwrap();

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Hotkey, "ctrl+shift+g");
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert_eq!(row.output, "two");
            assert_eq!(row.target_os, "win");
            assert_eq!(count_aliases(&conn, &pid).unwrap(), 1);
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
                "ctrl+shift+h".to_string(),
                "linux".to_string(),
                "linux".to_string(),
                true,
                None,
                None,
            )
            .unwrap();

            let conn = open_keyed_db(db_path);
            let win_pid = parent_for(&conn, InvocationType::Hotkey, "ctrl+shift+g");
            let linux_pid = parent_for(&conn, InvocationType::Hotkey, "ctrl+shift+h");
            assert_ne!(win_pid, linux_pid);
            assert_eq!(count_aliases(&conn, &win_pid).unwrap(), 1);
            assert_eq!(count_aliases(&conn, &linux_pid).unwrap(), 1);
        });
    }

    #[test]
    fn same_hotkey_disjoint_os_coexists_as_two_parents() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let mut win_args = add_args(vec!["Do win"], vec!["ctrl+h"], vec![], vec![]);
            win_args.os = TargetOsCli::Windows;
            execute_args(win_args, false).unwrap();

            let mut linux_args = add_args(vec!["Do linux"], vec!["ctrl+h"], vec![], vec![]);
            linux_args.os = TargetOsCli::Linux;
            execute_args(linux_args, false).unwrap();

            let conn = open_keyed_db(db_path);
            let parents: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM triggers WHERE is_deleted = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(parents, 2, "disjoint scopes keep two parents");
            let aliases: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM trigger_aliases
                      WHERE invocation_type = 'hotkey' AND invocation = 'ctrl+h'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(aliases, 2);
            let mut outputs: Vec<String> = conn
                .prepare("SELECT output FROM triggers WHERE is_deleted = 0 ORDER BY output")
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap();
            outputs.sort();
            assert_eq!(outputs, vec!["Do linux".to_string(), "Do win".to_string()]);
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
            assert!(error.to_string().contains("conflicts"));
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

            let conn = open_keyed_db(db_path);
            let left = parent_for(&conn, InvocationType::Hotkey, "lalt+m");
            let right = parent_for(&conn, InvocationType::Hotkey, "ralt+m");
            assert_ne!(left, right);
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

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Hotkey, "ctrl+shift+g");
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert_eq!(row.output, "two");
            assert_eq!(count_aliases(&conn, &pid).unwrap(), 1);
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

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Word, "gs");
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert_eq!(row.output, "two");
            assert_eq!(count_aliases(&conn, &pid).unwrap(), 1);
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

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Hotkey, "ctrl+shift+g");
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert_eq!(row.output, "two");
            assert_eq!(count_aliases(&conn, &pid).unwrap(), 1);
        });
    }

    #[test]
    fn auto_case_does_not_lowercase_regex_trigger() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            let mut args = add_args(vec!["output"], vec![], vec!["['A-Z']"], vec![]);
            args.auto_case = true;
            execute_args(args, false).unwrap();

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Regex, "['A-Z']");
            assert_eq!(
                list_aliases(&conn, &pid).unwrap()[0].invocation,
                "['A-Z']",
                "auto_case must not lowercase regex trigger"
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

            let conn = open_keyed_db(db_path);
            let word_pid = parent_for(&conn, InvocationType::Word, "tab");
            let hotkey_pid = parent_for(&conn, InvocationType::Hotkey, "tab");
            assert_ne!(word_pid, hotkey_pid);
        });
    }

    #[test]
    fn upsert_spanning_two_entries_errors() {
        init_tracing_for_tests();

        with_test_db(|_db_path| {
            execute_args(add_args(vec!["hi", "one"], vec![], vec![], vec![]), false).unwrap();
            execute_args(
                add_args(vec!["hello", "two"], vec![], vec![], vec![]),
                false,
            )
            .unwrap();

            let err = execute_args(
                add_args(vec!["hi", "hello", "merged"], vec![], vec![], vec![]),
                false,
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains("two different entries"), "Error was: {err}");
        });
    }

    #[test]
    fn invocation_cap_names_the_count() {
        init_tracing_for_tests();

        let words: Vec<&str> = vec!["w"; 21];
        let mut positional: Vec<&str> = words;
        positional.push("output");
        let err = execute_args(add_args(positional, vec![], vec![], vec![]), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("21"), "Error was: {err}");
        assert!(err.contains("20"), "Error was: {err}");
    }

    #[test]
    fn test_add_single_positional_no_trigger_diagnostic() {
        let mut args = add_args(vec![], vec![], vec![], vec![]);
        args.positional = vec![":brb".to_string()];

        let result = execute_args(args, false);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no trigger specified"), "Error was: {err}");
        assert!(!err.contains('`'), "Must not contain backticks: {err}");
        assert!(!err.contains('\''), "Must not contain single quotes: {err}");
    }

    #[test]
    fn test_add_missing_both_diagnostic() {
        let args = add_args(vec![], vec![], vec![], vec![]);

        let result = execute_args(args, false);
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Missing trigger and replacement output"),
            "Error was: {err}"
        );
        assert!(!err.contains('`'), "Must not contain backticks: {err}");
        assert!(!err.contains('\''), "Must not contain single quotes: {err}");
    }

    #[test]
    fn voice_only_with_single_positional_treats_it_as_output() {
        init_tracing_for_tests();

        with_test_db(|db_path| {
            execute_args(
                add_args(vec!["user@example.com"], vec![], vec![], vec!["my email"]),
                false,
            )
            .unwrap();

            let conn = open_keyed_db(db_path);
            let pid = parent_for(&conn, InvocationType::Voice, "my email");
            let row = get_trigger(&conn, &pid).unwrap().unwrap();
            assert_eq!(row.output, "user@example.com");
            assert_eq!(row.invocations.len(), 1);
        });
    }
}
