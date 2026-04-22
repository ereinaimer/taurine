use super::system;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    UnknownRoot(String),
    MissingModifier {
        root: &'static str,
    },
    UnexpectedModifier {
        root: &'static str,
        modifier: String,
    },
    InvalidModifier {
        root: &'static str,
        modifier: String,
        allowed: &'static [&'static str],
    },
}

const SYSTEM_ROOTS: &[&str] = &[
    "cursor",
    "clipboard",
    "time",
    "date",
    "uuid",
    "env",
    "sys",
    "run",
    "key",
    "delay",
];

const TIME_MODIFIERS: &[&str] = &[
    "greeting", "epoch", "unix", "utc", "tz", "12h", "24h", "now", "now.12h", "now.24h", "full",
    "full.12h", "full.24h",
];

const DATE_MODIFIERS: &[&str] = &[
    "iso",
    "short",
    "long",
    "tomorrow",
    "tomorrow.iso",
    "tomorrow.short",
    "tomorrow.long",
    "yesterday",
    "yesterday.iso",
    "yesterday.short",
    "yesterday.long",
    "weekday",
    "year",
    "month",
    "month_name",
    "day",
];

const UUID_MODIFIERS: &[&str] = &["v4", "v7", "simple"];
const SYS_MODIFIERS: &[&str] = &["os", "osversion", "arch", "hostname", "user"];
const RUN_LANGUAGES: &[&str] = &["bash", "powershell", "python", "node", "node_esm", "cmd"];
const RUN_MODIFIERS: &[&str] = &[
    "run.<lang>(...)",
    "run.silent.<lang>(...)",
    "run.<lang>.file(...).args(...)",
];
const KEY_MODIFIERS: &[&str] = &[
    "enter",
    "tab",
    "space",
    "esc",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pgup",
    "pageup",
    "pgdown",
    "pagedown",
    "insert",
    "ins",
    "backspace",
    "delete",
    "ctrl",
    "shift",
    "alt",
    "super",
    "mod",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "printscreen",
    "prtsc",
    "pause",
    "break",
    "capslock",
    "numlock",
    "scrolllock",
];

pub fn strip_global_transformers(mut key: &str) -> &str {
    while let Some((sub, _)) = system::split_modifier(key) {
        key = sub;
    }
    key
}

pub fn split_system_tag(key: &str) -> Option<(&str, Option<&str>)> {
    let base = strip_global_transformers(key);
    let (root, modifier) = match base.split_once('.') {
        Some((root, modifier)) => (root, Some(modifier.trim()).filter(|m| !m.is_empty())),
        None => (base, None),
    };

    SYSTEM_ROOTS.contains(&root).then_some((root, modifier))
}

pub fn valid_modifier_hint(root: &str) -> String {
    match root {
        "cursor" => "Valid form: [cursor]".to_string(),
        "clipboard" => "Valid form: [clipboard]".to_string(),
        "time" => format!("Valid modifiers: {}", TIME_MODIFIERS.join(", ")),
        "date" => format!("Valid modifiers: {}", DATE_MODIFIERS.join(", ")),
        "uuid" => format!("Valid modifiers: uuid, {}", UUID_MODIFIERS.join(", ")),
        "env" => "Valid form: [env.VAR_NAME]".to_string(),
        "sys" => format!("Valid modifiers: {}", SYS_MODIFIERS.join(", ")),
        "run" => "Valid form: [run.<lang>(...)] or [run.<lang>.file(...).args(...)]. Languages: bash, powershell, python, node, node_esm, cmd".to_string(),
        "key" => format!(
            "Valid key tokens: {}. You can combine them with `+`, and any single character token is also allowed.",
            KEY_MODIFIERS.join(", ")
        ),
        "delay" => "Valid form: [delay.<u64>ms]".to_string(),
        _ => "No modifier help available.".to_string(),
    }
}

pub fn validate_system_tag(root: &str, modifier: Option<&str>) -> Result<(), ValidationError> {
    match root {
        "cursor" => validate_no_modifier("cursor", modifier),
        "clipboard" => validate_no_modifier("clipboard", modifier),
        "time" => validate_known_modifier("time", modifier, TIME_MODIFIERS),
        "date" => validate_known_modifier("date", modifier, DATE_MODIFIERS),
        "uuid" => validate_optional_known_modifier("uuid", modifier, UUID_MODIFIERS),
        "env" => validate_env_modifier(modifier),
        "sys" => validate_known_modifier("sys", modifier, SYS_MODIFIERS),
        "run" => validate_run_modifier(modifier),
        "key" => validate_key_modifier(modifier),
        "delay" => validate_delay_modifier(modifier),
        _ => Err(ValidationError::UnknownRoot(root.to_string())),
    }
}

