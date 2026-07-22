use super::system;
use crate::engine::variables::system::exec::parse_invocation;

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
    "cursor", "clip", "time", "date", "uuid", "env", "net", "exec", "random", "key", "delay",
    "lorem", "mock", "file", "use", "http", "mouse", "img",
];

const TIME_METHODS: &[&str] = &["utc", "calc(±...)", "format(...)"];
const DATE_METHODS: &[&str] = &["utc", "calc(±...)", "format(...)"];

const UUID_MODIFIERS: &[&str] = &["v4", "v7"];
const NET_MODIFIERS: &[&str] = &["ip", "lip", "online", "port(n)"];
const EXEC_MODIFIERS: &[&str] = &[
    "exec.<lang>(...)",
    "exec.silent.<lang>(...)",
    "exec.<lang>.file(...).args(...)",
];
const RANDOM_MODIFIERS: &[&str] = &[
    "int(min, max)",
    "choice(a, b, ...)",
    "str(len)",
    "hex(len)",
    "pass(len)",
];
const LOREM_MODIFIERS: &[&str] = &["word(n)", "sentence(n)", "paragraph(n)"];
const MOCK_MODIFIERS: &[&str] = &[
    "name",
    "first_name",
    "last_name",
    "address",
    "city",
    "state",
    "zip_code",
    "country",
    "email",
    "domain",
    "username",
    "company",
    "job_title",
    "credit_card",
    "phone_number",
    "cell_number",
];
const FILE_MODIFIERS: &[&str] = &["read(path)", "read_line(path, start, [end])"];
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

pub fn strip_global_transformers(key: &str) -> &str {
    let pipeline = system::transformers::split_pipeline(key);
    pipeline[0]
}

pub fn split_system_tag(key: &str) -> Option<(&str, Option<&str>)> {
    let base = strip_global_transformers(key);
    if system::clip::is_clip_key(base) {
        return Some(("clip", None));
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
    if let Some(rest) = base.strip_prefix("env(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("env", Some(inner)));
    }
    if let Some(rest) = base.strip_prefix("use(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("use", Some(inner)));
    }
    if let Some(rest) = base.strip_prefix("img(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("img", Some(inner)));
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
        "clip" => "Valid forms: [clip], [clip(0)], [clip(1)], [clip(2)]"
            .to_string(),
        "time" => format!("Valid modifiers / methods: {}", TIME_METHODS.join(", ")),
        "date" => format!("Valid modifiers / methods: {}", DATE_METHODS.join(", ")),
        "uuid" => format!("Valid modifiers: uuid, {}", UUID_MODIFIERS.join(", ")),
        "env" => "Valid form: [env(<var_name>)] or [env(\"<var_name>\")]".to_string(),
        "net" => format!("Valid modifiers: {}", NET_MODIFIERS.join(", ")),
        "exec" => "Valid forms: [exec.bash(...)], [exec.powershell(...)], [exec.python(...)], [exec.node(...)], [exec.node_esm(...)], [exec.cmd(...)]".to_string(),
        "random" => format!("Valid modifiers: {}", RANDOM_MODIFIERS.join(", ")),
        "lorem" => format!("A modifier is required. Valid modifiers: {}", LOREM_MODIFIERS.join(", ")),
        "mock" => format!("Valid modifiers: {}", MOCK_MODIFIERS.join(", ")),
        "file" => format!("Valid modifiers: {}", FILE_MODIFIERS.join(", ")),
        "key" => format!(
            "Valid forms: [key(<token>)]. Tokens: {}. You can combine them with `+`, and any single character token is also allowed.",
            KEY_MODIFIERS.join(", ")
        ),
        "delay" => "Valid form: [delay(<ms>)] or [delay(<u64>ms)]".to_string(),
        "use" => "Valid form: [use(\"trigger_name\")]".to_string(),
        "http" => "Valid forms: [http.get(<url>)], [http.status(<url>)]".to_string(),
        "mouse" => "Valid forms: [mouse.click], [mouse.rclick], [mouse.mclick], [mouse.move(x, y)], [mouse.scroll(delta)], [mouse.hold], [mouse.release], [mouse.pos]".to_string(),
        _ => "No modifier help available.".to_string(),
    }
}

pub fn validate_system_tag(root: &str, modifier: Option<&str>) -> Result<(), ValidationError> {
    match root {
        "cursor" => validate_no_modifier("cursor", modifier),
        "clip" => validate_clip_modifier(modifier),
        "time" => validate_time_modifier(modifier),
        "date" => validate_date_modifier(modifier),
        "uuid" => validate_known_modifier("uuid", modifier, UUID_MODIFIERS),
        "env" => validate_env_modifier(modifier),
        "net" => validate_net_modifier(modifier),
        "exec" => validate_exec_modifier(modifier),
        "random" => validate_random_modifier(modifier),
        "lorem" => validate_lorem_modifier(modifier),
        "mock" => validate_mock_modifier(modifier),
        "file" => validate_file_modifier(modifier),
        "key" => validate_key_modifier(modifier),
        "delay" => validate_delay_modifier(modifier),
        "use" => validate_use_modifier(modifier),
        "http" => validate_http_modifier(modifier),
        "mouse" => validate_mouse_modifier(modifier),
        "img" => validate_img_modifier(modifier),
        _ => Err(ValidationError::UnknownRoot(root.to_string())),
    }
}

fn validate_img_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let raw = modifier.unwrap_or_default().trim();
    if raw.is_empty() {
        return Err(ValidationError::MissingModifier { root: "img" });
    }
    Ok(())
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

const CLIP_INDEX_MODIFIERS: &[&str] = &["(0)", "(1)", "(2)"];

fn validate_clip_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(m) if CLIP_INDEX_MODIFIERS.contains(&m) => Ok(()),
        Some(m) => Err(ValidationError::InvalidModifier {
            root: "clip",
            modifier: m.to_string(),
            allowed: CLIP_INDEX_MODIFIERS,
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

fn validate_time_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(m) => {
            if system::time::parse_methods(m).is_ok() {
                Ok(())
            } else {
                Err(ValidationError::InvalidModifier {
                    root: "time",
                    modifier: m.to_string(),
                    allowed: TIME_METHODS,
                })
            }
        }
    }
}

fn validate_date_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(m) => {
            if system::date::parse_methods(m).is_ok() {
                Ok(())
            } else {
                Err(ValidationError::InvalidModifier {
                    root: "date",
                    modifier: m.to_string(),
                    allowed: DATE_METHODS,
                })
            }
        }
    }
}

