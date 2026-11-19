use super::super::parser::{BindError, BoundArgs, Param, ParamSpec, bind_call, has_named_args};
use super::super::system::datetime::apply_temporal_calc;
use super::super::system::file::check_args as check_file_args;
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

struct NsSpec {
    name: &'static str,
    params: &'static [Param<'static>],
    names: &'static [&'static str],
    variadic: bool,
}

// Per-namespace binder specs for the unified `namespace(args)` syntax.
// Defaults from the catalog; value semantics belong to each resolver,
// this table only shapes positional/named binding.
const NS_SPECS: &[NsSpec] = &[
    NsSpec {
        name: "date",
        params: &[],
        names: &["tomorrow", "yesterday", "+offset", "utc", "format"],
        variadic: true,
    },
    NsSpec {
        name: "time",
        params: &[],
        names: &["utc", "local", "+offset", "format"],
        variadic: true,
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
        params: &[],
        names: &[],
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
        name: "run",
        params: &[
            Param {
                name: "command",
                required: true,
                default: "",
            },
            Param {
                name: "silent",
                required: false,
                default: "false",
            },
        ],
        names: &["command", "silent"],
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
        name: "wait",
        params: &[Param {
            name: "duration",
            required: true,
            default: "",
        }],
        names: &["duration"],
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
    NsSpec {
        name: "calc",
        params: &[Param {
            name: "expression",
            required: true,
            default: "",
        }],
        names: &["expression"],
        variadic: true,
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
        "chrono" => Some("date or time"),
        "datetime" => Some("date or time"),
        "execute" => Some("run \"cmd\""),
        "http" => Some("run \"curl ...\""),
        "delay" => Some("wait ..."),
        "key" => Some("enter], [tab], [ctrl+s"),
        "net" => Some("ip"),
        _ => None,
    }
}

const MOUSE_ACTIONS: &[&str] = &["click", "hold", "release", "move", "scroll", "pos"];

fn dot_hint(ns: &str) -> String {
    let (root, rest) = ns.split_once('.').unwrap_or((ns, ""));
    let sub = rest.split('.').next().unwrap_or("");
    let sub_base = sub.split('(').next().unwrap_or(sub);
    match root {
        "mouse" => match sub_base {
            "rclick" => "mouse click m2",
            "mclick" => "mouse click m3",
            "m4" => "mouse click m4",
            "m5" => "mouse click m5",
            "dblclick" => "mouse click m1 2",
            "down" | "hold" => "mouse hold m1",
            "up" | "release" => "mouse release m1",
            "move" => "mouse move x y",
            "scroll" => "mouse scroll delta",
            "pos" => "mouse pos",
            "click" => "mouse click m1",
            _ => "mouse click mN [n]",
        }
        .to_string(),
        "date" => match sub_base {
            "utc" | "local" => format!("date {sub_base}"),
            _ => "date".to_string(),
        },
        "time" => match sub_base {
            "utc" | "local" => format!("time {sub_base}"),
            _ => "time".to_string(),
        },
        "datetime" => "date] [time".to_string(),
        "chrono" => "date] or [time".to_string(),
        "clipboard" => "clip".to_string(),
        "net" => "ip".to_string(),
        _ => format!("{root} ..."),
    }
}

fn is_strict_mouse_button(s: &str) -> bool {
    s.strip_prefix('m')
        .and_then(|n| n.parse::<u16>().ok())
        .is_some_and(|n| (1..=255).contains(&n))
}

const CLIP_INDEXES: &[&str] = &["0", "1", "2"];
const UUID_VERSIONS: &[&str] = &["v4", "v7"];
const IP_TYPES: &[&str] = &["public", "local"];
const RANDOM_KINDS: &[&str] = &["int", "choice", "str", "pass"];
const LOREM_KINDS: &[&str] = &["words", "sentences", "paragraphs"];

fn invalid(
    root: &'static str,
    modifier: String,
    allowed: &'static [&'static str],
) -> ValidationError {
    ValidationError::InvalidModifier {
        root,
        modifier,
        allowed,
    }
}

fn parse_usize_at_least(value: &str, min: usize) -> Option<usize> {
    value.parse::<usize>().ok().filter(|&n| n >= min)
}

const RANDOM_INT_HINT: &[&str] = &["random(int[, min, max])"];
const RANDOM_CHOICE_HINT: &[&str] = &["random(choice, option, ...)"];
const RANDOM_STR_HINT: &[&str] = &["random(str[, len])", "random(pass[, len])"];
const FILE_HINT: &[&str] = &[
    "file(read, path)",
    "file(line, path, n)",
    "file(lines, path, start[, end])",
];

