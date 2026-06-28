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
    "net",
    "sys",
    "run",
    "random",
    "key",
    "delay",
    "lorem",
    "mock",
    "file",
];

const TIME_MODIFIERS: &[&str] = &[
    "greeting", "epoch", "unix", "millis", "ms", "utc", "tz", "12h", "24h", "now", "now.12h",
    "now.24h", "full", "full.12h", "full.24h", "hour", "hour.12h", "minute", "second", "am_pm",
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
    "next_week",
    "next_week.iso",
    "next_week.short",
    "next_week.long",
    "last_week",
    "last_week.iso",
    "last_week.short",
    "last_week.long",
    "next_month",
    "next_month.iso",
    "next_month.short",
    "next_month.long",
    "last_month",
    "last_month.iso",
    "last_month.short",
    "last_month.long",
    "weekday",
    "year",
    "month",
    "month_name",
    "day",
    "week",
    "quarter",
    "day_of_year",
    "days_in_month",
    "ordinal",
    "is_leap_year",
    "century",
];

const UUID_MODIFIERS: &[&str] = &["v4", "v7", "simple"];
const NET_MODIFIERS: &[&str] = &["hostname", "localip", "mac"];
const SYS_MODIFIERS: &[&str] = &["os", "osversion", "arch", "hostname", "user"];
const RUN_LANGUAGES: &[&str] = &["bash", "powershell", "python", "node", "node_esm", "cmd"];
const RUN_MODIFIERS: &[&str] = &[
    "run.<lang>(...)",
    "run.silent.<lang>(...)",
    "run.<lang>.file(...).args(...)",
];
const RANDOM_MODIFIERS: &[&str] = &[
    "int(min, max)",
    "float(min, max)",
    "bool",
    "choice(a, b, ...)",
    "string(len)",
    "alpha(len)",
    "numeric(len)",
    "hex(len)",
    "password(len)",
    "color",
    "ip",
    "mac",
];
const LOREM_MODIFIERS: &[&str] = &["words(n)", "sentence(n)", "paragraph(n)"];
const MOCK_MODIFIERS: &[&str] = &[
    "name",
    "first_name",
    "last_name",
    "title",
    "suffix",
    "address",
    "city",
    "state",
    "zip_code",
    "country",
    "latitude",
    "longitude",
    "email",
    "domain",
    "user_agent",
    "password(n)",
    "username",
    "company",
    "job_title",
    "catch_phrase",
    "bs",
    "credit_card",
    "currency_name",
    "currency_code",
    "phone_number",
    "cell_number",
    "status_code",
    "method",
];
const FILE_MODIFIERS: &[&str] = &[
    "read(path)",
    "random_line(path)",
    "read_line(path, start, [end])",
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
    if system::clipboard::is_clipboard_key(base) {
        return Some(("clipboard", None));
    }

    if let Some(rest) = base.strip_prefix("key(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("key", Some(inner)));
    }
    if let Some(rest) = base.strip_prefix("delay(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("delay", Some(inner)));
    }

    let (root, modifier) = match base.split_once('.') {
        Some((root, modifier)) => (root, Some(modifier.trim()).filter(|m| !m.is_empty())),
        None => (base, None),
    };

    SYSTEM_ROOTS.contains(&root).then_some((root, modifier))
}

