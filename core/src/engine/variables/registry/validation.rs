use super::super::parser::{BindError, BoundArgs, Param, ParamSpec, bind_call};
use super::super::system;
use super::{
    DATE_METHODS, DATETIME_METHODS, EXEC_MODIFIERS, FILE_MODIFIERS, KEY_MODIFIERS, LOREM_MODIFIERS,
    NET_MODIFIERS, RANDOM_MODIFIERS, TIME_METHODS, UUID_MODIFIERS,
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    UnknownRoot(String),
    /// Retired dotted chain; `hint` is the canonical `namespace(args)` form.
    DotChain {
        got: String,
        hint: String,
    },
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

#[derive(Debug, PartialEq, Eq)]
enum Method<'a> {
    Utc,
    Calc(&'a str),
    Format(&'a str),
}

fn parse_methods(mut key: &str) -> Result<Vec<Method<'_>>, String> {
    let mut methods = Vec::new();
    while !key.is_empty() {
        if key.starts_with("now") {
            key = &key[3..];
        } else if key.starts_with("utc") {
            methods.push(Method::Utc);
            key = &key[3..];
        } else if key.starts_with("calc(") {
            let mut end = 0;
            let mut depth = 1;
            let bytes = key.as_bytes();
            for (i, &b) in bytes.iter().enumerate().skip(5) {
                if b == b'(' {
                    depth += 1;
                } else if b == b')' {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
            }
            if end == 0 {
                return Err("unclosed paren in calc".to_string());
            }
            methods.push(Method::Calc(&key[5..end]));
            key = &key[end + 1..];
        } else if key.starts_with("format(") {
            let mut end = 0;
            let mut depth = 1;
            let mut in_quote = false;
            let bytes = key.as_bytes();
            for (i, &b) in bytes.iter().enumerate().skip(7) {
                match b {
                    b'\'' => in_quote = !in_quote,
                    b'(' if !in_quote => depth += 1,
                    b')' if !in_quote => {
                        depth -= 1;
                        if depth == 0 {
                            end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if end == 0 {
                return Err("unclosed paren in format".to_string());
            }
            methods.push(Method::Format(&key[7..end]));
            key = &key[end + 1..];
        } else {
            return Err(format!("unknown method '{}'", key));
        }

        if !key.is_empty() {
            if !key.starts_with('.') {
                return Err(format!("expected '.' before method, got '{}'", key));
            }
            key = &key[1..];
        }
    }
    Ok(methods)
}

// honey: legacy dot-chain validation glue (Task 9 owns the sweep); the
// resolver moved to chrono(...) and no longer parses dot chains.
pub fn validate_system_tag(root: &str, modifier: Option<&str>) -> Result<(), ValidationError> {
    match root {
        "newline" => validate_no_modifier("newline", modifier),
        "cursor" => validate_no_modifier("cursor", modifier),
        "clipboard" => validate_clip_modifier("clipboard", modifier),
        "time" => validate_time_modifier(modifier),
        "date" => validate_date_modifier(modifier),
        "datetime" => validate_datetime_modifier(modifier),
        "uuid" => validate_uuid_modifier(modifier),
        "env" => validate_env_modifier(modifier),
        "net" => validate_net_modifier(modifier),
        "execute" => validate_exec_modifier(modifier),
        "random" => validate_random_modifier(modifier),
        "lorem" => validate_lorem_modifier(modifier),
        "file" => validate_file_modifier(modifier),
        "key" => validate_key_modifier(modifier),
        "delay" => validate_delay_modifier(modifier),
        "use" => validate_use_modifier(modifier),
        "http" => validate_http_modifier(modifier),
        "mouse" => validate_mouse_modifier(modifier),
        "image" => validate_image_modifier(modifier),
        _ => Err(ValidationError::UnknownRoot(root.to_string())),
    }
}

fn validate_image_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let raw = modifier.unwrap_or_default().trim();
    if raw.is_empty() {
        return Err(ValidationError::MissingModifier { root: "image" });
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

const CLIP_INDEX_MODIFIERS: &[&str] = &["(1)", "(2)"];

fn validate_clip_modifier(
    root: &'static str,
    modifier: Option<&str>,
) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(m) if CLIP_INDEX_MODIFIERS.contains(&m) => Ok(()),
        Some(m) => Err(ValidationError::InvalidModifier {
            root,
            modifier: m.to_string(),
            allowed: CLIP_INDEX_MODIFIERS,
        }),
    }
}

fn validate_uuid_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(m) if UUID_MODIFIERS.contains(&m) => Ok(()),
        Some(m) => Err(ValidationError::InvalidModifier {
            root: "uuid",
            modifier: m.to_string(),
            allowed: UUID_MODIFIERS,
        }),
    }
}

fn validate_time_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(m) => {
            if parse_methods(m).is_ok() {
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
            if parse_methods(m).is_ok() {
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

fn validate_datetime_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(m) => {
            if parse_methods(m).is_ok() {
                Ok(())
            } else {
                Err(ValidationError::InvalidModifier {
                    root: "datetime",
                    modifier: m.to_string(),
                    allowed: DATETIME_METHODS,
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

    let valid = matches!((variant, args), ("publicip" | "localip" | "online", None));

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
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "execute" })?)
            .ok_or(ValidationError::MissingModifier { root: "execute" })?;

    // Delegate to the real order-independent parser from execute.rs
    match system::execute::parse_invocation(&format!("execute.{}", modifier)) {
        Ok(_) => Ok(()),
        Err(_) => Err(ValidationError::InvalidModifier {
            root: "execute",
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
    let modifier = match modifier.and_then(normalize_modifier) {
        None => return Ok(()),
        Some(m) => m,
    };

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
            args.is_empty() || args.len() == 1 || args.len() == 2
        }),
        "str" | "pass" => args.is_none_or(|args| {
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
        None => Ok(()),
        Some(modifier) => {
            let Some((variant, args)) = parse_lorem_modifier(modifier) else {
                return Err(ValidationError::InvalidModifier {
                    root: "lorem",
                    modifier: modifier.to_string(),
                    allowed: LOREM_MODIFIERS,
                });
            };

            let valid = match args {
                None => matches!(variant, "paragraphs" | "words" | "sentences"),
                Some(args_str) => {
                    let args = split_modifier_args(args_str);
                    // honey: counts are numeric or dynamic ([...]); resolver drops the rest.
                    matches!(variant, "paragraphs" | "words" | "sentences")
                        && args.len() <= 1
                        && args
                            .iter()
                            .all(|a| a.parse::<usize>().is_ok() || a.contains('['))
                }
            };

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

fn validate_file_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "file" })?)
            .ok_or(ValidationError::MissingModifier { root: "file" })?;

    if let Some((variant, args)) = parse_file_modifier(modifier) {
        let valid = match variant {
            "read" => args.is_some_and(|args| split_modifier_args(args).len() == 1),
            "line" => args.is_some_and(|args| split_modifier_args(args).len() == 2),
            "lines" => args.is_some_and(|args| {
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
    "click(button)",
    "dblclick",
    "dblclick(button)",
    "down",
    "down(button)",
    "up",
    "up(button)",
    "hold",
    "hold(button)",
    "release",
    "release(button)",
    "rclick",
    "mclick",
    "m4",
    "m5",
    "move(x, y)",
    "scroll(delta)",
];

fn validate_mouse_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    use crate::keys::MouseButton;

    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "mouse" })?)
            .ok_or(ValidationError::MissingModifier { root: "mouse" })?;

    if let Some((variant, args)) = parse_file_modifier(modifier)
        && (match variant {
            "pos" | "rclick" | "mclick" | "m4" | "m5" => args.is_none(),
            "click" | "dblclick" | "down" | "up" | "hold" | "release" => {
                if let Some(arg_str) = args {
                    let parts = split_modifier_args(arg_str);
                    parts.len() == 1 && MouseButton::from_alias(parts[0]).is_some()
                } else {
                    true
                }
            }
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

fn parse_lorem_modifier(input: &str) -> Option<(&str, Option<&str>)> {
    if input.starts_with('(') {
        let (args, trailing) = scan_exec_parenthesized(input)?;
        if trailing.trim().is_empty() {
            Some(("paragraphs", Some(args)))
        } else {
            None
        }
    } else if let Some(paren_idx) = input.find('(') {
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
            allowed: &["<u64>ms", "<f64>s"],
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
    } else if let Some(n) = s.strip_suffix('s') {
        n.parse::<f64>()
            .ok()
            .map(|seconds| (seconds * 1000.0) as u64)
    } else {
        s.parse::<u64>().ok()
    }
}

struct NsSpec {
    name: &'static str,
    params: &'static [Param<'static>],
    names: &'static [&'static str],
    variadic: bool,
}

// Per-namespace binder specs for the unified `namespace(args)` syntax.
// Defaults from the catalog; value semantics belong to each resolver
// (Tasks 4-8), this table only shapes positional/named binding.
const NS_SPECS: &[NsSpec] = &[
    NsSpec {
        name: "chrono",
        params: &[
            Param {
                name: "type",
                required: false,
                default: "datetime",
            },
            Param {
                name: "offset",
                required: false,
                default: "none",
            },
            Param {
                name: "format",
                required: false,
                default: "",
            },
            Param {
                name: "tz",
                required: false,
                default: "local",
            },
        ],
        names: &["type", "offset", "format", "tz"],
        variadic: false,
    },
    NsSpec {
        name: "clip",
        params: &[Param {
            name: "index",
            required: false,
            default: "0",
        }],
        names: &["index"],
        variadic: false,
    },
    NsSpec {
        name: "uuid",
        params: &[Param {
            name: "version",
            required: false,
            default: "v4",
        }],
        names: &["version"],
        variadic: false,
    },
    NsSpec {
        name: "random",
        params: &[
            Param {
                name: "type",
                required: false,
                default: "int",
            },
            Param {
                name: "min",
                required: false,
                default: "0",
            },
            Param {
                name: "max",
                required: false,
                default: "100",
            },
        ],
        names: &["type", "min", "max"],
        variadic: true,
    },
    NsSpec {
        name: "lorem",
        params: &[
            Param {
                name: "type",
                required: false,
                default: "paragraphs",
            },
            Param {
                name: "count",
                required: false,
                default: "",
            },
        ],
        names: &["type", "count"],
        variadic: false,
    },
    NsSpec {
        name: "file",
        params: &[
            Param {
                name: "op",
                required: true,
                default: "",
            },
            Param {
                name: "path",
                required: false,
                default: "",
            },
            Param {
                name: "start",
                required: false,
                default: "",
            },
            Param {
                name: "end",
                required: false,
                default: "",
            },
        ],
        names: &["op", "path", "start", "end"],
        variadic: false,
    },
    NsSpec {
        name: "ip",
        params: &[Param {
            name: "type",
            required: false,
            default: "public",
        }],
        names: &["type"],
        variadic: false,
    },
    NsSpec {
        name: "http",
        params: &[
            Param {
                name: "op",
                required: true,
                default: "",
            },
            Param {
                name: "url",
                required: true,
                default: "",
            },
        ],
        names: &["op", "url"],
        variadic: false,
    },
    NsSpec {
        name: "env",
        params: &[
            Param {
                name: "name",
                required: true,
                default: "",
            },
            Param {
                name: "default",
                required: false,
                default: "",
            },
        ],
        names: &["name", "default"],
        variadic: false,
    },
    NsSpec {
        name: "execute",
        params: &[
            Param {
                name: "lang",
                required: true,
                default: "",
            },
            Param {
                name: "subject",
                required: true,
                default: "",
            },
            Param {
                name: "file",
                required: false,
                default: "false",
            },
            Param {
                name: "silent",
                required: false,
                default: "false",
            },
        ],
        names: &["lang", "subject", "file", "silent"],
        variadic: true,
    },
    NsSpec {
        name: "mouse",
        params: &[
            Param {
                name: "action",
                required: true,
                default: "",
            },
            Param {
                name: "btn",
                required: false,
                default: "",
            },
            Param {
                name: "count",
                required: false,
                default: "1",
            },
        ],
        names: &["action", "btn", "count"],
        variadic: false,
    },
    NsSpec {
        name: "key",
        params: &[Param {
            name: "token",
            required: true,
            default: "",
        }],
        names: &["token"],
        variadic: false,
    },
    NsSpec {
        name: "delay",
        params: &[Param {
            name: "ms",
            required: true,
            default: "",
        }],
        names: &["ms"],
        variadic: false,
    },
    NsSpec {
        name: "use",
        params: &[Param {
            name: "name",
            required: true,
            default: "",
        }],
        names: &["name"],
        variadic: false,
    },
    NsSpec {
        name: "image",
        params: &[Param {
            name: "path",
            required: true,
            default: "",
        }],
        names: &["path"],
        variadic: false,
    },
    NsSpec {
        name: "cursor",
        params: &[],
        names: &[],
        variadic: false,
    },
    NsSpec {
        name: "newline",
        params: &[],
        names: &[],
        variadic: false,
    },
];

/// Binder spec for a unified namespace (keys are case-insensitive).
pub fn param_spec(ns: &str) -> Option<ParamSpec<'static>> {
    let lower = ns.trim().to_ascii_lowercase();
    NS_SPECS
        .iter()
        .find(|s| s.name == lower)
        .map(|s| ParamSpec { params: s.params })
}

/// Did-you-mean hint for deleted roots, for error renderers.
pub fn deleted_root_hint(ns: &str) -> Option<&'static str> {
    match ns.trim().to_ascii_lowercase().as_str() {
        "clipboard" => Some("clip"),
        "date" | "time" | "datetime" => Some("chrono(...)"),
        "net" => Some("ip"),
        _ => None,
    }
}

const MOUSE_ACTIONS: &[&str] = &["click", "hold", "release", "move", "scroll", "pos"];

fn dot_hint(ns: &str) -> String {
    let (root, rest) = ns.split_once('.').unwrap_or((ns, ""));
    let sub = rest.split('.').next().unwrap_or("");
    match root {
        "mouse" => match sub {
            "down" | "hold" => "mouse(hold, mN)",
            "up" | "release" => "mouse(release, mN)",
            "move" => "mouse(move, x, y)",
            "scroll" => "mouse(scroll, delta)",
            "pos" => "mouse(pos)",
            _ => "mouse(click, mN[, n])",
        }
        .to_string(),
        "date" | "time" | "datetime" => "chrono(...)".to_string(),
        "clipboard" => "clip".to_string(),
        "net" => "ip".to_string(),
        _ => format!("{root}(...)"),
    }
}

fn is_strict_mouse_button(s: &str) -> bool {
    s.strip_prefix('m')
        .and_then(|n| n.parse::<u16>().ok())
        .is_some_and(|n| (1..=255).contains(&n))
}

fn validate_mouse_args(bound: &BoundArgs) -> Result<(), ValidationError> {
    let action = bound.named.get("action").map(String::as_str).unwrap_or("");
    if !MOUSE_ACTIONS.contains(&action) {
        return Err(ValidationError::InvalidModifier {
            root: "mouse",
            modifier: action.to_string(),
            allowed: MOUSE_ACTIONS,
        });
    }
    if matches!(action, "click" | "hold" | "release") {
        let btn = bound.named.get("btn").map(String::as_str).unwrap_or("");
        if btn.is_empty() {
            return Err(ValidationError::MissingModifier { root: "mouse" });
        }
        if !is_strict_mouse_button(btn) {
            return Err(ValidationError::InvalidModifier {
                root: "mouse",
                modifier: btn.to_string(),
                allowed: &["m1..mN"],
            });
        }
    }
    Ok(())
}

fn map_bind_error(spec: &NsSpec, raw: &str, err: BindError) -> ValidationError {
    match err {
        BindError::MissingRequired { .. } => ValidationError::MissingModifier { root: spec.name },
        BindError::UnknownKey { key, .. } => ValidationError::InvalidModifier {
            root: spec.name,
            modifier: key,
            allowed: spec.names,
        },
        BindError::Duplicate { param, .. } => ValidationError::InvalidModifier {
            root: spec.name,
            modifier: param,
            allowed: spec.names,
        },
        BindError::PositionalAfterNamed { .. } | BindError::Arity { .. } => {
            ValidationError::InvalidModifier {
                root: spec.name,
                modifier: raw.to_string(),
                allowed: spec.names,
            }
        }
    }
}

/// Validates a unified `namespace(args)` call: namespace from
/// `parse_system_call`, raw args (None when bare). Namespace keys are
/// case-insensitive, values are case-sensitive.
pub fn validate_system_call(ns: &str, raw: Option<&str>) -> Result<(), ValidationError> {
    let ns = ns.trim();
    let raw = raw.unwrap_or("").trim();
    let lower = ns.to_ascii_lowercase();
    if lower.contains('.') {
        let got = if raw.is_empty() {
            ns.to_string()
        } else {
            format!("{ns}({raw})")
        };
        return Err(ValidationError::DotChain {
            got,
            hint: dot_hint(&lower),
        });
    }
    match lower.as_str() {
        "clipboard" => Err(ValidationError::UnknownRoot(ns.to_string())),
        "date" | "time" | "datetime" if !raw.is_empty() => Err(ValidationError::DotChain {
            got: format!("{ns}.{raw}"),
            hint: "chrono(...)".to_string(),
        }),
        "date" | "time" | "datetime" => Err(ValidationError::UnknownRoot(ns.to_string())),
        _ => {
            let Some(spec) = NS_SPECS.iter().find(|s| s.name == lower) else {
                return Err(ValidationError::UnknownRoot(ns.to_string()));
            };
            let bound = match bind_call(
                spec.name,
                raw,
                &ParamSpec {
                    params: spec.params,
                },
            ) {
                Ok(bound) => bound,
                Err(err) => return Err(map_bind_error(spec, raw, err)),
            };
            if (spec.name == "cursor" || spec.name == "newline") && !bound.positional.is_empty() {
                return Err(ValidationError::UnexpectedModifier {
                    root: spec.name,
                    modifier: raw.to_string(),
                });
            }
            if !spec.variadic && bound.positional.len() > spec.params.len() {
                return Err(ValidationError::InvalidModifier {
                    root: spec.name,
                    modifier: raw.to_string(),
                    allowed: spec.names,
                });
            }
            if spec.name == "mouse" {
                validate_mouse_args(&bound)
            } else {
                Ok(())
            }
        }
    }
}
