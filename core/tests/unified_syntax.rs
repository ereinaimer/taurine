use nite_core::engine::variables::plan::ExecutionPlan;
use nite_core::engine::variables::tags::find_next_tag;
use nite_core::engine::variables::types::{ArgMap, ExpansionOrigin, ExpansionStep};
use nite_core::engine::variables::validate_system_call;

#[test]
fn test_inner_tag_whitespace_tolerance() {
    let text = "[ ip ]";
    let tag = find_next_tag(text, 0).expect("tag should be found");
    assert_eq!(&text[tag.start..=tag.end], "[ ip ]");

    let plan = ExecutionPlan::compile("[  ip  ]");
    let expansion = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert!(!expansion.steps.is_empty());
}

#[test]
fn test_dynamic_variable_mandatory_default() {
    // Declared with default: [$name = "Alex"]
    let plan = ExecutionPlan::compile("Hello, [$name = \"Alex\"]!");
    let expansion = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        expansion.steps,
        vec![ExpansionStep::Text("Hello, Alex!".to_string())]
    );

    // Declared without quotes: [$name = Alex]
    let plan2 = ExecutionPlan::compile("Hello, [$name = Alex]!");
    let exp2 = plan2.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp2.steps,
        vec![ExpansionStep::Text("Hello, Alex!".to_string())]
    );

    // Empty default allowed: [$notes = ""]
    let plan3 = ExecutionPlan::compile("Note: [ $notes = \"\" ]");
    let exp3 = plan3.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp3.steps, vec![ExpansionStep::Text("Note: ".to_string())]);

    // Re-use of declared variable
    let plan4 = ExecutionPlan::compile("[$first = \"John\"] [$first]");
    let exp4 = plan4.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp4.steps,
        vec![ExpansionStep::Text("John John".to_string())]
    );
}

#[test]
fn test_dynamic_variable_passed_via_args() {
    let mut args = ArgMap::default();
    args.named.insert("user".to_string(), "Sam".to_string());
    let plan = ExecutionPlan::compile("Hi [$user = \"Default\"]");
    let exp = plan.evaluate(&args, None, ExpansionOrigin::User);
    assert_eq!(exp.steps, vec![ExpansionStep::Text("Hi Sam".to_string())]);
}

#[test]
fn test_bare_physical_keys() {
    let plan = ExecutionPlan::compile("hello[enter]world[tab][ctrl+s]");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp.steps,
        vec![
            ExpansionStep::Text("hello".to_string()),
            ExpansionStep::KeyPress("enter".to_string()),
            ExpansionStep::Text("world".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::KeyPress("ctrl+s".to_string()),
        ]
    );
}

#[test]
fn test_physical_key_repeats() {
    let plan = ExecutionPlan::compile("indent[tab 3]done");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp.steps,
        vec![
            ExpansionStep::Text("indent".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::Text("done".to_string()),
        ]
    );
}

#[test]
fn test_system_date_and_time_roots() {
    // [date]
    let plan = ExecutionPlan::compile("[date]");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp.steps.len(), 1);
    if let ExpansionStep::Text(ref s) = exp.steps[0] {
        // YYYY-MM-DD has len 10
        assert_eq!(s.len(), 10);
    } else {
        panic!("expected text step");
    }

    // [time]
    let plan_time = ExecutionPlan::compile("[time]");
    let exp_time = plan_time.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp_time.steps.len(), 1);
    if let ExpansionStep::Text(ref s) = exp_time.steps[0] {
        // HH:mm has len 5
        assert_eq!(s.len(), 5);
    } else {
        panic!("expected text step");
    }

    // Order-independent arguments for [date]: [date tomorrow YYYY-MM-DD] vs [date YYYY-MM-DD tomorrow]
    let plan_d1 = ExecutionPlan::compile("[date tomorrow YYYY-MM-DD]");
    let exp_d1 = plan_d1.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    let plan_d2 = ExecutionPlan::compile("[date YYYY-MM-DD tomorrow]");
    let exp_d2 = plan_d2.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp_d1.steps, exp_d2.steps);

    // Order-independent arguments for [time]: [time utc HH:mm:ss] vs [time HH:mm:ss utc]
    let plan_t1 = ExecutionPlan::compile("[time utc HH:mm:ss]");
    let exp_t1 = plan_t1.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    let plan_t2 = ExecutionPlan::compile("[time HH:mm:ss utc]");
    let exp_t2 = plan_t2.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp_t1.steps, exp_t2.steps);
}