fn validate_no_modifier(root: &'static str, modifier: Option<&str>) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(modifier) => Err(ValidationError::UnexpectedModifier {
            root,
            modifier: modifier.to_string(),
        }),
    }
}

fn validate_known_modifier(
    root: &'static str,
    modifier: Option<&str>,
    allowed: &'static [&'static str],
) -> Result<(), ValidationError> {
    let modifier = normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root })?)
        .ok_or(ValidationError::MissingModifier { root })?;

    if allowed.contains(&modifier) {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifier {
            root,
            modifier: modifier.to_string(),
            allowed,
        })
    }
}

fn validate_optional_known_modifier(
    root: &'static str,
    modifier: Option<&str>,
    allowed: &'static [&'static str],
) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(modifier) if allowed.contains(&modifier) => Ok(()),
        Some(modifier) => Err(ValidationError::InvalidModifier {
            root,
            modifier: modifier.to_string(),
            allowed,
        }),
    }
}

fn validate_env_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    if normalize_modifier(modifier.unwrap_or_default()).is_some() {
        Ok(())
    } else {
        Err(ValidationError::MissingModifier { root: "env" })
    }
}

fn validate_run_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "run" })?)
            .ok_or(ValidationError::MissingModifier { root: "run" })?;

    let mut rest = modifier;
    if let Some(suffix) = rest.strip_prefix("silent.") {
        rest = suffix;
    }

    let (language, after_language) =
        parse_run_language(rest).ok_or_else(|| ValidationError::InvalidModifier {
            root: "run",
            modifier: modifier.to_string(),
            allowed: RUN_MODIFIERS,
        })?;

    if !RUN_LANGUAGES.contains(&language) {
        return Err(ValidationError::InvalidModifier {
            root: "run",
            modifier: modifier.to_string(),
            allowed: RUN_MODIFIERS,
        });
    }

    let after_file = after_language
        .strip_prefix(".file")
        .unwrap_or(after_language);
    let (_, trailing) =
        scan_run_parenthesized(after_file).ok_or_else(|| ValidationError::InvalidModifier {
            root: "run",
            modifier: modifier.to_string(),
            allowed: RUN_MODIFIERS,
        })?;

    let trailing = if let Some(args) = trailing.strip_prefix(".args") {
        let (_, trailing) =
            scan_run_parenthesized(args).ok_or_else(|| ValidationError::InvalidModifier {
                root: "run",
                modifier: modifier.to_string(),
                allowed: RUN_MODIFIERS,
            })?;
        trailing
    } else {
        trailing
    };

    if trailing.trim().is_empty() {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifier {
            root: "run",
            modifier: modifier.to_string(),
            allowed: RUN_MODIFIERS,
        })
    }
}

fn parse_run_language(input: &str) -> Option<(&str, &str)> {
    for language in RUN_LANGUAGES {
        if let Some(rest) = input.strip_prefix(language)
            && (rest.starts_with('(') || rest.starts_with(".file"))
        {
            return Some((language, rest));
        }
    }
    None
}

fn scan_run_parenthesized(input: &str) -> Option<(&str, &str)> {
    if !input.starts_with('(') {
        return None;
    }

    let mut depth = 0usize;
    let mut start = None;

    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => {
                if depth == 0 {
                    start = Some(idx + ch.len_utf8());
                }
                depth += 1;
            }
            ')' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    let start = start?;
                    return Some((&input[start..idx], &input[idx + 1..]));
                }
            }
            _ => {}
        }
    }

    None
}

fn validate_key_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "key" })?)
            .ok_or(ValidationError::MissingModifier { root: "key" })?;

    for token in modifier.split('+') {
        let token = token.trim();
        let normalized = token.to_ascii_lowercase();
        let is_known_special = KEY_MODIFIERS.contains(&normalized.as_str());
        let is_single_char = token.chars().count() == 1;

        if token.is_empty() || (!is_known_special && !is_single_char) {
            return Err(ValidationError::InvalidModifier {
                root: "key",
                modifier: modifier.to_string(),
                allowed: KEY_MODIFIERS,
            });
        }
    }

    Ok(())
}

