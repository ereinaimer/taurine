use super::*;

#[test]
fn strips_global_transformers_before_system_validation() {
    assert_eq!(strip_global_transformers("time.now | upper"), "time.now");
    assert_eq!(strip_global_transformers("name | upper"), "name");
}

#[test]
fn splits_known_system_roots_only() {
    assert_eq!(
        split_system_tag("time.now | upper"),
        Some(("time", Some("now")))
    );
    assert_eq!(
        split_system_tag("net.hostname | upper"),
        Some(("net", Some("hostname")))
    );
    assert_eq!(split_system_tag("clip"), Some(("clip", None)));
    assert_eq!(split_system_tag("clip(1)"), Some(("clip", None)));
    assert_eq!(split_system_tag("clip(2) | upper"), Some(("clip", None)));
    assert_eq!(split_system_tag("newline"), Some(("newline", None)));
    assert_eq!(split_system_tag("query | upper"), None);
}

#[test]
fn validates_time_modifiers_from_resolver_match_arms() {
    assert_eq!(validate_system_tag("time", None), Ok(()));
    assert_eq!(validate_system_tag("time", Some("utc")), Ok(()));
    assert_eq!(
        validate_system_tag("time", Some("utc.format(HH:mm)")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("time", Some("india")),
        Err(ValidationError::InvalidModifier {
            root: "time",
            modifier: "india".to_string(),
            allowed: TIME_METHODS,
        })
    );
}

#[test]
fn validates_newline_modifiers() {
    assert_eq!(validate_system_tag("newline", None), Ok(()));
    assert!(validate_system_tag("newline", Some("invalid")).is_err());
}

#[test]
fn validates_date_modifiers_from_resolver_match_arms() {
    assert_eq!(validate_system_tag("date", None), Ok(()));
    assert_eq!(validate_system_tag("date", Some("calc(+1d)")), Ok(()));
    assert_eq!(
        validate_system_tag("date", Some("tomorrow.india")),
        Err(ValidationError::InvalidModifier {
            root: "date",
            modifier: "tomorrow.india".to_string(),
            allowed: DATE_METHODS,
        })
    );
}

#[test]
fn validates_uuid_modifiers() {
    assert_eq!(
        validate_system_tag("uuid", None),
        Err(ValidationError::MissingModifier { root: "uuid" })
    );
    for modifier in UUID_MODIFIERS {
        assert_eq!(validate_system_tag("uuid", Some(modifier)), Ok(()));
    }
    assert_eq!(
        validate_system_tag("uuid", Some("v1")),
        Err(ValidationError::InvalidModifier {
            root: "uuid",
            modifier: "v1".to_string(),
            allowed: UUID_MODIFIERS,
        })
    );
}

#[test]
fn validates_net_modifiers() {
    assert_eq!(validate_system_tag("net", Some("ip")), Ok(()));
    assert_eq!(validate_system_tag("net", Some("lip")), Ok(()));
    assert_eq!(validate_system_tag("net", Some("online")), Ok(()));
    assert_eq!(validate_system_tag("net", Some("port(8080)")), Ok(()));

    assert_eq!(
        validate_system_tag("net", None),
        Err(ValidationError::MissingModifier { root: "net" })
    );
    assert_eq!(
        validate_system_tag("net", Some("hostname")),
        Err(ValidationError::InvalidModifier {
            root: "net",
            modifier: "hostname".to_string(),
            allowed: NET_MODIFIERS,
        })
    );
}

#[test]
fn validates_roots_with_no_modifiers() {
    assert_eq!(validate_system_tag("cursor", None), Ok(()));
    assert_eq!(validate_system_tag("clip", None), Ok(()));
    assert_eq!(
        validate_system_tag("cursor", Some("now")),
        Err(ValidationError::UnexpectedModifier {
            root: "cursor",
            modifier: "now".to_string(),
        })
    );
}

#[test]
fn validates_clip_syntax() {
    assert_eq!(validate_system_tag("clip", None), Ok(()));
    assert_eq!(validate_system_tag("clip", Some("(0)")), Ok(()));
    assert_eq!(validate_system_tag("clip", Some("(1)")), Ok(()));
    assert_eq!(validate_system_tag("clip", Some("(2)")), Ok(()));

    assert_eq!(
        validate_system_tag("clip", Some("unknown")),
        Err(ValidationError::InvalidModifier {
            root: "clip",
            modifier: "unknown".to_string(),
            allowed: &["(0)", "(1)", "(2)"],
        })
    );
}

#[test]
fn validates_env_as_dynamic_key() {
    assert_eq!(validate_system_tag("env", Some("TAURINE_HOME")), Ok(()));
    assert_eq!(validate_system_tag("env", Some(" USERPROFILE ")), Ok(()));
    assert_eq!(
        validate_system_tag("env", None),
        Err(ValidationError::MissingModifier { root: "env" })
    );
}

#[test]
fn test_validate_env_modifier() {
    assert_eq!(validate_system_tag("env", Some("PATH")), Ok(()));
    assert_eq!(validate_system_tag("env", Some("\"PATH\"")), Ok(()));
    assert_eq!(
        validate_system_tag("env", None),
        Err(ValidationError::MissingModifier { root: "env" })
    );
}

#[test]
fn validates_exec_modifier_syntax() {
    // Standard forms
    assert_eq!(validate_system_tag("exec", Some("bash(echo 42)")), Ok(()));
    assert_eq!(
        validate_system_tag("exec", Some("silent.bash(echo start)")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("exec", Some("bash.file(/tmp/test.sh).args(arg1, arg2)")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("exec", Some("node_esm(console.log((1 + 2)))")),
        Ok(())
    );

    // Order-independence: language can come after .file()
    assert_eq!(
        validate_system_tag("exec", Some("file(/tmp/test.sh).bash")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("exec", Some("file(/tmp/test.sh).bash.args(a, b)")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("exec", Some("file(/tmp/test.sh).python.silent")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("exec", Some("file(/tmp/test.sh).silent.bash")),
        Ok(())
    );

    // Order-independence: .silent after the language or subject
    assert_eq!(
        validate_system_tag("exec", Some("bash(echo 1).silent")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("exec", Some("bash(echo 1).silent.args(a, b)")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("exec", Some("file(/tmp/test.sh).python.silent.args(a, b)")),
        Ok(())
    );

    // Error cases remain the same
    assert_eq!(
        validate_system_tag("exec", Some("ruby(puts 1)")),
        Err(ValidationError::InvalidModifier {
            root: "exec",
            modifier: "ruby(puts 1)".to_string(),
            allowed: EXEC_MODIFIERS,
        })
    );
    assert_eq!(
        validate_system_tag("exec", Some("bash(echo 1")),
        Err(ValidationError::InvalidModifier {
            root: "exec",
            modifier: "bash(echo 1".to_string(),
            allowed: EXEC_MODIFIERS,
        })
    );
}

#[test]
fn validates_random_modifier_syntax() {
    assert_eq!(validate_system_tag("random", Some("int")), Ok(()));
    assert_eq!(validate_system_tag("random", Some("int()")), Ok(()));
    assert_eq!(validate_system_tag("random", Some("int(1, 2)")), Ok(()));
    assert_eq!(
        validate_system_tag("random", Some("choice(alpha(one, two), beta)")),
        Ok(())
    );
    assert_eq!(validate_system_tag("random", Some("str(8)")), Ok(()));
    assert_eq!(validate_system_tag("random", Some("hex(8)")), Ok(()));
    assert_eq!(validate_system_tag("random", Some("pass(8)")), Ok(()));

    assert_eq!(
        validate_system_tag("random", Some("int(1)")),
        Err(ValidationError::InvalidModifier {
            root: "random",
            modifier: "int(1)".to_string(),
            allowed: RANDOM_MODIFIERS,
        })
    );
    assert_eq!(
        validate_system_tag("random", Some("choice")),
        Err(ValidationError::InvalidModifier {
            root: "random",
            modifier: "choice".to_string(),
            allowed: RANDOM_MODIFIERS,
        })
    );
    assert_eq!(
        validate_system_tag("random", Some("uuid")),
        Err(ValidationError::InvalidModifier {
            root: "random",
            modifier: "uuid".to_string(),
            allowed: RANDOM_MODIFIERS,
        })
    );
}

#[test]
fn validates_lorem_modifier_syntax() {
    assert_eq!(validate_system_tag("lorem", Some("word(3)")), Ok(()));
    assert_eq!(validate_system_tag("lorem", Some("word()")), Ok(()));
    assert_eq!(validate_system_tag("lorem", Some("sentence(2)")), Ok(()));
    assert_eq!(validate_system_tag("lorem", Some("paragraph(1)")), Ok(()));
    assert_eq!(validate_system_tag("lorem", Some("word([num=5])")), Ok(()));
    assert_eq!(
        validate_system_tag("lorem", Some("word([random.int(3, 3)])")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("lorem", None),
        Err(ValidationError::MissingModifier { root: "lorem" })
    );
    assert_eq!(
        validate_system_tag("lorem", Some("paragraph(nope)")),
        Ok(())
    );

    assert_eq!(
        validate_system_tag("lorem", Some("word")),
        Err(ValidationError::InvalidModifier {
            root: "lorem",
            modifier: "word".to_string(),
            allowed: LOREM_MODIFIERS,
        })
    );
    assert_eq!(
        validate_system_tag("lorem", Some("word(1, 2)")),
        Err(ValidationError::InvalidModifier {
            root: "lorem",
            modifier: "word(1, 2)".to_string(),
            allowed: LOREM_MODIFIERS,
        })
    );
}

#[test]
fn validates_key_against_explicit_whitelist() {
    for modifier in KEY_MODIFIERS {
        assert_eq!(validate_system_tag("key", Some(modifier)), Ok(()));
    }
    assert_eq!(validate_system_tag("key", Some("Ctrl+Shift+End")), Ok(()));
    assert_eq!(validate_system_tag("key", Some("ctrl+a+p")), Ok(()));
    assert_eq!(validate_system_tag("key", Some("shift+tab")), Ok(()));
    assert_eq!(
        validate_system_tag("key", Some("not_a_real_key")),
        Err(ValidationError::InvalidModifier {
            root: "key",
            modifier: "not_a_real_key".to_string(),
            allowed: KEY_MODIFIERS,
        })
    );
    assert_eq!(
        validate_system_tag("key", None),
        Err(ValidationError::MissingModifier { root: "key" })
    );
}

#[test]
fn validates_delay_with_same_shape_as_system_parser() {
    assert_eq!(validate_system_tag("delay", Some("200ms")), Ok(()));
    assert_eq!(validate_system_tag("delay", Some(" 0ms ")), Ok(()));
    assert_eq!(
        validate_system_tag("delay", Some("200s")),
        Err(ValidationError::InvalidModifier {
            root: "delay",
            modifier: "200s".to_string(),
            allowed: &["<u64>ms"],
        })
    );
}

#[test]
fn rejects_unknown_roots() {
    assert_eq!(
        validate_system_tag("timezone", Some("utc")),
        Err(ValidationError::UnknownRoot("timezone".to_string()))
    );
}

#[test]
fn validates_img_modifier_accepts_any_nonempty_path() {
    // Any non-empty string is accepted; format validation happens at compile time when the
    // file is read, not during static template validation.
    assert_eq!(
        validate_system_tag("img", Some(r"C:\Users\aimer\Pictures\logo.png")),
        Ok(())
    );
    assert_eq!(
        validate_system_tag("img", Some("/home/user/logo.png")),
        Ok(())
    );
    // asset references are also valid path strings
    assert_eq!(validate_system_tag("img", Some("asset(abc123)")), Ok(()));
    assert_eq!(
        validate_system_tag("img", None),
        Err(ValidationError::MissingModifier { root: "img" })
    );
}

#[test]
fn split_system_tag_recognises_img_prefix() {
    assert_eq!(
        split_system_tag("img(/path/to/logo.png)"),
        Some(("img", Some("/path/to/logo.png")))
    );
    assert_eq!(
        split_system_tag(r"img(C:\Users\aimer\Pictures\Screenshots\hi.png)"),
        Some(("img", Some(r"C:\Users\aimer\Pictures\Screenshots\hi.png")))
    );
    assert_eq!(
        split_system_tag("img(asset(deadbeef))"),
        Some(("img", Some("asset(deadbeef)")))
    );
}
