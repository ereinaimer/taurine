use super::*;

#[test]
fn strips_global_transformers_before_system_validation() {
    assert_eq!(
        strip_global_transformers("time.now | case(upper)"),
        "time.now"
    );
    assert_eq!(strip_global_transformers("name | case(upper)"), "name");
}

#[test]
fn test_system_variable_roots_catalog() {
    let roots = system_variable_roots();
    assert!(roots.contains(&"cursor"));
    assert!(roots.contains(&"clip"));
    assert!(roots.contains(&"date"));
    assert!(roots.contains(&"time"));
    assert!(is_valid_system_root("cursor"));
    assert!(is_valid_system_root("  CURSOR  "));
    assert!(!is_valid_system_root("bogus_root"));
}

#[test]
fn test_system_transformers_catalog() {
    let transformers = system_transformers();
    assert!(transformers.contains(&"case"));
    assert!(transformers.contains(&"lines"));
    assert!(transformers.contains(&"truncate"));
    assert!(is_valid_transformer("case(upper)"));
    assert!(is_valid_transformer("  CASE(UPPER)  "));
    assert!(is_valid_transformer("truncate(5)"));
    assert!(!is_valid_transformer("bogus_transformer"));
}

#[test]
fn parses_call_and_rejects_dots() {
    assert_eq!(
        parse_system_call("date tomorrow YYYY-MM-DD"),
        Some(("date", "tomorrow YYYY-MM-DD"))
    );
    assert_eq!(parse_system_call("clip"), Some(("clip", "")));
    assert_eq!(
        parse_system_call("run \"echo hi\""),
        Some(("run", "\"echo hi\""))
    );
    assert!(validate_system_call("clipboard", None).is_err());
}

#[test]
fn parses_call_shapes() {
    assert_eq!(
        parse_system_call("random(choice, a, b)"),
        Some(("random", "choice, a, b"))
    );
    assert_eq!(parse_system_call("date"), Some(("date", "")));
    assert_eq!(parse_system_call("  clip(1)  "), Some(("clip", "1")));
    assert_eq!(parse_system_call("clip | case(upper)"), Some(("clip", "")));
    assert_eq!(
        parse_system_call("env(X, \"a|b\")"),
        Some(("env", "X, \"a|b\""))
    );
    assert_eq!(parse_system_call(""), None);
    assert_eq!(parse_system_call("   "), None);
    assert_eq!(parse_system_call("clip(1"), None);
    assert_eq!(parse_system_call("()"), None);
    assert_eq!(parse_system_call("(1)"), None);
}

#[test]
fn system_roots_follow_unified_catalog() {
    for root in [
        "date", "time", "clip", "uuid", "random", "lorem", "file", "ip", "env", "run", "mouse",
        "wait", "use", "image", "cursor", "newline", "calc",
    ] {
        assert!(SYSTEM_ROOTS.contains(&root), "missing root {root}");
    }
    assert_eq!(SYSTEM_ROOTS.len(), 17);
    for deleted in ["clipboard", "chrono", "http", "datetime", "net"] {
        assert!(!SYSTEM_ROOTS.contains(&deleted));
    }
}

#[test]
fn rejects_unknown_and_deleted_roots() {
    assert_eq!(
        validate_system_call("frobnicate", None),
        Err(ValidationError::UnknownRoot("frobnicate".to_string()))
    );
    assert_eq!(
        validate_system_call("clipboard", None),
        Err(ValidationError::UnknownRoot("clipboard".to_string()))
    );
    assert!(validate_system_call("clipboard", Some("1")).is_err());
    assert_eq!(
        validate_system_call("chrono", None),
        Err(ValidationError::UnknownRoot("chrono".to_string()))
    );
    assert_eq!(deleted_root_hint("clipboard"), Some("clip"));
    assert_eq!(deleted_root_hint("chrono"), Some("date or time"));
    assert_eq!(deleted_root_hint("datetime"), Some("date or time"));
    assert_eq!(deleted_root_hint("http"), Some("run \"curl ...\""));
    assert_eq!(deleted_root_hint("net"), Some("ip"));
    assert_eq!(deleted_root_hint("clip"), None);
}

#[test]
fn rejects_dotted_namespaces_with_canonical_hint() {
    assert_eq!(
        validate_system_call("mouse.click", None),
        Err(ValidationError::DotChain {
            got: "mouse.click".to_string(),
            hint: "mouse click m1".to_string(),
        })
    );
    assert!(matches!(
        validate_system_call("mouse.rclick", None),
        Err(ValidationError::DotChain { hint, .. }) if hint == "mouse click m2"
    ));
    assert!(matches!(
        validate_system_call("mouse.dblclick", None),
        Err(ValidationError::DotChain { hint, .. }) if hint == "mouse click m1 2"
    ));
    assert!(matches!(
        validate_system_call("mouse.down", None),
        Err(ValidationError::DotChain { hint, .. }) if hint == "mouse hold m1"
    ));
    assert!(matches!(
        validate_system_call("mouse.release", None),
        Err(ValidationError::DotChain { hint, .. }) if hint == "mouse release m1"
    ));
    assert!(matches!(
        validate_system_call("mouse.frobnicate", None),
        Err(ValidationError::DotChain { hint, .. }) if hint == "mouse click mN [n]"
    ));
    assert_eq!(
        validate_system_call("random.int", Some("1, 2")),
        Err(ValidationError::DotChain {
            got: "random.int 1, 2".to_string(),
            hint: "random ...".to_string(),
        })
    );
}