#[test]
fn test_system_ip_zero_variants() {
    let plan = ExecutionPlan::compile("[ip]");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp.steps.len(), 1);
    if let ExpansionStep::Text(ref s) = exp.steps[0] {
        assert!(!s.is_empty(), "ip should return non-empty string");
    } else {
        panic!("expected text step");
    }

    // Passing variants to ip is invalid
    assert!(validate_system_call("ip", Some("public")).is_err());
    assert!(validate_system_call("ip", Some("local")).is_err());
}

#[test]
fn test_run_system_variable() {
    let plan = ExecutionPlan::compile("[run \"echo hello\"]");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp.steps.len(), 1);
    match &exp.steps[0] {
        ExpansionStep::InlineRun(meta, _) => {
            assert_eq!(
                meta.behavior,
                nite_core::engine::shell::ScriptBehavior::Inline
            );
        }
        _ => panic!("expected InlineRun"),
    }

    // Silent background run: [run silent "echo hello"] and [run "echo hello" silent]
    let plan_s1 = ExecutionPlan::compile("[run silent \"echo hello\"]");
    let exp_s1 = plan_s1.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    match &exp_s1.steps[0] {
        ExpansionStep::InlineRun(meta, _) => {
            assert_eq!(
                meta.behavior,
                nite_core::engine::shell::ScriptBehavior::Silent
            );
        }
        _ => panic!("expected InlineRun silent"),
    }

    let plan_s2 = ExecutionPlan::compile("[run \"echo hello\" silent]");
    let exp_s2 = plan_s2.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    match &exp_s2.steps[0] {
        ExpansionStep::InlineRun(meta, _) => {
            assert_eq!(
                meta.behavior,
                nite_core::engine::shell::ScriptBehavior::Silent
            );
        }
        _ => panic!("expected InlineRun silent"),
    }
}

#[test]
fn test_wait_system_variable() {
    let plan = ExecutionPlan::compile("wait[wait 250ms]done");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp.steps,
        vec![
            ExpansionStep::Text("wait".to_string()),
            ExpansionStep::Delay(250),
            ExpansionStep::Text("done".to_string()),
        ]
    );

    // Bare number defaults to ms: [wait 200]
    let plan2 = ExecutionPlan::compile("[wait 200]");
    let exp2 = plan2.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp2.steps, vec![ExpansionStep::Delay(200)]);
}

#[test]
fn test_direct_calc_tag() {
    let plan = ExecutionPlan::compile("[calc 10 * 5]");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp.steps, vec![ExpansionStep::Text("50".to_string())]);
}

#[test]
fn test_direct_casing_and_trim_transformers() {
    let plan = ExecutionPlan::compile("['  hello world  ' | trim | upper]");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp.steps,
        vec![ExpansionStep::Text("HELLO WORLD".to_string())]
    );

    let plan_slug = ExecutionPlan::compile("['Hello World 2026!' | slug]");
    let exp_slug = plan_slug.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp_slug.steps,
        vec![ExpansionStep::Text("hello-world-2026".to_string())]
    );
}

#[test]
fn test_uncapped_repeat_transformer() {
    let plan = ExecutionPlan::compile("['=' | repeat 500]");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp.steps, vec![ExpansionStep::Text("=".repeat(500))]);
}

