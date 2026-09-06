use super::*;
use crate::engine::variables::types::ExpansionStep;

#[test]
fn test_is_reserved() {
    assert!(is_reserved("cursor"));
    assert!(is_reserved("uuid"));
    assert!(is_reserved("clip"));
    assert!(is_reserved("clip(1)"));
    assert!(is_reserved("clip.truncate(5)"));
    assert!(is_reserved("clip(2).upper"));
    assert!(is_reserved("uuid.v4"));
    assert!(is_reserved("time"));
    assert!(is_reserved("time.utc"));
    assert!(is_reserved("net.localip"));
    assert!(is_reserved("net.localip"));
    assert!(is_reserved("exec.bash(echo hi)"));
    assert!(is_reserved("random.int(1, 9)"));
    assert!(is_reserved("lorem"));
    assert!(is_reserved("lorem.word(3)"));

    // These are valid user variables and should not be reserved
    assert!(!is_reserved("username"));
}

#[test]
fn test_is_directive() {
    assert!(is_directive("cursor"));
    assert!(is_directive("key(tab)"));
    assert!(is_directive("key(ctrl+a)"));
    assert!(is_directive("delay(200ms)"));
    assert!(is_directive("delay(200)"));
    assert!(!is_directive("key.tab"));
    assert!(!is_directive("delay.200ms"));
    assert!(!is_directive("time.utc"));
}

#[test]
fn test_resolve_random_int_interpolation() {
    assert_eq!(
        crate::engine::variables::interpolate::interpolate(
            "[random.int(5, 5)]",
            &crate::engine::variables::types::ArgMap::default()
        ),
        "5"
    );
}

#[test]
fn test_resolve_lorem_word_interpolation_count() {
    let resolved = crate::engine::variables::interpolate::interpolate(
        "[lorem.word(3)]",
        &crate::engine::variables::types::ArgMap::default(),
    );

    assert_eq!(resolved.split_whitespace().count(), 3);
}

#[test]
fn test_finalize_cursor_positioning() {
    let res = finalize("hello [cursor] world", None);
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::Text("hello  world".to_string()),
            // 6 left arrows to position cursor after "hello "
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
        ]
    );
    assert!(!res.is_calculation);
}

#[test]
fn test_finalize_text_only() {
    let res = finalize("hello world", None);
    assert_eq!(
        res.steps,
        vec![ExpansionStep::Text("hello world".to_string())]
    );
}

#[test]
fn test_finalize_key_directive_splits_into_steps() {
    let res = finalize("name[key(tab)]email", None);
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::Text("name".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::Text("email".to_string()),
        ]
    );
}

#[test]
fn test_finalize_delay_directive() {
    let res = finalize("first[delay(200ms)]second[delay(100)]third", None);
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::Text("first".to_string()),
            ExpansionStep::Delay(200),
            ExpansionStep::Text("second".to_string()),
            ExpansionStep::Delay(100),
            ExpansionStep::Text("third".to_string()),
        ]
    );
}

#[test]
fn test_finalize_inline_run_splits_progressive_steps() {
    let res = finalize("Wait for it... [exec.bash(echo Done!)]", None);

    assert_eq!(
        res.steps[0],
        ExpansionStep::Text("Wait for it... ".to_string())
    );
    match &res.steps[1] {
        ExpansionStep::InlineRun(metadata, transformers) => {
            assert_eq!(
                crate::engine::shell::decompress(&metadata.compressed_content).unwrap(),
                "echo Done!"
            );
            assert!(transformers.is_empty());
        }
        other => panic!("expected InlineRun step, got {other:?}"),
    }
}

#[test]
fn test_finalize_silent_inline_run_uses_silent_metadata() {
    let res = finalize("start[exec.silent.bash(echo background)]end", None);

    assert_eq!(res.steps.len(), 3);
    match &res.steps[1] {
        ExpansionStep::InlineRun(metadata, transformers) => {
            assert_eq!(
                metadata.behavior,
                crate::engine::shell::ScriptBehavior::Silent
            );
            assert!(transformers.is_empty());
        }
        other => panic!("expected InlineRun step, got {other:?}"),
    }
}

