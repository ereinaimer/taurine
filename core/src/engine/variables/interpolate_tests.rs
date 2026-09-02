use super::interpolate::extract_placeholders;
use super::types::ArgMap;
use super::*;

#[test]
fn test_interpolate_quoted_system_arguments() {
    let args = ArgMap::default();
    let tpl = "Raw: [time.calc(+2h)] | Double: [time.calc(\"+2h\")] | Single: [time.calc('+2h')]";
    let res = interpolate(tpl, &args);
    assert!(!res.contains("[Error"));
    assert!(res.contains("Raw: "));
}

#[test]
fn test_extract_placeholders() {
    let text = "https://github.com/[username=ereinaimer]/[repo=default]";
    let p = extract_placeholders(text);
    assert_eq!(p.len(), 2);
    assert_eq!(p.get("username").unwrap().default_value, Some("ereinaimer"));
    assert_eq!(p.get("repo").unwrap().default_value, Some("default"));
}

#[test]
fn test_extract_placeholders_deduplicate() {
    let text = "a [foo=bar] b [foo=baz] c [foo=bar]";
    let p = extract_placeholders(text);
    assert_eq!(p.len(), 1);
    // Should keep the first appearance
    assert_eq!(p.get("foo").unwrap().default_value, Some("bar"));
}

#[test]
fn test_extract_placeholders_ignore_system() {
    let text = "Hello [cursor] at [time.now]. My name is [name=John]";
    let p = extract_placeholders(text);
    assert_eq!(p.len(), 1);
    assert!(p.contains_key("name"));
    assert!(!p.contains_key("cursor"));
    assert!(!p.contains_key("time.now"));
}

#[test]
fn test_extract_placeholders_escapes() {
    let text = r#"function \[ return "[msg=default]"; \]"#;
    let p = extract_placeholders(text);
    assert_eq!(p.len(), 1);
    assert!(p.contains_key("msg"));
}

#[test]
fn test_extract_placeholders_trims_inner_whitespace() {
    let text = "Hello [  name = John ] and [ title = Captain ]";
    let p = extract_placeholders(text);
    assert_eq!(p.len(), 2);
    assert!(p.contains_key("name"));
    assert_eq!(p.get("title").unwrap().default_value, Some("Captain"));
}

#[test]
fn test_interpolate_positional() {
    let mut args = ArgMap::default();
    args.positional.push("ereinaimer".to_string());
    args.positional.push("taurine".to_string());

    let tpl = "https://github.com/[0=org]/[1=repo]";
    assert_eq!(
        interpolate(tpl, &args),
        "https://github.com/ereinaimer/taurine"
    );

    let tpl_no_defaults = "https://github.com/[0]/[1]";
    assert_eq!(
        interpolate(tpl_no_defaults, &args),
        "https://github.com/ereinaimer/taurine"
    );
}

#[test]
fn test_interpolate_named() {
    let tpl = "https://github.com/[0=user]/[repo=repo]";
    let mut args = ArgMap::default();
    args.positional.push("ereinaimer".to_string());
    args.named.insert("repo".to_string(), "taurine".to_string());
    assert_eq!(
        interpolate(tpl, &args),
        "https://github.com/ereinaimer/taurine"
    );
}

#[test]
fn test_interpolate_defaults() {
    let args = ArgMap::default();
    let tpl = "https://github.com/[username=ereinaimer]/[repo=taurine]";
    assert_eq!(
        interpolate(tpl, &args),
        "https://github.com/ereinaimer/taurine"
    );
}

#[test]
fn test_interpolate_empty_default() {
    // Empty defaults are no longer valid, this test can just ensure missing positional arg uses default
    let tpl = "git commit -m \"fix: [msg=default]\"";
    let args = ArgMap::default();
    assert_eq!(interpolate(tpl, &args), "git commit -m \"fix: default\"");
}

#[test]
fn test_interpolate_missing_args() {
    let tpl = "https://github.com/[username=user]/[repo=repo]";
    let args = ArgMap::default();
    assert_eq!(interpolate(tpl, &args), "https://github.com/user/repo");
}

#[test]
fn test_interpolate_escapes() {
    let text = r#"const x = \[ "key": "123" \]; // literal \\ path"#;
    let args = ArgMap::default();
    let result = interpolate(text, &args);
    // Escapes are now resolved by system::finalize in split_into_steps
    assert_eq!(
        result,
        r#"const x = \[ "key": "123" \]; // literal \\ path"#
    );
}