fn validate_net_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "net" })?)
            .ok_or(ValidationError::MissingModifier { root: "net" })?;

    let Some((variant, args)) = parse_file_modifier(modifier) else {
        return Err(ValidationError::InvalidModifier {
            root: "net",
            modifier: modifier.to_string(),
            allowed: NET_MODIFIERS,
        });
    };

    let valid = match (variant, args) {
        ("ip" | "lip" | "online", None) => true,
        ("port", Some(args)) => split_modifier_args(args).len() == 1,
        _ => false,
    };

    if valid {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifier {
            root: "net",
            modifier: modifier.to_string(),
            allowed: NET_MODIFIERS,
        })
    }
}

fn validate_env_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let raw = modifier.unwrap_or_default().trim();
    let var_name = crate::engine::variables::system::strip_quotes(raw).unwrap_or(raw);
    if !var_name.is_empty() {
        Ok(())
    } else {
        Err(ValidationError::MissingModifier { root: "env" })
    }
}

fn validate_exec_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "exec" })?)
            .ok_or(ValidationError::MissingModifier { root: "exec" })?;

    // Delegate to the real order-independent parser from exec.rs
    match parse_invocation(&format!("exec.{}", modifier)) {
        Ok(_) => Ok(()),
        Err(_) => Err(ValidationError::InvalidModifier {
            root: "exec",
            modifier: modifier.to_string(),
            allowed: EXEC_MODIFIERS,
        }),
    }
}