fn validate_value_args(
    ns: &str,
    bound: &BoundArgs,
    spec: &NsSpec,
    raw: &str,
) -> Result<(), ValidationError> {
    let named = |key: &str| bound.named.get(key).map(String::as_str).unwrap_or("");
    match ns {
        "clip" => {
            let index = named("index");
            if index
                .parse::<usize>()
                .is_ok_and(|n| n < crate::engine::variables::system::clipboard::HISTORY_CAPACITY)
            {
                Ok(())
            } else {
                Err(invalid("clip", index.to_string(), CLIP_INDEXES))
            }
        }
        "uuid" => {
            if UUID_VERSIONS.contains(&named("version")) {
                Ok(())
            } else {
                Err(invalid("uuid", named("version").to_string(), UUID_VERSIONS))
            }
        }
        "ip" => {
            if IP_TYPES.contains(&named("type")) {
                Ok(())
            } else {
                Err(invalid("ip", named("type").to_string(), IP_TYPES))
            }
        }
        "random" => validate_random_args(bound, spec, raw),
        "lorem" => {
            let pspec = ParamSpec {
                params: spec.params,
            };
            let (kind, count) = if has_named_args(bound, &pspec) {
                (named("type").to_string(), named("count").to_string())
            } else if bound.positional.is_empty() {
                ("paragraphs".to_string(), String::new())
            } else if bound.positional.len() == 1 && bound.positional[0].parse::<usize>().is_ok() {
                ("paragraphs".to_string(), bound.positional[0].clone())
            } else if bound.positional.len() == 1 {
                (bound.positional[0].clone(), String::new())
            } else {
                (bound.positional[0].clone(), bound.positional[1].clone())
            };
            if !LOREM_KINDS.contains(&kind.as_str()) {
                return Err(invalid("lorem", kind, LOREM_KINDS));
            }
            if count.is_empty()
                || parse_usize_at_least(count.trim(), 1)
                    .is_some_and(|n| n <= crate::engine::variables::system::lorem::MAX_LOREM_COUNT)
            {
                Ok(())
            } else {
                Err(invalid("lorem", count, &["lorem(words, 5)"]))
            }
        }
        "file" => {
            if check_file_args(named("op"), named("path"), named("start"), named("end")) {
                Ok(())
            } else {
                Err(invalid("file", raw.to_string(), FILE_HINT))
            }
        }
        _ => Ok(()),
    }
}

fn validate_random_args(
    bound: &BoundArgs,
    spec: &NsSpec,
    raw: &str,
) -> Result<(), ValidationError> {
    let pspec = ParamSpec {
        params: spec.params,
    };
    let named = |key: &str| bound.named.get(key).map(String::as_str).unwrap_or("");
    let kind: &str = if bound.positional.is_empty() {
        named("type")
    } else if RANDOM_KINDS.contains(&bound.positional[0].as_str()) {
        bound.positional[0].as_str()
    } else if bound.positional[0].parse::<i64>().is_ok() {
        "int"
    } else {
        return Err(invalid("random", bound.positional[0].clone(), RANDOM_KINDS));
    };
    if !RANDOM_KINDS.contains(&kind) {
        return Err(invalid("random", kind.to_string(), RANDOM_KINDS));
    }
    match kind {
        "int" => {
            if has_named_args(bound, &pspec) {
                let ok = match (named("min").parse::<i64>(), named("max").parse::<i64>()) {
                    (Ok(lo), Ok(hi)) => lo <= hi,
                    _ => false,
                };
                if ok {
                    Ok(())
                } else {
                    Err(invalid("random", raw.to_string(), RANDOM_INT_HINT))
                }
            } else {
                let nums: Vec<&str> = if bound.positional.first().map(String::as_str) == Some("int")
                {
                    bound
                        .positional
                        .iter()
                        .skip(1)
                        .map(String::as_str)
                        .collect()
                } else {
                    bound.positional.iter().map(String::as_str).collect()
                };
                let ok = match nums.as_slice() {
                    [] => true,
                    [max] => max.parse::<i64>().is_ok_and(|m| m >= 1),
                    [min, max] => match (min.parse::<i64>(), max.parse::<i64>()) {
                        (Ok(lo), Ok(hi)) => lo <= hi,
                        _ => false,
                    },
                    _ => false,
                };
                if ok {
                    Ok(())
                } else {
                    Err(invalid("random", raw.to_string(), RANDOM_INT_HINT))
                }
            }
        }
        "choice" => {
            let opts: Vec<&str> = if bound.positional.first().map(String::as_str) == Some("choice")
            {
                bound
                    .positional
                    .iter()
                    .skip(1)
                    .map(String::as_str)
                    .collect()
            } else {
                bound.positional.iter().map(String::as_str).collect()
            };
            if opts.is_empty() {
                return Err(invalid("random", raw.to_string(), RANDOM_CHOICE_HINT));
            }
            if has_named_args(bound, &pspec) && (named("min") != "0" || named("max") != "100") {
                return Err(invalid("random", raw.to_string(), RANDOM_CHOICE_HINT));
            }
            Ok(())
        }
        _ => {
            let tail: Vec<&str> = if bound.positional.first().map(String::as_str) == Some(kind) {
                bound
                    .positional
                    .iter()
                    .skip(1)
                    .map(String::as_str)
                    .collect()
            } else {
                bound.positional.iter().map(String::as_str).collect()
            };
            if tail.len() > 1 {
                return Err(invalid("random", raw.to_string(), RANDOM_STR_HINT));
            }
            if let Some(len_str) = tail.first() {
                if has_named_args(bound, &pspec) {
                    return Err(invalid("random", raw.to_string(), RANDOM_STR_HINT));
                }
                if parse_usize_at_least(len_str, 1).is_some_and(|n| {
                    n <= crate::engine::variables::system::random::MAX_RANDOM_STRING_LEN
                }) {
                    Ok(())
                } else {
                    Err(invalid("random", raw.to_string(), RANDOM_STR_HINT))
                }
            } else if has_named_args(bound, &pspec) {
                let (min, max) = (named("min"), named("max"));
                let ok = match (min, max) {
                    ("0", "100") => true,
                    (m, "100") if m != "0" => parse_usize_at_least(m, 1).is_some_and(|n| {
                        n <= crate::engine::variables::system::random::MAX_RANDOM_STRING_LEN
                    }),
                    ("0", m) if m != "100" => parse_usize_at_least(m, 1).is_some_and(|n| {
                        n <= crate::engine::variables::system::random::MAX_RANDOM_STRING_LEN
                    }),
                    _ => false,
                };
                if ok {
                    Ok(())
                } else {
                    Err(invalid("random", raw.to_string(), RANDOM_STR_HINT))
                }
            } else {
                Ok(())
            }
        }
    }
}