#[test]
fn test_space_separated_transformer_arguments() {
    let plan = ExecutionPlan::compile("['apple,banana,orange' | replace ',' ';']");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp.steps,
        vec![ExpansionStep::Text("apple;banana;orange".to_string())]
    );

    let plan_trunc = ExecutionPlan::compile("['abcdefghij' | truncate 4]");
    let exp_trunc = plan_trunc.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp_trunc.steps,
        vec![ExpansionStep::Text("abcd".to_string())]
    );
}

#[test]
fn test_bare_physical_keys_case_insensitivity() {
    let plan = ExecutionPlan::compile("hello[Enter]world[TAB][Ctrl+A][Tab 5]");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(
        exp.steps,
        vec![
            ExpansionStep::Text("hello".to_string()),
            ExpansionStep::KeyPress("enter".to_string()),
            ExpansionStep::Text("world".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::KeyPress("ctrl+a".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
            ExpansionStep::KeyPress("tab".to_string()),
        ]
    );
}

#[test]
fn test_wait_directive_seconds_float() {
    let plan = ExecutionPlan::compile("[wait 1.5s]");
    let exp = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp.steps, vec![ExpansionStep::Delay(1500)]);

    let plan2 = ExecutionPlan::compile("[wait 500]");
    let exp2 = plan2.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
    assert_eq!(exp2.steps, vec![ExpansionStep::Delay(500)]);
}

#[test]
fn test_all_casing_filters() {
    assert_eq!(
        ExecutionPlan::compile("['hello world' | upper]")
            .evaluate(&ArgMap::default(), None, ExpansionOrigin::User)
            .steps,
        vec![ExpansionStep::Text("HELLO WORLD".to_string())]
    );
    assert_eq!(
        ExecutionPlan::compile("['HELLO WORLD' | lower]")
            .evaluate(&ArgMap::default(), None, ExpansionOrigin::User)
            .steps,
        vec![ExpansionStep::Text("hello world".to_string())]
    );
    assert_eq!(
        ExecutionPlan::compile("['hello world' | title]")
            .evaluate(&ArgMap::default(), None, ExpansionOrigin::User)
            .steps,
        vec![ExpansionStep::Text("Hello World".to_string())]
    );
    assert_eq!(
        ExecutionPlan::compile("['hello world' | snake]")
            .evaluate(&ArgMap::default(), None, ExpansionOrigin::User)
            .steps,
        vec![ExpansionStep::Text("hello_world".to_string())]
    );
    assert_eq!(
        ExecutionPlan::compile("['hello world' | kebab]")
            .evaluate(&ArgMap::default(), None, ExpansionOrigin::User)
            .steps,
        vec![ExpansionStep::Text("hello-world".to_string())]
    );
    assert_eq!(
        ExecutionPlan::compile("['hello world' | pascal]")
            .evaluate(&ArgMap::default(), None, ExpansionOrigin::User)
            .steps,
        vec![ExpansionStep::Text("HelloWorld".to_string())]
    );
    assert_eq!(
        ExecutionPlan::compile("['hello world' | camel]")
            .evaluate(&ArgMap::default(), None, ExpansionOrigin::User)
            .steps,
        vec![ExpansionStep::Text("helloWorld".to_string())]
    );
}

#[test]
fn test_rejection_diagnostics_dot_notation_and_deleted_roots() {
    use nite_core::engine::variables::registry::validate_system_call;

    // Dot notation rejected
    assert!(validate_system_call("date.utc", None).is_err());
    assert!(validate_system_call("time.local", None).is_err());
    assert!(validate_system_call("mouse.click", None).is_err());

    // Deleted roots rejected
    assert!(validate_system_call("chrono", None).is_err());
    assert!(validate_system_call("datetime", None).is_err());
    assert!(validate_system_call("execute", None).is_err());
    assert!(validate_system_call("http", None).is_err());

    // Empty run command rejected
    assert!(validate_system_call("run", Some("")).is_err());
    assert!(validate_system_call("run", Some("\"\"")).is_err());
}
