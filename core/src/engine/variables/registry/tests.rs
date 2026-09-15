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
    assert!(roots.contains(&"chrono"));
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
        parse_system_call("chrono(date, +1d)"),
        Some(("chrono", "date, +1d"))
    );
    assert_eq!(parse_system_call("clip"), Some(("clip", "")));
    assert!(matches!(
        validate_system_call("date", Some("utc")),
        Err(ValidationError::DotChain { .. })
    ));
    assert!(validate_system_call("clipboard", None).is_err());
}

#[test]
fn parses_call_shapes() {
    assert_eq!(
        parse_system_call("random(choice, a, b)"),
        Some(("random", "choice, a, b"))
    );
    assert_eq!(parse_system_call("chrono()"), Some(("chrono", "")));
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
        "chrono", "clip", "uuid", "random", "lorem", "file", "ip", "http", "env", "execute",
        "mouse", "key", "delay", "use", "image", "cursor", "newline",
    ] {
        assert!(SYSTEM_ROOTS.contains(&root), "missing root {root}");
        assert!(param_spec(root).is_some());
    }
    assert_eq!(SYSTEM_ROOTS.len(), 17);
    for deleted in ["clipboard", "date", "time", "datetime", "net"] {
        assert!(!SYSTEM_ROOTS.contains(&deleted));
        assert!(param_spec(deleted).is_none());
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
        validate_system_call("date", None),
        Err(ValidationError::UnknownRoot("date".to_string()))
    );
    assert_eq!(
        validate_system_call("date", Some("utc")),
        Err(ValidationError::DotChain {
            got: "date.utc".to_string(),
            hint: "chrono(...)".to_string(),
        })
    );
    assert_eq!(deleted_root_hint("clipboard"), Some("clip"));
    assert_eq!(deleted_root_hint("datetime"), Some("chrono(...)"));
    assert_eq!(deleted_root_hint("net"), Some("ip"));
    assert_eq!(deleted_root_hint("clip"), None);
}

#[test]
fn rejects_dotted_namespaces_with_canonical_hint() {
    assert_eq!(
        validate_system_call("mouse.click", None),
        Err(ValidationError::DotChain {
            got: "mouse.click".to_string(),
            hint: "mouse(click, mN[, n])".to_string(),
        })
    );
    assert!(matches!(
        validate_system_call("mouse.rclick", None),
        Err(ValidationError::DotChain { hint, .. }) if hint == "mouse(click, mN[, n])"
    ));
    assert!(matches!(
        validate_system_call("mouse.dblclick", None),
        Err(ValidationError::DotChain { hint, .. }) if hint == "mouse(click, mN[, n])"
    ));
    assert!(matches!(
        validate_system_call("mouse.down", None),
        Err(ValidationError::DotChain { hint, .. }) if hint == "mouse(hold, mN)"
    ));
    assert!(matches!(
        validate_system_call("mouse.release", None),
        Err(ValidationError::DotChain { hint, .. }) if hint == "mouse(release, mN)"
    ));
    assert_eq!(
        validate_system_call("random.int", Some("1, 2")),
        Err(ValidationError::DotChain {
            got: "random.int(1, 2)".to_string(),
            hint: "random(...)".to_string(),
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
    assert_eq!(validate_system_call("chrono", Some("date, +1d")), Ok(()));
    assert_eq!(
        validate_system_call("chrono", Some("type=time, tz=utc")),
        Ok(())
    );
    assert_eq!(validate_system_call("CHRONO", Some("date")), Ok(()));
    assert_eq!(validate_system_call("env", Some("X, fallback")), Ok(()));
    assert_eq!(
        validate_system_call("env", None),
        Err(ValidationError::MissingModifier { root: "env" })
    );
    assert!(validate_system_call("env", Some("X=y")).is_err());
    assert_eq!(
        validate_system_call("execute", Some("bash")),
        Err(ValidationError::MissingModifier { root: "execute" })
    );
    assert_eq!(
        validate_system_call("execute", Some("bash, echo hi")),
        Ok(())
    );
    assert_eq!(validate_system_call("cursor", None), Ok(()));
    assert_eq!(
        validate_system_call("cursor", Some("x")),
        Err(ValidationError::UnexpectedModifier {
            root: "cursor",
            modifier: "x".to_string(),
        })
    );
}