fn validate_mouse_args(bound: &BoundArgs, raw: &str) -> Result<(), ValidationError> {
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
    let count_raw = bound.named.get("count").map(String::as_str).unwrap_or("1");
    match action {
        "click" => {
            if bound.positional.len() > 3 {
                return Err(invalid("mouse", raw.to_string(), &["mouse click mN [n]"]));
            }
            if count_raw.parse::<u32>().is_err() {
                return Err(invalid(
                    "mouse",
                    count_raw.to_string(),
                    &["mouse click mN [n]"],
                ));
            }
        }
        "hold" | "release" => {
            if bound.positional.len() > 2 || count_raw != "1" {
                return Err(invalid("mouse", raw.to_string(), &["mouse hold mN"]));
            }
        }
        "move" => {
            if bound.positional.len() > 3
                || (bound.positional.len() == 2 && count_raw == "1")
                || (bound.positional.is_empty() && count_raw == "1")
            {
                return Err(invalid("mouse", raw.to_string(), &["mouse move x y"]));
            }
            let x_ok = bound
                .named
                .get("btn")
                .map(String::as_str)
                .unwrap_or("")
                .parse::<u16>()
                .is_ok();
            if !x_ok || count_raw.parse::<u16>().is_err() {
                return Err(invalid("mouse", raw.to_string(), &["mouse move x y"]));
            }
        }
        "scroll" => {
            if bound.positional.len() > 2 || count_raw != "1" {
                return Err(invalid("mouse", raw.to_string(), &["mouse scroll delta"]));
            }
            let delta_ok = bound
                .named
                .get("btn")
                .map(String::as_str)
                .unwrap_or("")
                .parse::<i32>()
                .is_ok();
            if !delta_ok {
                return Err(invalid("mouse", raw.to_string(), &["mouse scroll delta"]));
            }
        }
        "pos" => {
            let btn = bound.named.get("btn").map(String::as_str).unwrap_or("");
            if bound.positional.len() > 1 || !btn.is_empty() || count_raw != "1" {
                return Err(invalid("mouse", raw.to_string(), &["mouse pos"]));
            }
        }
        _ => {}
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

fn validate_date_args(raw: &str) -> Result<(), ValidationError> {
    if raw.is_empty() {
        return Ok(());
    }
    let tokens = crate::engine::variables::parser::tokenize_words(raw);
    for token in tokens {
        let trimmed = crate::engine::variables::system::strip_argument_quotes(&token);
        let lower = trimmed.to_ascii_lowercase();
        if lower == "tomorrow"
            || lower == "yesterday"
            || lower == "today"
            || lower == "utc"
            || lower == "local"
        {
            continue;
        }
        if trimmed.starts_with('+') || trimmed.starts_with('-') {
            if apply_temporal_calc(time::OffsetDateTime::now_utc(), trimmed).is_err() {
                return Err(invalid(
                    "date",
                    token,
                    &["+1d", "-2w", "tomorrow", "yesterday", "utc"],
                ));
            }
            continue;
        }
        if trimmed.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return Err(invalid(
                "date",
                token,
                &["+1d", "-1d", "tomorrow", "yesterday", "utc"],
            ));
        }
    }
    Ok(())
}

fn validate_time_args(raw: &str) -> Result<(), ValidationError> {
    if raw.is_empty() {
        return Ok(());
    }
    let tokens = crate::engine::variables::parser::tokenize_words(raw);
    for token in tokens {
        let trimmed = crate::engine::variables::system::strip_argument_quotes(&token);
        let lower = trimmed.to_ascii_lowercase();
        if lower == "utc" || lower == "local" {
            continue;
        }
        if trimmed.starts_with('+') || trimmed.starts_with('-') {
            if apply_temporal_calc(time::OffsetDateTime::now_utc(), trimmed).is_err() {
                return Err(invalid("time", token, &["+1h", "-30min", "utc"]));
            }
            continue;
        }
        if trimmed.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return Err(invalid("time", token, &["+1h", "-1h", "utc"]));
        }
    }
    Ok(())
}