pub fn valid_modifier_hint(root: &str) -> String {
    match root {
        "cursor" => "Valid form: [cursor]".to_string(),
        "clipboard" => "Valid forms: [clipboard], [clipboard(0)], [clipboard(1)], [clipboard(2)]"
            .to_string(),
        "time" => format!("Valid modifiers: {}", TIME_MODIFIERS.join(", ")),
        "date" => format!("Valid modifiers: {}", DATE_MODIFIERS.join(", ")),
        "uuid" => format!("Valid modifiers: uuid, {}", UUID_MODIFIERS.join(", ")),
        "env" => "Valid form: [env.VAR_NAME]".to_string(),
        "net" => format!("Valid modifiers: {}", NET_MODIFIERS.join(", ")),
        "sys" => format!("Valid modifiers: {}", SYS_MODIFIERS.join(", ")),
        "run" => "Valid form: [run.<lang>(...)] or [run.<lang>.file(...).args(...)]. Languages: bash, powershell, python, node, node_esm, cmd".to_string(),
        "random" => format!("Valid modifiers: {}", RANDOM_MODIFIERS.join(", ")),
        "lorem" => format!("Valid modifiers: lorem, {}", LOREM_MODIFIERS.join(", ")),
        "mock" => format!("Valid modifiers: {}", MOCK_MODIFIERS.join(", ")),
        "file" => format!("Valid modifiers: {}", FILE_MODIFIERS.join(", ")),
        "key" => format!(
            "Valid forms: [key(<token>)]. Tokens: {}. You can combine them with `+`, and any single character token is also allowed.",
            KEY_MODIFIERS.join(", ")
        ),
        "delay" => "Valid form: [delay(<ms>)] or [delay(<u64>ms)]".to_string(),
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
        "net" => validate_known_modifier("net", modifier, NET_MODIFIERS),
        "sys" => validate_known_modifier("sys", modifier, SYS_MODIFIERS),
        "run" => validate_run_modifier(modifier),
        "random" => validate_random_modifier(modifier),
        "lorem" => validate_lorem_modifier(modifier),
        "mock" => validate_mock_modifier(modifier),
        "file" => validate_file_modifier(modifier),
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

fn validate_random_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "random" })?)
            .ok_or(ValidationError::MissingModifier { root: "random" })?;

    let Some((variant, args)) = parse_random_modifier(modifier) else {
        return Err(ValidationError::InvalidModifier {
            root: "random",
            modifier: modifier.to_string(),
            allowed: RANDOM_MODIFIERS,
        });
    };

    let valid = match variant {
        "int" | "float" => args.is_none_or(|args| {
            let args = split_random_args(args);
            args.is_empty() || args.len() == 2
        }),
        "string" | "alpha" | "numeric" | "hex" | "password" => args.is_none_or(|args| {
            let args = split_random_args(args);
            args.is_empty() || args.len() == 1
        }),
        "choice" => args.is_some_and(|args| !split_random_args(args).is_empty()),
        "bool" | "color" | "ip" | "mac" => {
            args.is_none_or(|args| split_random_args(args).is_empty())
        }
        _ => false,
    };

    if valid {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifier {
            root: "random",
            modifier: modifier.to_string(),
            allowed: RANDOM_MODIFIERS,
        })
    }
}

fn validate_lorem_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(modifier) => {
            let Some((variant, args)) = parse_lorem_modifier(modifier) else {
                return Err(ValidationError::InvalidModifier {
                    root: "lorem",
                    modifier: modifier.to_string(),
                    allowed: LOREM_MODIFIERS,
                });
            };

            let args = split_modifier_args(args);
            let valid = matches!(variant, "words" | "sentence" | "paragraph") && args.len() <= 1;

            if valid {
                Ok(())
            } else {
                Err(ValidationError::InvalidModifier {
                    root: "lorem",
                    modifier: modifier.to_string(),
                    allowed: LOREM_MODIFIERS,
                })
            }
        }
    }
}

fn validate_mock_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "mock" })?)
            .ok_or(ValidationError::MissingModifier { root: "mock" })?;

    let Some((variant, args)) = parse_mock_modifier(modifier) else {
        return Err(ValidationError::InvalidModifier {
            root: "mock",
            modifier: modifier.to_string(),
            allowed: MOCK_MODIFIERS,
        });
    };

    let valid = match (variant, args) {
        ("password", Some(args)) => split_modifier_args(args).len() == 1,
        ("password", None) => false,
        (_, None) => MOCK_MODIFIERS.contains(&variant),
        _ => false,
    };

    if valid {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifier {
            root: "mock",
            modifier: modifier.to_string(),
            allowed: MOCK_MODIFIERS,
        })
    }
}