fn scan_exec_parenthesized(input: &str) -> Option<(&str, &str)> {
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
        "int" => args.is_none_or(|args| {
            let args = split_random_args(args);
            args.is_empty() || args.len() == 2
        }),
        "str" | "hex" | "pass" => args.is_none_or(|args| {
            let args = split_random_args(args);
            args.is_empty() || args.len() == 1
        }),
        "choice" => args.is_some_and(|args| !split_random_args(args).is_empty()),
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
        None => Err(ValidationError::MissingModifier { root: "lorem" }),
        Some(modifier) => {
            let Some((variant, args)) = parse_lorem_modifier(modifier) else {
                return Err(ValidationError::InvalidModifier {
                    root: "lorem",
                    modifier: modifier.to_string(),
                    allowed: LOREM_MODIFIERS,
                });
            };

            let args = split_modifier_args(args);
            let valid = matches!(variant, "word" | "sentence" | "paragraph") && args.len() <= 1;

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
    validate_known_modifier("mock", modifier, MOCK_MODIFIERS)
}

fn validate_file_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "file" })?)
            .ok_or(ValidationError::MissingModifier { root: "file" })?;

    if let Some((variant, args)) = parse_file_modifier(modifier) {
        let valid = match variant {
            "read" => args.is_some_and(|args| split_modifier_args(args).len() == 1),
            "read_line" => args.is_some_and(|args| {
                let count = split_modifier_args(args).len();
                count == 2 || count == 3
            }),
            _ => false,
        };

        if valid {
            return Ok(());
        }
    }

    Err(ValidationError::InvalidModifier {
        root: "file",
        modifier: modifier.to_string(),
        allowed: FILE_MODIFIERS,
    })
}

fn validate_use_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let raw = modifier.unwrap_or_default().trim();
    let name = crate::engine::variables::system::strip_quotes(raw).unwrap_or(raw);
    if !name.is_empty() {
        Ok(())
    } else {
        Err(ValidationError::MissingModifier { root: "use" })
    }
}

const HTTP_MODIFIERS: &[&str] = &["get(url)", "status(url)"];

fn validate_http_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "http" })?)
            .ok_or(ValidationError::MissingModifier { root: "http" })?;

    if let Some((variant, args)) = parse_file_modifier(modifier)
        && (match variant {
            "get" | "status" => args.is_some_and(|args| split_modifier_args(args).len() == 1),
            _ => false,
        })
    {
        return Ok(());
    }

    Err(ValidationError::InvalidModifier {
        root: "http",
        modifier: modifier.to_string(),
        allowed: HTTP_MODIFIERS,
    })
}

const MOUSE_MODIFIERS: &[&str] = &[
    "pos",
    "click",
    "rclick",
    "mclick",
    "hold",
    "release",
    "move(x, y)",
    "scroll(delta)",
];

fn validate_mouse_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "mouse" })?)
            .ok_or(ValidationError::MissingModifier { root: "mouse" })?;

    if let Some((variant, args)) = parse_file_modifier(modifier)
        && (match variant {
            "pos" | "click" | "rclick" | "mclick" | "hold" | "release" => args.is_none(),
            "move" => args.is_some_and(|args| split_modifier_args(args).len() == 2),
            "scroll" => args.is_some_and(|args| split_modifier_args(args).len() == 1),
            _ => false,
        })
    {
        return Ok(());
    }

    Err(ValidationError::InvalidModifier {
        root: "mouse",
        modifier: modifier.to_string(),
        allowed: MOUSE_MODIFIERS,
    })
}

fn parse_random_modifier(input: &str) -> Option<(&str, Option<&str>)> {
    if let Some(paren_idx) = input.find('(') {
        let variant = input[..paren_idx].trim();
        let (args, trailing) = scan_exec_parenthesized(&input[paren_idx..])?;
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
        let (args, trailing) = scan_exec_parenthesized(&input[paren_idx..])?;
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
    let (args, trailing) = scan_exec_parenthesized(&input[paren_idx..])?;

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
    let trimmed = crate::engine::variables::system::strip_argument_quotes(modifier);
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
    fn validates_mock_modifier_syntax() {
        assert_eq!(validate_system_tag("mock", Some("name")), Ok(()));
        assert_eq!(validate_system_tag("mock", Some("email")), Ok(()));
        assert_eq!(
            validate_system_tag("mock", None),
            Err(ValidationError::MissingModifier { root: "mock" })
        );
        assert_eq!(
            validate_system_tag("mock", Some("password(12)")),
            Err(ValidationError::InvalidModifier {
                root: "mock",
                modifier: "password(12)".to_string(),
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
}