fn validate_delay_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "delay" })?)
            .ok_or(ValidationError::MissingModifier { root: "delay" })?;

    if parse_delay_ms(modifier).is_some() {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifier {
            root: "delay",
            modifier: modifier.to_string(),
            allowed: &["<u64>ms"],
        })
    }
}

fn normalize_modifier(modifier: &str) -> Option<&str> {
    let trimmed = modifier.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn parse_delay_ms(s: &str) -> Option<u64> {
    s.trim()
        .strip_suffix("ms")
        .and_then(|n| n.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_global_transformers_before_system_validation() {
        assert_eq!(strip_global_transformers("time.now.upper"), "time.now");
        assert_eq!(strip_global_transformers("name.upper"), "name");
    }

    #[test]
    fn splits_known_system_roots_only() {
        assert_eq!(
            split_system_tag("time.now.upper"),
            Some(("time", Some("now")))
        );
        assert_eq!(split_system_tag("clipboard"), Some(("clipboard", None)));
        assert_eq!(split_system_tag("query.upper"), None);
    }

    #[test]
    fn validates_time_modifiers_from_resolver_match_arms() {
        for modifier in TIME_MODIFIERS {
            assert_eq!(validate_system_tag("time", Some(modifier)), Ok(()));
        }
        assert_eq!(
            validate_system_tag("time", Some("india")),
            Err(ValidationError::InvalidModifier {
                root: "time",
                modifier: "india".to_string(),
                allowed: TIME_MODIFIERS,
            })
        );
    }

    #[test]
    fn validates_date_modifiers_from_resolver_match_arms() {
        for modifier in DATE_MODIFIERS {
            assert_eq!(validate_system_tag("date", Some(modifier)), Ok(()));
        }
        assert_eq!(
            validate_system_tag("date", Some("tomorrow.india")),
            Err(ValidationError::InvalidModifier {
                root: "date",
                modifier: "tomorrow.india".to_string(),
                allowed: DATE_MODIFIERS,
            })
        );
    }

    #[test]
    fn validates_uuid_modifiers_and_optional_default() {
        assert_eq!(validate_system_tag("uuid", None), Ok(()));
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
    fn validates_roots_with_no_modifiers() {
        assert_eq!(validate_system_tag("cursor", None), Ok(()));
        assert_eq!(validate_system_tag("clipboard", None), Ok(()));
        assert_eq!(
            validate_system_tag("cursor", Some("now")),
            Err(ValidationError::UnexpectedModifier {
                root: "cursor",
                modifier: "now".to_string(),
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
    fn test_validate_sys_known_modifier() {
        for modifier in SYS_MODIFIERS {
            assert_eq!(validate_system_tag("sys", Some(modifier)), Ok(()));
        }
    }

    #[test]
    fn test_validate_sys_unknown_modifier() {
        assert_eq!(
            validate_system_tag("sys", Some("home")),
            Err(ValidationError::InvalidModifier {
                root: "sys",
                modifier: "home".to_string(),
                allowed: SYS_MODIFIERS,
            })
        );
        assert_eq!(
            validate_system_tag("sys", None),
            Err(ValidationError::MissingModifier { root: "sys" })
        );
    }

    #[test]
    fn validates_run_modifier_syntax() {
        assert_eq!(validate_system_tag("run", Some("bash(echo 42)")), Ok(()));
        assert_eq!(
            validate_system_tag("run", Some("silent.bash(echo start)")),
            Ok(())
        );
        assert_eq!(
            validate_system_tag("run", Some("bash.file(/tmp/test.sh).args(arg1, arg2)")),
            Ok(())
        );
        assert_eq!(
            validate_system_tag("run", Some("node_esm(console.log((1 + 2)))")),
            Ok(())
        );
        assert_eq!(
            validate_system_tag("run", Some("ruby(puts 1)")),
            Err(ValidationError::InvalidModifier {
                root: "run",
                modifier: "ruby(puts 1)".to_string(),
                allowed: RUN_MODIFIERS,
            })
        );
        assert_eq!(
            validate_system_tag("run", Some("bash(echo 1")),
            Err(ValidationError::InvalidModifier {
                root: "run",
                modifier: "bash(echo 1".to_string(),
                allowed: RUN_MODIFIERS,
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
}