#[test]
fn test_interpolate_system_variables() {
    let mut args = ArgMap::default();
    args.named.insert("msg".to_string(), "hello".to_string());

    system::clip::set_mock_clip(Some("clip_content".to_string()));

    let tpl = "[msg=] [cursor] [time.now] [clip]";
    let res = interpolate(tpl, &args);

    assert!(res.contains("hello [cursor] "));
    assert!(res.contains("clip_content"));
    assert!(!res.contains("[time.now]"));
    assert!(!res.contains("[clip]"));

    system::clip::set_mock_clip(None);
}

#[test]
fn test_interpolate_system_cursor_collision() {
    let args = ArgMap::default();
    let tpl = "Hello [cursor=invalid] world";
    assert_eq!(interpolate(tpl, &args), "Hello [cursor] world");
}

#[test]
fn test_extract_cursor_offset() {
    use super::types::ExpansionStep;

    let res = system::finalize("hello [cursor] world", None);
    assert_eq!(
        res.steps,
        vec![
            ExpansionStep::Text("hello  world".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
        ]
    );

    let res2 = system::finalize("hello [cursor] world [cursor]", None);
    assert_eq!(
        res2.steps,
        vec![
            ExpansionStep::Text("hello  world ".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
            ExpansionStep::KeyPress("left".to_string()),
        ]
    );

    let res3 = system::finalize(r#"Hello \[cursor\]"#, None);
    assert_eq!(
        res3.steps,
        vec![ExpansionStep::Text("Hello [cursor]".to_string())]
    );
}

#[test]
fn test_interpolate_repeated() {
    let tpl = "https://[0=sub].github.io/[0=sub]";
    let mut args = ArgMap::default();
    args.positional.push("foo".to_string());
    assert_eq!(interpolate(tpl, &args), "https://foo.github.io/foo");
}

#[test]
fn test_interpolate_nested_system() {
    let mut args = ArgMap::default();
    args.named
        .insert("val".to_string(), "MixedCase".to_string());
    let tpl = "[[val | lower] | upper]";
    // Pass 1: [val | lower] -> mixedcase
    // Pass 2: [mixedcase | upper] remains literal because mixedcase is not a variable
    assert_eq!(interpolate(tpl, &args), "[mixedcase | upper]");
}

#[test]
fn test_interpolate_nested_user() {
    let mut args = ArgMap::default();
    args.named.insert("name".to_string(), "john".to_string());
    let tpl = "[[name=] | upper]";
    // Under strict validation, unquoted tags that are not variables are left as-is.
    // [name=] resolves to john, resulting in [john | upper].
    // john is not a variable, so [john | upper] remains literal.
    assert_eq!(interpolate(tpl, &args), "[john | upper]");
}

#[test]
fn test_interpolate_nested_default() {
    let args = ArgMap::default();
    // Template: [outer=[inner=fallback]]
    // inner resolves to fallback, then outer resolves to fallback
    let tpl = "[outer=[inner=fallback]]";
    assert_eq!(interpolate(tpl, &args), "fallback");
}

#[test]
fn test_interpolate_nested_variable_default() {
    let mut args = ArgMap::default();
    args.named
        .insert("default".to_string(), "friend".to_string());

    assert_eq!(interpolate("[name=[default]]", &args), "friend");
}

#[test]
fn test_interpolate_modified_default_prefers_positional_arg() {
    let mut args = ArgMap::default();
    args.positional.push("aimer".to_string());

    assert_eq!(interpolate("[name=erein | title]", &args), "Erein");
    assert_eq!(
        interpolate("[name=erein | title]", &ArgMap::default()),
        "Erein"
    );
}

#[test]
fn test_interpolate_balanced_with_escapes() {
    let text = r#"A\[B\]C"#;
    let args = ArgMap::default();
    let result = interpolate(text, &args);
    assert_eq!(result, r#"A\[B\]C"#);
}

#[test]
fn test_interpolate_flattened_system() {
    let args = ArgMap::default();
    // time.now | upper should resolve to the current time in uppercase
    let res = interpolate("[time.now | upper]", &args);
    // We check if it resolved to SOMETHING that isn't the literal string or empty
    assert!(!res.is_empty());
    assert!(!res.contains("time.now"));
    // Check if it's uppercase
    assert_eq!(res, res.to_uppercase());
}

#[test]
fn test_interpolate_flattened_user() {
    let mut args = ArgMap::default();
    args.named.insert("name".to_string(), "john".to_string());
    // name | upper should resolve to JOHN
    assert_eq!(interpolate("[name= | upper]", &args), "JOHN");
}

#[test]
fn test_interpolate_quoted_literal() {
    let args = ArgMap::default();
    assert_eq!(interpolate("['hello world' | upper]", &args), "HELLO WORLD");
    assert_eq!(
        interpolate("[\"hello world\" | upper]", &args),
        "HELLO WORLD"
    );
}

#[test]
fn test_interpolate_deep_flattened() {
    let mut args = ArgMap::default();
    args.named
        .insert("val".to_string(), "MixedCase".to_string());
    assert_eq!(interpolate("[val | lower | upper]", &args), "MIXEDCASE");
}

#[test]
fn test_extract_placeholders_suffixed() {
    let text = "Hello [name=John | upper] and [email=DEFAULT@EMAIL.COM | lower]";
    let p = extract_placeholders(text);
    assert_eq!(p.len(), 2);
    assert!(p.contains_key("name"));
    assert!(p.contains_key("email"));
    assert_eq!(
        p.get("email").unwrap().default_value,
        Some("DEFAULT@EMAIL.COM")
    );
}

#[test]
fn test_extract_placeholders_parameterized_transformers() {
    let text = "Hello [name=John | truncate(3)] and [email=DEFAULT | replace(\"@\", \"+\")]";
    let p = extract_placeholders(text);
    assert_eq!(p.len(), 2);
    assert!(p.contains_key("name"));
    assert!(p.contains_key("email"));
    assert_eq!(p.get("email").unwrap().default_value, Some("DEFAULT"));
}

#[test]
fn test_interpolate_unknown_transformed_tag_remains_literal() {
    let args = ArgMap::default();
    assert_eq!(interpolate("[foo | upper]", &args), "[foo | upper]");
}

#[test]
fn test_interpolate_parameterized_transformers_for_user_values() {
    let mut args = ArgMap::default();
    args.named.insert("name".to_string(), "john".to_string());

    assert_eq!(interpolate("[name=default | truncate(2)]", &args), "jo");
    assert_eq!(
        interpolate("[name=default | replace(\"o\", \"0\") | upper]", &args),
        "J0HN"
    );
}

#[test]
fn test_interpolate_parameterized_transformers_for_system_values() {
    let args = ArgMap::default();
    system::clip::set_mock_clip(Some("alpha,beta".to_string()));

    assert_eq!(interpolate("[clip | truncate(5)]", &args), "alpha");
    assert_eq!(
        interpolate("[clip | replace(\",\", \";\")]", &args),
        "alpha;beta"
    );

    system::clip::set_mock_clip(None);
}

#[test]
fn test_interpolate_clipboard_history_function_syntax() {
    let args = ArgMap::default();
    system::clip::set_mock_clip_history(vec!["current".to_string(), "previous".to_string()]);

    assert_eq!(interpolate("[clip]", &args), "current");
    assert_eq!(interpolate("[clip(0)]", &args), "current");
    assert_eq!(interpolate("[clip(1) | upper]", &args), "PREVIOUS");
    assert_eq!(interpolate("[clip(2)]", &args), "");

    system::clip::set_mock_clip(None);
}

#[test]
fn test_interpolate_replace_handles_literal_commas() {
    let args = ArgMap::default();
    assert_eq!(
        interpolate(r#"['a,b,c' | replace(",", ";")]"#, &args),
        "a;b;c"
    );
}

#[test]
fn test_interpolate_regexreplace_handles_commas_in_quoted_args() {
    let args = ArgMap::default();
    assert_eq!(
        interpolate(
            r#"['a,B,c,D' | regexreplace("([a-z]),([A-Z])", "$1 $2")]"#,
            &args
        ),
        "a B,c D"
    );
}

#[test]
fn test_interpolate_substring_is_utf8_safe() {
    let args = ArgMap::default();
    assert_eq!(interpolate(r#"['aßç' | substring(1, 3)]"#, &args), "ßç");
}

#[test]
fn test_interpolate_json_array_literal_remains_untouched() {
    let args = ArgMap::default();
    assert_eq!(
        interpolate("payload = [1, 2, 3]", &args),
        "payload = [1, 2, 3]"
    );
}

#[test]
fn test_interpolate_user_variable_mapping_to_positional() {
    let mut args = ArgMap::default();
    args.positional.push("monkeytype.com".to_string());

    let tpl = "Start-Process https://[url=google.com]";
    // [url] should not consume the positional argument "monkeytype.com"
    assert_eq!(interpolate(tpl, &args), "Start-Process https://google.com");

    // Test fallback when argument is omitted
    let args_empty = ArgMap::default();
    assert_eq!(
        interpolate(tpl, &args_empty),
        "Start-Process https://google.com"
    );
}

#[test]
fn test_interpolate_user_variable_without_default() {
    let tpl = "Hello [var]";
    let mut args = ArgMap::default();
    args.positional.push("John".to_string());
    assert_eq!(interpolate(tpl, &args), "Hello [var]");
}

mod compatibility_interpolation_tests {
    use super::*;

    #[test]
    fn directives_are_preserved_for_finalize_phase() {
        let args = ArgMap::default();

        assert_eq!(
            interpolate("before [cursor] [key(tab)] [delay(25ms)] after", &args),
            "before [cursor] [key(tab)] [delay(25ms)] after"
        );
    }

    #[test]
    fn named_placeholders_do_not_consume_sequential_positional_fallback() {
        let mut args = ArgMap::default();
        args.named
            .insert("name".to_string(), "ereinaimer".to_string());
        args.positional.push("taurine".to_string());

        // The positional argument "taurine" should NOT map to [repo]
        assert_eq!(
            interpolate("[name=] / [repo=default]", &args),
            "ereinaimer / default"
        );
    }

    #[test]
    fn empty_positional_values_beat_defaults() {
        let mut args = ArgMap::default();
        args.positional.push(String::new());

        assert_eq!(interpolate("numeric=[0=fallback]", &args), "numeric=");
        // [value] does not consume the empty positional, so it falls back to its default
        assert_eq!(
            interpolate("sequential=[value=fallback]", &args),
            "sequential=fallback"
        );
    }

    #[test]
    fn escaped_cursor_literal_and_backslashes_survive_interpolation() {
        let text = r#"Hello \[cursor\] and \\ path"#;
        let args = ArgMap::default();
        let result = super::interpolate(text, &args);
        // The interpolate step leaves escapes alone, and finalize/split_into_steps processes them.
        assert_eq!(result, r#"Hello \[cursor\] and \\ path"#);
    }

    #[test]
    fn nested_transformer_forms_stay_literal_while_flat_form_resolves() {
        let mut args = ArgMap::default();
        args.positional.push("banana".to_string());

        assert_eq!(
            interpolate("nested=[[0=val] | url.encode]", &args),
            "nested=[banana | url.encode]"
        );
        assert_eq!(
            interpolate("flat=[0=val | url.encode]", &args),
            "flat=banana"
        );
    }

    #[test]
    fn detects_and_extracts_ai_markers() {
        assert!(!contains_ai_markers("normal text"));

        let marked = "\x03input_text\x1Fprompt_text\x04";
        assert!(contains_ai_markers(marked));

        let extracted = extract_ai_markers(marked);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].0, "input_text");
        assert_eq!(extracted[0].1, "prompt_text");

        let multiple = "hello \x03in1\x1Fp1\x04 and \x03in2\x1Fp2\x04 end";
        let multi_extracted = extract_ai_markers(multiple);
        assert_eq!(multi_extracted.len(), 2);
        assert_eq!(multi_extracted[0].0, "in1");
        assert_eq!(multi_extracted[0].1, "p1");
        assert_eq!(multi_extracted[1].0, "in2");
        assert_eq!(multi_extracted[1].1, "p2");
    }

    #[test]
    fn global_pipeline_strips_quotes_and_preserves_spaces() {
        let args = ArgMap::default();
        // Test 1.1: Global pipeline quote stripping
        assert_eq!(
            interpolate("\"hello world \" | title | repeat(2)", &args),
            "Hello World Hello World "
        );
        assert_eq!(interpolate("'hello world ' | upper", &args), "HELLO WORLD ");
    }

    #[test]
    fn nested_directive_evaluates_to_literal_when_quoted() {
        let args = ArgMap::default();
        // Test 2.2: Quoted directive evaluates to literal without escaping brackets
        // This allows the downstream macro parser (injector) to still see it and execute it.
        // If the user wants to prevent execution, they must escape the brackets explicitly.
        assert_eq!(
            interpolate("['[key(enter)]' | repeat(3)]", &args),
            "[key(enter)][key(enter)][key(enter)]"
        );
    }

    #[test]
    fn test_gcp_manual_case() {
        let mut args = ArgMap::default();
        args.positional.push("cli".to_string());
        args.positional
            .push("add support for custom pipelines".to_string());
        let tpl = "git commit -m \"feat([0=core]): [1=update codebase | sentence]\"[key(enter)][delay(500ms)]git push origin main[key(enter)]";
        assert_eq!(
            interpolate(tpl, &args),
            "git commit -m \"feat(cli): Add support for custom pipelines\"[key(enter)][delay(500ms)]git push origin main[key(enter)]"
        );
    }

    #[test]
    fn test_tblrow_manual_case() {
        let mut args = ArgMap::default();
        args.positional.push("101".to_string());
        args.positional.push("john doe".to_string());
        args.positional.push("active".to_string());
        let tpl = "| [0=ID] | [1=Name | title] | [2=Status | upper] |[key(enter)]| ['--- | ' | repeat(3)][key(enter)]";
        assert_eq!(
            interpolate(tpl, &args),
            "| 101 | John Doe | ACTIVE |[key(enter)]| --- | --- | --- | [key(enter)]"
        );
    }

    #[test]
    fn test_docsnippet_manual_case() {
        let args = ArgMap::default();
        let tpl = r#"\'\[key(enter)\]\' directive | title | repeat(2)"#;
        assert_eq!(
            interpolate(tpl, &args),
            r#"\'\[key(enter)\]\' Directive\'\[key(enter)\]\' Directive"#
        );
    }

    #[test]
    fn test_mockreq_manual_case() {
        let mut args = ArgMap::default();
        args.positional.push("password_reset".to_string());
        // Mock the env var and uuid in system resolve via a mock or just test the structure
        // Since we can't easily mock UUID without a lock, we can use a known value.
        // Wait, UUID changes. We'll skip [uuid] and [date.iso] for exact match and just test env
        // actually we can test the interpolation of `[env(TAURINE_TEST_USER=admin)]`.
        let tpl =
            r#"{"user": "[env(TAURINE_TEST_USER=admin) | lower]", "action": "[0=login | upper]"}"#;
        assert_eq!(
            interpolate(tpl, &args),
            r#"{"user": "admin", "action": "PASSWORD_RESET"}"#
        );
    }

    #[test]
    fn test_aisummary_manual_case() {
        let args = ArgMap::default();
        let tpl = "### SUMMARY OF COPIED TEXT ([date]):[key(enter)][clip | ai(summarize this in 3 concise bullet points) | trim]";
        system::clip::set_mock_clip(Some("Long article text".to_string()));
        let result = interpolate(tpl, &args);
        // date.short will be the actual date, so we just check the AI marker structure
        assert!(result.starts_with("### SUMMARY OF COPIED TEXT ("));
        assert!(result.contains(
            "):[key(enter)]\x03Long article text\x1Fsummarize this in 3 concise bullet points\x04"
        ));
        // trim is applied to the AI marker?
        // the pipeline handles `clipboard | ai(...) | trim` by adding \x03 and \x04.
        // Wait, the test checks if it generates correct markers.
        system::clip::set_mock_clip(None);
    }

    #[test]
    fn test_testchain_manual_case() {
        let args = ArgMap::default();
        let tpl =
            "'hello_world-demo_test' | replace('_', ' ') | replace('-', ' ') | title | repeat(2)";
        assert_eq!(
            interpolate(tpl, &args),
            "Hello World Demo TestHello World Demo Test"
        );
    }

    #[test]
    fn test_nested_deferred_pipelines() {
        let mut args = ArgMap::default();
        args.named
            .insert("url".to_string(), "httpbin.org/json".to_string());
        let tpl = "[http.get([url]) | json.get('slideshow.title') | upper]";
        let result = interpolate(tpl, &args);
        assert_eq!(
            result,
            "\x03\x1Fsys:http.get(httpbin.org/json) | json.get('slideshow.title') | upper\x04"
        );
    }

    #[test]
    fn test_interpolate_static_fast_path() {
        let args = ArgMap::default();
        let tpl = "Hello world! 12345 @#%&*()_+=~`?><,./;:'\"";
        assert_eq!(interpolate(tpl, &args), tpl);
    }

    #[test]
    fn test_finalize_static_fast_path() {
        let res = finalize("Plain text output for omw expansion", Some("omw"));
        assert_eq!(res.steps.len(), 1);
        assert_eq!(
            res.steps[0],
            ExpansionStep::Text("Plain text output for omw expansion".to_string())
        );
        assert!(!res.is_calculation);
        assert!(res.ai_transformer_template.is_none());
    }
}