fn validate_file_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "file" })?)
            .ok_or(ValidationError::MissingModifier { root: "file" })?;

    let Some((variant, args)) = parse_file_modifier(modifier) else {
        return Err(ValidationError::InvalidModifier {
            root: "file",
            modifier: modifier.to_string(),
            allowed: FILE_MODIFIERS,
        });
    };

    let valid = match variant {
        "read" | "random_line" => args.is_some_and(|args| split_modifier_args(args).len() == 1),
        "read_line" => args.is_some_and(|args| {
            let num_args = split_modifier_args(args).len();
            num_args == 2 || num_args == 3
        }),
        _ => false,
    };

    if valid {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifier {
            root: "file",
            modifier: modifier.to_string(),
            allowed: FILE_MODIFIERS,
        })
    }
}

fn parse_random_modifier(input: &str) -> Option<(&str, Option<&str>)> {
    if let Some(paren_idx) = input.find('(') {
        let variant = input[..paren_idx].trim();
        let (args, trailing) = scan_run_parenthesized(&input[paren_idx..])?;
        if !variant.is_empty() && trailing.trim().is_empty() {
            Some((variant, Some(args)))
        } else {
            None
        }
    } else if input.contains(')') {
        None
    } else {
        Some((input.trim(), None)).filter(|(variant, _)| !variant.is_empty())
    }
}

fn parse_file_modifier(input: &str) -> Option<(&str, Option<&str>)> {
    if let Some(paren_idx) = input.find('(') {
        let variant = input[..paren_idx].trim();
        let (args, trailing) = scan_run_parenthesized(&input[paren_idx..])?;
        if !variant.is_empty() && trailing.trim().is_empty() {
            Some((variant, Some(args)))
        } else {
            None
        }
    } else if input.contains(')') {
        None
    } else {
        Some((input.trim(), None)).filter(|(variant, _)| !variant.is_empty())
    }
}

fn parse_mock_modifier(input: &str) -> Option<(&str, Option<&str>)> {
    if let Some(paren_idx) = input.find('(') {
        let variant = input[..paren_idx].trim();
        let (args, trailing) = scan_run_parenthesized(&input[paren_idx..])?;
        if !variant.is_empty() && trailing.trim().is_empty() {
            Some((variant, Some(args)))
        } else {
            None
        }
    } else if input.contains(')') {
        None
    } else {
        Some((input.trim(), None)).filter(|(variant, _)| !variant.is_empty())
    }
}

fn parse_lorem_modifier(input: &str) -> Option<(&str, &str)> {
    let paren_idx = input.find('(')?;
    let variant = input[..paren_idx].trim();
    let (args, trailing) = scan_run_parenthesized(&input[paren_idx..])?;

    if variant.is_empty() || !trailing.trim().is_empty() {
        None
    } else {
        Some((variant, args))
    }
}

fn split_random_args(input: &str) -> Vec<&str> {
    split_modifier_args(input)
}