#[test]
fn test_finalize_inline_run_with_transformers() {
    let res = finalize("[exec.bash(echo done) | upper | trim]", None);
    assert_eq!(res.steps.len(), 1);
    match &res.steps[0] {
        ExpansionStep::InlineRun(metadata, transformers) => {
            assert_eq!(
                crate::engine::shell::decompress(&metadata.compressed_content).unwrap(),
                "echo done"
            );
            assert_eq!(transformers, &vec!["upper".to_string(), "trim".to_string()]);
        }
        other => panic!("expected InlineRun step, got {other:?}"),
    }
}

#[test]
fn test_finalize_missing_run_file_emits_error_text() {
    let res = finalize("[exec.bash.file(C:\\definitely\\missing.sh)]", None);

    assert_eq!(
        res.steps,
        vec![ExpansionStep::Text(
            "[Error: path to script not found!]".to_string()
        )]
    );
}

#[test]
fn test_finalize_cursor_suppressed_when_key_directives_present() {
    let res = finalize("name[cursor][key(tab)]email", None);
    // [cursor] should be kept as literal text, not processed as a directive.
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::Text("name[cursor]".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::Text("email".to_string()),
        ]
    );
}

#[test]
fn test_finalize_multiple_key_directives() {
    let res = finalize("a[key(tab)]b[key(enter)]c", None);
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::Text("a".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::Text("b".to_string()),
            ExpansionStep::KeyPress("enter".to_string()),
            ExpansionStep::Text("c".to_string()),
        ]
    );
}

#[test]
fn test_finalize_key_alias_case_insensitive() {
    let res = finalize("[key(TAB)]", None);
    assert_eq!(res.steps, vec![ExpansionStep::KeyPress("tab".to_string())]);
}

#[test]
fn test_finalize_key_alias_strips_quotes() {
    let res = finalize("[key(\"tab\")]", None);
    assert_eq!(res.steps, vec![ExpansionStep::KeyPress("tab".to_string())]);
    let res2 = finalize("[key('enter')]", None);
    assert_eq!(
        res2.steps,
        vec![ExpansionStep::KeyPress("enter".to_string())]
    );
}

#[test]
fn test_contains_key_or_delay_directives() {
    assert!(contains_key_or_delay_directives("hello [key(tab)] world"));
    assert!(contains_key_or_delay_directives("test [delay(100ms)]"));
    assert!(!contains_key_or_delay_directives("hello [cursor] world"));
    assert!(!contains_key_or_delay_directives("just plain text"));
    // Escaped should not count.
    assert!(!contains_key_or_delay_directives(r#"\[key(tab)\]"#));
}

#[test]
fn test_parse_delay_ms() {
    assert_eq!(parse_delay_ms("200ms"), Some(200));
    assert_eq!(parse_delay_ms("0ms"), Some(0));
    assert_eq!(parse_delay_ms("1000ms"), Some(1000));
    assert_eq!(parse_delay_ms("invalid"), None);
    assert_eq!(parse_delay_ms("ms"), None);
    // New test cases for seconds:
    assert_eq!(parse_delay_ms("1s"), Some(1000));
    assert_eq!(parse_delay_ms("1.5s"), Some(1500));
    assert_eq!(parse_delay_ms("0.5s"), Some(500));
    assert_eq!(parse_delay_ms("0s"), Some(0));
    assert_eq!(parse_delay_ms("60s"), Some(60000));
}

#[test]
fn test_append_unescaped_segment_control_chars() {
    let mut out = String::new();
    append_unescaped_segment("hello\\nworld\\tgoodbye\\r!", &mut out);
    assert_eq!(out, "hello\\nworld\\tgoodbye\\r!");
}

#[test]
fn test_finalize_with_control_char_escapes() {
    let res = finalize("first\\nsecond\\tthird", None);
    assert_eq!(
        res.steps,
        vec![ExpansionStep::Text("first\\nsecond\\tthird".to_string())]
    );
}

#[test]
fn test_newline_resolves_to_actual_newline() {
    let interpolated = crate::engine::variables::interpolate::interpolate(
        "hello [newline] world",
        &crate::engine::variables::ArgMap::default(),
    );
    let res = finalize(&interpolated, None);
    assert_eq!(
        res.steps,
        vec![ExpansionStep::Text("hello \n world".to_string())]
    );
}

#[test]
fn test_escaped_newline_stays_literal() {
    let res = finalize("hello\\nworld", None);
    assert_eq!(
        res.steps,
        vec![ExpansionStep::Text("hello\\nworld".to_string())]
    );
}

#[test]
fn test_literal_newline_passes_through_unchanged() {
    let res = finalize("hello\nworld", None);
    assert_eq!(
        res.steps,
        vec![ExpansionStep::Text("hello\nworld".to_string())]
    );
}

#[test]
fn test_finalize_escaped_brackets_in_key_mode() {
    let res = finalize(r#"\[literal\][key(tab)]after"#, None);
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::Text("[literal]".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::Text("after".to_string()),
        ]
    );
}

#[test]
fn test_finalize_modifier_combo_key() {
    let res = finalize("[key(ctrl+a)]", None);
    assert_eq!(
        res.steps,
        vec![ExpansionStep::KeyPress("ctrl+a".to_string())]
    );
}

#[test]
fn test_finalize_multi_modifier_combo_case_normalized() {
    let res = finalize("[key(Ctrl+Shift+End)]", None);
    assert_eq!(
        res.steps,
        vec![ExpansionStep::KeyPress("ctrl+shift+end".to_string())]
    );
}

#[test]
fn test_finalize_combo_between_text_segments() {
    let res = finalize("Name[key(tab)]Address[key(shift+tab)]Back", None);
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::Text("Name".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::Text("Address".to_string()),
            ExpansionStep::KeyPress("shift+tab".to_string()),
            ExpansionStep::Text("Back".to_string()),
        ]
    );
}

#[test]
fn test_finalize_standalone_modifier_directives() {
    let res = finalize("[key(mod)][key(super)][key(ctrl)]", None);
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::KeyPress("mod".to_string()),
            ExpansionStep::KeyPress("super".to_string()),
            ExpansionStep::KeyPress("ctrl".to_string()),
        ]
    );
}