/// Validates a system call: namespace from
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
            format!("{ns} {raw}")
        };
        return Err(ValidationError::DotChain {
            got,
            hint: dot_hint(&lower),
        });
    }

    if let Some(_hint) = deleted_root_hint(&lower) {
        return Err(ValidationError::UnknownRoot(ns.to_string()));
    }

    let Some(spec) = NS_SPECS.iter().find(|s| s.name == lower) else {
        return Err(ValidationError::UnknownRoot(ns.to_string()));
    };

    if spec.name == "ip" {
        if !raw.is_empty() {
            return Err(ValidationError::UnexpectedModifier {
                root: "ip",
                modifier: raw.to_string(),
            });
        }
        return Ok(());
    }

    if spec.name == "cursor" {
        if !raw.is_empty() {
            return Err(ValidationError::UnexpectedModifier {
                root: "cursor",
                modifier: raw.to_string(),
            });
        }
        return Ok(());
    }

    if spec.name == "newline" {
        if !raw.is_empty() && raw.parse::<usize>().is_err() {
            return Err(ValidationError::InvalidModifier {
                root: "newline",
                modifier: raw.to_string(),
                allowed: &["newline", "newline 2"],
            });
        }
        return Ok(());
    }

    if spec.name == "date" {
        return validate_date_args(raw);
    }

    if spec.name == "time" {
        return validate_time_args(raw);
    }

    if spec.name == "run" {
        if raw.is_empty() {
            return Err(ValidationError::MissingModifier { root: "run" });
        }
        return match crate::engine::variables::system::execute::parse_run_invocation(raw) {
            Ok(_) => Ok(()),
            Err(_) => Err(invalid(
                "run",
                raw.to_string(),
                &["run \"cmd\"", "run silent \"cmd\""],
            )),
        };
    }

    if spec.name == "wait" {
        if raw.is_empty() {
            return Err(ValidationError::MissingModifier { root: "wait" });
        }
        if crate::engine::variables::system::parse_wait_directive(raw).is_none()
            && crate::engine::variables::system::parse_delay_ms(raw).is_none()
            && raw.parse::<u64>().is_err()
        {
            return Err(invalid(
                "wait",
                raw.to_string(),
                &["wait 200", "wait 200ms", "wait 1.5s"],
            ));
        }
        return Ok(());
    }

    if spec.name == "calc" {
        if raw.is_empty() {
            return Err(ValidationError::MissingModifier { root: "calc" });
        }
        if crate::engine::math::evaluate(raw).is_none() {
            return Err(invalid("calc", raw.to_string(), &["calc 10 * 5"]));
        }
        return Ok(());
    }

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

    if !spec.variadic && bound.positional.len() > spec.params.len() {
        return Err(ValidationError::InvalidModifier {
            root: spec.name,
            modifier: raw.to_string(),
            allowed: spec.names,
        });
    }

    if spec.name == "mouse" {
        validate_mouse_args(&bound, raw)
    } else {
        validate_value_args(spec.name, &bound, spec, raw)
    }
}