#[test]
fn validates_mouse_call_buttons() {
    assert_eq!(validate_system_call("mouse", Some("click, m1")), Ok(()));
    assert_eq!(validate_system_call("mouse", Some("click, m2, 2")), Ok(()));
    assert_eq!(validate_system_call("mouse", Some("hold, m5")), Ok(()));
    assert_eq!(validate_system_call("mouse", Some("pos")), Ok(()));
    assert_eq!(
        validate_system_call("mouse", None),
        Err(ValidationError::MissingModifier { root: "mouse" })
    );
    assert_eq!(
        validate_system_call("mouse", Some("click")),
        Err(ValidationError::MissingModifier { root: "mouse" })
    );
    for btn in [
        "1",
        "left",
        "right",
        "middle",
        "back",
        "m0",
        "m256",
        "nonextent",
    ] {
        assert!(
            matches!(
                validate_system_call("mouse", Some(&format!("click, {btn}"))),
                Err(ValidationError::InvalidModifier { allowed, .. })
                    if allowed == ["m1..mN"]
            ),
            "button {btn} must hint m1..mN"
        );
    }
    assert!(validate_system_call("mouse", Some("dblclick")).is_err());
}

#[test]
fn validates_call_binding_structurally() {
    assert_eq!(validate_system_call("clip", Some("1")), Ok(()));
    assert!(validate_system_call("clip", Some("1, 2")).is_err());
    assert_eq!(validate_system_call("date", Some("+1d")), Ok(()));
    assert_eq!(validate_system_call("time", Some("utc")), Ok(()));
    assert_eq!(validate_system_call("DATE", Some("tomorrow")), Ok(()));
    assert_eq!(validate_system_call("env", Some("X fallback")), Ok(()));
    assert_eq!(
        validate_system_call("env", None),
        Err(ValidationError::MissingModifier { root: "env" })
    );
    assert_eq!(
        validate_system_call("run", None),
        Err(ValidationError::MissingModifier { root: "run" })
    );
    assert_eq!(validate_system_call("run", Some("\"echo hi\"")), Ok(()));
    assert_eq!(validate_system_call("cursor", None), Ok(()));
    assert_eq!(
        validate_system_call("cursor", Some("x")),
        Err(ValidationError::UnexpectedModifier {
            root: "cursor",
            modifier: "x".to_string(),
        })
    );
}

#[test]
fn rejects_out_of_range_scalar_values() {
    for (ns, raw) in [
        ("clip", "9"),
        ("clip", "abc"),
        ("clip", "-1"),
        ("uuid", "7"),
        ("uuid", "v1"),
        ("uuid", "V4"),
        ("ip", "online"),
        ("ip", "PUBLIC"),
        ("ip", "bogus"),
        ("date", "1d"),
        ("lorem", "bogus"),
        ("lorem", "words, nope"),
        ("lorem", "words, 0"),
        ("lorem", "words, 100001"),
        ("file", "bogus, p"),
        ("file", "read, p, extra"),
        ("file", "line, p"),
        ("file", "lines, p, 5, 2"),
        ("file", "line, p, 0"),
        ("random", "bogus"),
        ("random", "int, 10, 5"),
        ("random", "choice"),
        ("random", "str, 0"),
        ("random", "str, 5000"),
        ("random", "6, 7, 8"),
        ("mouse", "move, 10"),
        ("mouse", "scroll, abc"),
        ("mouse", "pos, m1"),
        ("mouse", "click, m1, abc"),
        ("mouse", "hold, m1, 2"),
        ("mouse", "click, left"),
        ("wait", "abc"),
        ("wait", "1.5x"),
        ("wait", "-5s"),
    ] {
        assert!(
            validate_system_call(ns, Some(raw)).is_err(),
            "expected error for {ns}({raw})"
        );
    }
    for (ns, raw) in [
        ("clip", ""),
        ("clip", "1"),
        ("uuid", ""),
        ("uuid", "v7"),
        ("ip", ""),
        ("date", "YYYY-MM-DD"),
        ("date", "+1d"),
        ("date", "tomorrow"),
        ("time", "utc"),
        ("time", "HH:mm"),
        ("lorem", "words, 5"),
        ("lorem", "words, 100000"),
        ("lorem", "3"),
        ("file", "read, p"),
        ("file", "line, p, 2"),
        ("file", "lines, p, 1, 5"),
        ("random", ""),
        ("random", "6"),
        ("random", "int, 1, 6"),
        ("random", "choice, a, b"),
        ("random", "str, 8"),
        ("run", "\"echo hi\""),
        ("run", "silent \"echo hi\""),
        ("mouse", "click, m1"),
        ("mouse", "click, m2, 2"),
        ("mouse", "click, m1, 0"),
        ("mouse", "hold, m1"),
        ("mouse", "move, 1, 2"),
        ("mouse", "scroll, -3"),
        ("mouse", "pos"),
        ("wait", "200ms"),
        ("wait", "1.5s"),
        ("wait", "100"),
    ] {
        assert!(
            validate_system_call(ns, Some(raw)).is_ok(),
            "expected ok for {ns}({raw})"
        );
    }
}