#[test]
fn test_validate_output_logic_paths() {
    // These calls shouldn't panic. We are primarily testing the path coverage.
    validate_output("valid", None).unwrap();
    validate_output("[cursor] [cursor]", Some("multi")).unwrap();
    validate_output("[key(tab)] [cursor]", Some("conflict")).unwrap();
    validate_output("[cursor=invalid]", Some("default")).unwrap();
    validate_output("[lorem.word([num=5])]", Some("nested")).unwrap();
    validate_output(r#"\[cursor\] [cursor]"#, Some("escaped")).unwrap();
    validate_output("[clip=invalid]", None).unwrap();
}

#[test]
fn test_validate_output_exceeds_max_length() {
    let long_output = "a".repeat(100_001);
    assert!(validate_output(&long_output, None).is_err());
}

#[test]
fn test_validate_output_at_max_length() {
    let max_output = "a".repeat(100_000);
    assert!(validate_output(&max_output, None).is_ok());
}

#[test]
fn test_validate_output_rejects_empty() {
    assert!(validate_output("", None).is_err());
    assert!(validate_output("   ", None).is_err());
    assert!(validate_output("\t\n", None).is_err());
}

#[test]
fn test_split_key_default_respects_nested_placeholders() {
    assert_eq!(
        split_key_default("lorem.word([num=5])"),
        ("lorem.word([num=5])", None)
    );
    assert_eq!(
        split_key_default("cursor=invalid"),
        ("cursor", Some("invalid"))
    );
}

mod compatibility_finalize_tests {
    use super::*;

    #[test]
    fn delay_directives_also_suppress_cursor_positioning() {
        let res = finalize("start[cursor][delay(25ms)]end", None);

        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("start[cursor]".to_string()),
                ExpansionStep::Delay(25),
                ExpansionStep::Text("end".to_string()),
            ]
        );
    }

    #[test]
    fn escaped_cursor_literal_stays_literal_when_key_directives_exist() {
        let res = finalize(r#"\[cursor\][key(tab)]after"#, None);

        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("[cursor]".to_string()),
                ExpansionStep::KeyPress("tab".to_string()),
                ExpansionStep::Text("after".to_string()),
            ]
        );
    }

    #[test]
    fn first_cursor_location_wins_when_multiple_cursors_exist() {
        let res = finalize("[cursor]alpha[cursor]beta", None);

        assert_eq!(res.steps[0], ExpansionStep::Text("alphabeta".to_string()));
        assert_eq!(res.steps.len(), "alphabeta".chars().count() + 1);
        assert!(
            res.steps[1..]
                .iter()
                .all(|step| matches!(step, ExpansionStep::KeyPress(key) if key == "left"))
        );
    }

    #[test]
    fn key_and_delay_directives_preserve_current_execution_order() {
        let res = finalize("a[key(tab)]b[delay(10ms)]c[key(enter)]", None);

        assert_eq!(
            res.steps,
            vec![
                ExpansionStep::Text("a".to_string()),
                ExpansionStep::KeyPress("tab".to_string()),
                ExpansionStep::Text("b".to_string()),
                ExpansionStep::Delay(10),
                ExpansionStep::Text("c".to_string()),
                ExpansionStep::KeyPress("enter".to_string()),
            ]
        );
    }

    #[test]
    fn escaped_directives_are_unescaped_to_text() {
        let res = finalize(r#"\[key(enter)\]"#, None);
        assert_eq!(
            res.steps,
            vec![ExpansionStep::Text("[key(enter)]".to_string())]
        );
    }

    #[test]
    fn escaped_quotes_are_unescaped() {
        let res = finalize(r#"echo \'hello\' | grep \"hello\""#, None);
        assert_eq!(
            res.steps,
            vec![ExpansionStep::Text(
                r#"echo 'hello' | grep "hello""#.to_string()
            )]
        );
    }

    fn evaluate_template(
        text: &str,
        args: Option<&crate::engine::variables::types::ArgMap>,
    ) -> FinalExpansion {
        let interpolated = crate::engine::variables::interpolate::interpolate(
            text,
            args.unwrap_or(&crate::engine::variables::types::ArgMap::default()),
        );
        finalize(&interpolated, None)
    }

    #[test]
    fn test_template_syntax_spec_compliance_evaluation() {
        let _guard = crate::testing::TEST_LOCK.lock().unwrap();
        let (_dir, _conn) = crate::testing::open_test_db();
        unsafe {
            std::env::set_var(
                "TAURINE_DB_PATH",
                _dir.path().join("test_taurine.db").to_str().unwrap(),
            );
        }

        // Test Case 1: testvars
        {
            let mut args = crate::engine::variables::types::ArgMap::default();
            args.positional.push("Bob".to_string());
            args.positional.push("New York".to_string());
            args.named.insert("role".to_string(), "Manager".to_string());
            let res = evaluate_template(
                "Hello [0=friend]! You live in [1='San Francisco'] and work as [role='Software Engineer'].",
                Some(&args),
            );
            assert_eq!(
                res.steps,
                vec![ExpansionStep::Text(
                    "Hello Bob! You live in New York and work as Manager.".to_string()
                )]
            );
        }

        // Test Case 2: testescape
        {
            let mut args = crate::engine::variables::types::ArgMap::default();
            args.positional.push("custom".to_string());
            let res = evaluate_template(
                "Escaped brackets: \\[0=ignored\\] | Literal pipe: [0='default value' \\| upper] | Parsed pipe: [0='hello' | upper]",
                Some(&args),
            );
            assert_eq!(
                res.steps,
                vec![ExpansionStep::Text(
                    "Escaped brackets: [0=ignored] | Literal pipe: custom | Parsed pipe: CUSTOM"
                        .to_string()
                )]
            );

            let res_no_args = evaluate_template(
                "Escaped brackets: \\[0=ignored\\] | Literal pipe: [0='default value' \\| upper] | Parsed pipe: [0='hello' | upper]",
                None,
            );
            assert_eq!(res_no_args.steps, vec![ExpansionStep::Text("Escaped brackets: [0=ignored] | Literal pipe: 'default value' | upper | Parsed pipe: 'DEFAULT VALUE' | UPPER".to_string())]);
        }

        // Test Case 3: testdatetime
        {
            let res = evaluate_template(
                "Local: [date] [time] | UTC +1w: [date.utc.calc(+1w).format('Today is' dddd, MMMM D, YYYY)] | UTC Time -2h: [time.utc.calc(-2h).format(hh:mm A)] | Cased AM/PM: [time.format(A) | lower]",
                None,
            );
            assert_eq!(res.steps.len(), 1);
            if let ExpansionStep::Text(ref text) = res.steps[0] {
                assert!(text.contains("Local: "));
                assert!(text.contains("UTC +1w: Today is "));
                assert!(text.contains("UTC Time -2h: "));
                assert!(text.contains("Cased AM/PM: "));
            } else {
                panic!("Expected Text step");
            }
        }

        // Test Case 4: testenv
        {
            unsafe {
                std::env::set_var("USERNAME", "aimer");
                std::env::set_var("USERPROFILE", "c:\\users\\aimer");
            }
            let res = evaluate_template(
                "User (Title Case): [env(USERNAME) | title] | Home Path (Lowercase): [env(USERPROFILE) | lower]",
                None,
            );
            assert_eq!(
                res.steps,
                vec![ExpansionStep::Text(
                    "User (Title Case): Aimer | Home Path (Lowercase): c:\\users\\aimer"
                        .to_string()
                )]
            );
        }

        // Test Case 5: testfile
        {
            if let Some(home) = directories::UserDirs::new().map(|d| d.home_dir().to_path_buf()) {
                let path = home.join("taurine_test.txt");
                std::fs::write(&path, "line one\nline two\nline three").ok();
                let res = evaluate_template(
                    "Full Content: [file.read(~/taurine_test.txt) | trim] | Line 2: [file.read_line(~/taurine_test.txt, 2) | upper] | Lines 1-3: [file.read_line(~/taurine_test.txt, 1, 3)]",
                    None,
                );
                std::fs::remove_file(&path).ok();

                assert_eq!(res.steps.len(), 1);
                if let ExpansionStep::Text(ref text) = res.steps[0] {
                    let normalized = text.replace("\r\n", "\n");
                    assert_eq!(
                        normalized,
                        "Full Content: line one\nline two\nline three | Line 2: LINE TWO | Lines 1-3: line one\nline two\nline three"
                    );
                } else {
                    panic!("Expected Text step");
                }
            }
        }

        // Test Case 6: testclip
        {
            super::clip::set_mock_clip_history(vec![
                "  apple pie  ".to_string(),
                "banana".to_string(),
            ]);
            let res = evaluate_template(
                "Latest (Slugified): [clip | slug] | Second: [clip(0) | trim] | Third (Upper): [clip(1) | upper] | Empty index: [clip(2) | squote]",
                None,
            );
            super::clip::set_mock_clip(None);

            assert_eq!(
                    res.steps,
                    vec![ExpansionStep::Text("Latest (Slugified): apple-pie | Second: apple pie | Third (Upper): BANANA | Empty index: ''".to_string())]
                );
        }

        // Test Case 7: testexec
        {
            let res = evaluate_template(
                "Cwd Path: [exec.powershell((Get-Location).Path) | trim] | Cmd Command: [exec.cmd(echo hello from cmd) | upper] | Silent Task: [exec.silent.powershell(echo 'background task')]",
                None,
            );
            assert_eq!(res.steps.len(), 6);
            assert_eq!(res.steps[0], ExpansionStep::Text("Cwd Path: ".to_string()));
            if let ExpansionStep::InlineRun(ref m, ref t) = res.steps[1] {
                assert_eq!(t, &vec!["trim".to_string()]);
                assert_eq!(m.behavior, crate::engine::shell::ScriptBehavior::Inline);
            } else {
                panic!("Expected InlineRun");
            }
            assert_eq!(
                res.steps[2],
                ExpansionStep::Text(" | Cmd Command: ".to_string())
            );
            if let ExpansionStep::InlineRun(ref m, ref t) = res.steps[3] {
                assert_eq!(t, &vec!["upper".to_string()]);
                assert_eq!(m.behavior, crate::engine::shell::ScriptBehavior::Inline);
            } else {
                panic!("Expected InlineRun");
            }
            assert_eq!(
                res.steps[4],
                ExpansionStep::Text(" | Silent Task: ".to_string())
            );
            if let ExpansionStep::InlineRun(ref m, ref t) = res.steps[5] {
                assert!(t.is_empty());
                assert_eq!(m.behavior, crate::engine::shell::ScriptBehavior::Silent);
            } else {
                panic!("Expected InlineRun");
            }
        }

        // Test Case 8: testhttp
        {
            let res = evaluate_template(
                "Status: [http.status(https://httpbin.org/status/200)] | UA: [http.get(https://httpbin.org/headers) | json.get('headers.User-Agent') | truncate(15)]",
                None,
            );
            assert_eq!(res.steps.len(), 1);
            if let ExpansionStep::Text(ref text) = res.steps[0] {
                assert_eq!(
                    text,
                    "Status: \x03\x1Fsys:http.status(https://httpbin.org/status/200)\x04 | UA: \x03\x1Fsys:http.get(https://httpbin.org/headers) | json.get('headers.User-Agent') | truncate(15)\x04"
                );
            } else {
                panic!("Expected Text step");
            }
        }

        // Test Case 9: testrandom
        {
            let res = evaluate_template(
                "Int (10-50): [random.int(10, 50)] | Pass (12): [random.pass(12)] | Choice: [random.choice(apple, banana, cherry) | title] | Lorem (Dynamic Count): [lorem.word([random.int(2, 4)]) | kebab]",
                None,
            );
            assert_eq!(res.steps.len(), 1);
            if let ExpansionStep::Text(ref text) = res.steps[0] {
                assert!(text.contains("Int (10-50): "));
                assert!(text.contains(" | Pass (12): "));
                assert!(text.contains(" | Choice: "));
                assert!(text.contains(" | Lorem (Dynamic Count): "));
            } else {
                panic!("Expected Text step");
            }
        }

        // Test Case 11: testnested
        {
            let conn = rusqlite::Connection::open(crate::paths::get_db_path()).unwrap();
            conn.execute(
                    "INSERT OR REPLACE INTO triggers (id, trigger, output, action_type, target_os, name, tags, is_deleted, created_at, updated_at)
                     VALUES ('test_inner_id', 'testinner', 'Hello from the inner snippet!', 'text', 'all', 'testinner', '[]', 0, 1719878400, 1719878400)",
                    []
                ).unwrap();

            let res = evaluate_template("Output: [use('testinner') | upper] | Date: [date]", None);

            conn.execute("DELETE FROM triggers WHERE id = 'test_inner_id'", [])
                .ok();

            assert_eq!(res.steps.len(), 1);
            if let ExpansionStep::Text(ref text) = res.steps[0] {
                println!("ACTUAL TEXT: {}", text);
                assert!(text.contains("Output: HELLO FROM THE INNER SNIPPET! | Date: "));
            } else {
                panic!("Expected Text step");
            }
        }

        // Test Case 12: testcombo
        {
            let res = evaluate_template(
                "User [name='Developer'] checked [url='httpbin.org/json'] at [time.utc.format(HH:mm)] UTC. Title of JSON: [http.get([url]) | json.get('slideshow.title') | upper]",
                None,
            );
            assert_eq!(res.steps.len(), 1);
            if let ExpansionStep::Text(ref text) = res.steps[0] {
                assert!(text.contains("User Developer checked httpbin.org/json at "));
                assert!(text.contains(" UTC. Title of JSON: \x03\x1Fsys:http.get(httpbin.org/json) | json.get('slideshow.title') | upper\x04"));
            } else {
                panic!("Expected Text step");
            }
        }

        // Test Case 13: testkeys
        {
            let mut args = crate::engine::variables::types::ArgMap::default();
            args.positional.push("Jane".to_string());
            args.positional.push("Smith".to_string());
            args.positional.push("Admin".to_string());
            let res = evaluate_template(
                "[0=first][key(tab)][delay(100ms)][1=second][key(tab)][delay(50)][2=third][key(enter)]",
                Some(&args),
            );
            assert_eq!(
                res.steps,
                vec![
                    ExpansionStep::Text("Jane".to_string()),
                    ExpansionStep::KeyPress("tab".to_string()),
                    ExpansionStep::Delay(100),
                    ExpansionStep::Text("Smith".to_string()),
                    ExpansionStep::KeyPress("tab".to_string()),
                    ExpansionStep::Delay(50),
                    ExpansionStep::Text("Admin".to_string()),
                    ExpansionStep::KeyPress("enter".to_string()),
                ]
            );
        }
    }
}

#[test]
fn test_finalize_ai_origin_blocks_all_directives_and_cursor() {
    use crate::engine::variables::types::ExpansionOrigin;

    let input = "AI output with [key(tab)] [delay(100ms)] [mouse.click] [img(file:\"a.png\")] [exec.python(\"1\")] and [cursor]";
    let res = finalize_with_origin(input, None, ExpansionOrigin::Ai);
    assert_eq!(
        res.steps,
        vec![ExpansionStep::Text(
            "AI output with [key(tab)] [delay(100ms)] [mouse.click] [img(file:\"a.png\")] [exec.python(\"1\")] and [cursor]"
                .to_string()
        )]
    );
}

#[test]
fn test_finalize_user_origin_preserves_exec_inline_run() {
    use crate::engine::variables::types::ExpansionOrigin;

    let input = "User snippet [exec.powershell(\"whoami\")]";
    let res = finalize_with_origin(input, None, ExpansionOrigin::User);
    assert_eq!(res.steps.len(), 2);
    assert_eq!(
        res.steps[0],
        ExpansionStep::Text("User snippet ".to_string())
    );
    assert!(matches!(res.steps[1], ExpansionStep::InlineRun(_, _)));
}

#[test]
fn test_parse_mouse_directive_all_buttons() {
    use crate::keys::MouseButton;

    assert_eq!(
        parse_mouse_directive("mouse.click"),
        Some(ExpansionStep::MouseClick(MouseButton::Left))
    );
    assert_eq!(
        parse_mouse_directive("mouse.click(left)"),
        Some(ExpansionStep::MouseClick(MouseButton::Left))
    );
    assert_eq!(
        parse_mouse_directive("mouse.click(right)"),
        Some(ExpansionStep::MouseClick(MouseButton::Right))
    );
    assert_eq!(
        parse_mouse_directive("mouse.click(middle)"),
        Some(ExpansionStep::MouseClick(MouseButton::Middle))
    );
    assert_eq!(
        parse_mouse_directive("mouse.click(m4)"),
        Some(ExpansionStep::MouseClick(MouseButton::Button4))
    );
    assert_eq!(
        parse_mouse_directive("mouse.click(back)"),
        Some(ExpansionStep::MouseClick(MouseButton::Button4))
    );
    assert_eq!(
        parse_mouse_directive("mouse.click(m5)"),
        Some(ExpansionStep::MouseClick(MouseButton::Button5))
    );
    assert_eq!(
        parse_mouse_directive("mouse.click(forward)"),
        Some(ExpansionStep::MouseClick(MouseButton::Button5))
    );
    assert_eq!(
        parse_mouse_directive("mouse.click(m6)"),
        Some(ExpansionStep::MouseClick(MouseButton::Other(6)))
    );
    assert_eq!(
        parse_mouse_directive("mouse.m4"),
        Some(ExpansionStep::MouseClick(MouseButton::Button4))
    );
    assert_eq!(
        parse_mouse_directive("mouse.m5"),
        Some(ExpansionStep::MouseClick(MouseButton::Button5))
    );
    assert_eq!(
        parse_mouse_directive("mouse.dblclick"),
        Some(ExpansionStep::MouseDblClick(MouseButton::Left))
    );
    assert_eq!(
        parse_mouse_directive("mouse.dblclick(m4)"),
        Some(ExpansionStep::MouseDblClick(MouseButton::Button4))
    );
    assert_eq!(
        parse_mouse_directive("mouse.down"),
        Some(ExpansionStep::MouseDown(MouseButton::Left))
    );
    assert_eq!(
        parse_mouse_directive("mouse.down(middle)"),
        Some(ExpansionStep::MouseDown(MouseButton::Middle))
    );
    assert_eq!(
        parse_mouse_directive("mouse.up"),
        Some(ExpansionStep::MouseUp(MouseButton::Left))
    );
    assert_eq!(
        parse_mouse_directive("mouse.up(middle)"),
        Some(ExpansionStep::MouseUp(MouseButton::Middle))
    );
    assert_eq!(
        parse_mouse_directive("mouse.hold(m5)"),
        Some(ExpansionStep::MouseDown(MouseButton::Button5))
    );
    assert_eq!(
        parse_mouse_directive("mouse.release(m5)"),
        Some(ExpansionStep::MouseUp(MouseButton::Button5))
    );
    assert_eq!(
        parse_mouse_directive("mouse.move(1920, 1080)"),
        Some(ExpansionStep::MouseMove(1920, 1080))
    );
    assert_eq!(
        parse_mouse_directive("mouse.scroll(-120)"),
        Some(ExpansionStep::MouseScroll(-120))
    );
    assert_eq!(parse_mouse_directive("mouse.click(invalid)"), None);
}

#[test]
fn test_finalize_mouse_directives() {
    use crate::keys::MouseButton;

    let input = "Action: [mouse.click(m4)][delay(50)][mouse.dblclick(left)]";
    let res = finalize(input, None);
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::Text("Action: ".to_string()),
            ExpansionStep::MouseClick(MouseButton::Button4),
            ExpansionStep::Delay(50),
            ExpansionStep::MouseDblClick(MouseButton::Left),
        ]
    );
}