fn split_modifier_args(input: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;

    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' if bracket_depth > 0 => bracket_depth -= 1,
            ',' if paren_depth == 0 && bracket_depth == 0 => {
                push_random_arg(&mut args, &input[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    push_random_arg(&mut args, &input[start..]);
    args
}

fn push_random_arg<'a>(args: &mut Vec<&'a str>, raw: &'a str) {
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        args.push(trimmed);
    }
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
    let s = s.trim();
    if let Some(n) = s.strip_suffix("ms") {
        n.parse::<u64>().ok()
    } else {
        s.parse::<u64>().ok()
    }
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
        assert_eq!(
            split_system_tag("net.hostname.upper"),
            Some(("net", Some("hostname")))
        );
        assert_eq!(split_system_tag("clipboard"), Some(("clipboard", None)));
        assert_eq!(split_system_tag("clipboard(1)"), Some(("clipboard", None)));
        assert_eq!(
            split_system_tag("clipboard(2).upper"),
            Some(("clipboard", None))
        );
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
    fn validates_net_modifiers() {
        for modifier in NET_MODIFIERS {
            assert_eq!(validate_system_tag("net", Some(modifier)), Ok(()));
        }

        assert_eq!(
            validate_system_tag("net", None),
            Err(ValidationError::MissingModifier { root: "net" })
        );
        assert_eq!(
            validate_system_tag("net", Some("publicip")),
            Err(ValidationError::InvalidModifier {
                root: "net",
                modifier: "publicip".to_string(),
                allowed: NET_MODIFIERS,
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
    fn validates_random_modifier_syntax() {
        assert_eq!(validate_system_tag("random", Some("int")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("int()")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("int(1, 2)")), Ok(()));
        assert_eq!(
            validate_system_tag("random", Some("float(0.1, 9.9)")),
            Ok(())
        );
        assert_eq!(validate_system_tag("random", Some("bool")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("bool()")), Ok(()));
        assert_eq!(
            validate_system_tag("random", Some("choice(alpha(one, two), beta)")),
            Ok(())
        );
        assert_eq!(validate_system_tag("random", Some("string(8)")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("alpha(8)")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("numeric(8)")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("hex(8)")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("password(8)")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("color")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("ip")), Ok(()));
        assert_eq!(validate_system_tag("random", Some("mac")), Ok(()));

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
        assert_eq!(validate_system_tag("lorem", None), Ok(()));
        assert_eq!(validate_system_tag("lorem", Some("words(3)")), Ok(()));
        assert_eq!(validate_system_tag("lorem", Some("words()")), Ok(()));
        assert_eq!(validate_system_tag("lorem", Some("sentence(2)")), Ok(()));
        assert_eq!(validate_system_tag("lorem", Some("paragraph(1)")), Ok(()));
        assert_eq!(validate_system_tag("lorem", Some("words([num=5])")), Ok(()));
        assert_eq!(
            validate_system_tag("lorem", Some("words([random.int(3, 3)])")),
            Ok(())
        );
        assert_eq!(
            validate_system_tag("lorem", Some("paragraph(nope)")),
            Ok(())
        );

        assert_eq!(
            validate_system_tag("lorem", Some("words")),
            Err(ValidationError::InvalidModifier {
                root: "lorem",
                modifier: "words".to_string(),
                allowed: LOREM_MODIFIERS,
            })
        );
        assert_eq!(
            validate_system_tag("lorem", Some("words(1, 2)")),
            Err(ValidationError::InvalidModifier {
                root: "lorem",
                modifier: "words(1, 2)".to_string(),
                allowed: LOREM_MODIFIERS,
            })
        );
    }

    #[test]
    fn validates_mock_modifier_syntax() {
        assert_eq!(validate_system_tag("mock", Some("name")), Ok(()));
        assert_eq!(validate_system_tag("mock", Some("email")), Ok(()));
        assert_eq!(validate_system_tag("mock", Some("status_code")), Ok(()));
        assert_eq!(validate_system_tag("mock", Some("password(12)")), Ok(()));
        assert_eq!(
            validate_system_tag("mock", Some("password([len=12])")),
            Ok(())
        );

        assert_eq!(
            validate_system_tag("mock", None),
            Err(ValidationError::MissingModifier { root: "mock" })
        );
        assert_eq!(
            validate_system_tag("mock", Some("password")),
            Err(ValidationError::InvalidModifier {
                root: "mock",
                modifier: "password".to_string(),
                allowed: MOCK_MODIFIERS,
            })
        );
        assert_eq!(
            validate_system_tag("mock", Some("password()")),
            Err(ValidationError::InvalidModifier {
                root: "mock",
                modifier: "password()".to_string(),
                allowed: MOCK_MODIFIERS,
            })
        );
        assert_eq!(
            validate_system_tag("mock", Some("password(12, 16)")),
            Err(ValidationError::InvalidModifier {
                root: "mock",
                modifier: "password(12, 16)".to_string(),
                allowed: MOCK_MODIFIERS,
            })
        );
        assert_eq!(
            validate_system_tag("mock", Some("unknown")),
            Err(ValidationError::InvalidModifier {
                root: "mock",
                modifier: "unknown".to_string(),
                allowed: MOCK_MODIFIERS,
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
