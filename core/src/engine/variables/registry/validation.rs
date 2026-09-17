use super::super::parser::{BindError, BoundArgs, Param, ParamSpec, bind_call, has_named_args};
use super::super::system::datetime::apply_temporal_calc;
use super::super::system::execute::{ExecuteParseError, parse_invocation};
use super::super::system::file::check_args as check_file_args;
use super::super::system::{parse_delay_ms, strip_argument_quotes, strip_quotes};
use crate::keys::{LogicalKey, Modifier};
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
    let sub_base = sub.split('(').next().unwrap_or(sub);
    match root {
        "mouse" => match sub_base {
            "rclick" => "mouse(click, m2)",
            "mclick" => "mouse(click, m3)",
            "m4" => "mouse(click, m4)",
            "m5" => "mouse(click, m5)",
            "dblclick" => "mouse(click, m1, 2)",
            "down" | "hold" => "mouse(hold, m1)",
            "up" | "release" => "mouse(release, m1)",
            "move" => "mouse(move, x, y)",
            "scroll" => "mouse(scroll, delta)",
            "pos" => "mouse(pos)",
            "click" => "mouse(click, m1)",
            _ => "mouse(click, mN[, n])",
        }
        .to_string(),
        "date" | "time" | "datetime" => match sub_base {
            "utc" | "local" => format!("chrono({root}, tz={sub_base})"),
            _ => format!("chrono({root})"),
        },
        "clipboard" => "clip".to_string(),
        "net" => match sub_base {
            "publicip" => "ip(public)",
            "localip" => "ip(local)",
            "online" => "http(status, url)",
            _ => "ip",
        }
        .to_string(),
        _ => format!("{root}(...)"),
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
const CHRONO_KINDS: &[&str] = &["date", "time", "datetime"];
const CHRONO_ZONES: &[&str] = &["utc", "local"];
const RANDOM_KINDS: &[&str] = &["int", "choice", "str", "pass"];
const LOREM_KINDS: &[&str] = &["words", "sentences", "paragraphs"];
const HTTP_OPS: &[&str] = &["get", "status"];
const EXECUTE_LANGS: &[&str] = &["bash", "powershell", "python", "node", "cmd"];
const KEY_EXAMPLES: &[&str] = &["enter", "tab", "f5", "ctrl+s"];

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
const HTTP_HINT: &[&str] = &["http(get, url)", "http(status, url)"];
const DELAY_HINT: &[&str] = &["delay(200ms)", "delay(1.5s)"];

fn validate_chrono_args(bound: &BoundArgs, spec: &NsSpec) -> Result<(), ValidationError> {
    let pspec = ParamSpec {
        params: spec.params,
    };
    let named = |key: &str| bound.named.get(key).map(String::as_str).unwrap_or("");
    if bound.positional.len() == 1 && !has_named_args(bound, &pspec) {
        let token = bound.positional[0].trim();
        let lower = token.to_ascii_lowercase();
        if token.starts_with('+') || token.starts_with('-') {
            return Ok(());
        }
        if matches!(
            lower.as_str(),
            "utc" | "local" | "date" | "time" | "datetime"
        ) {
            return Ok(());
        }
        if token.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return Err(invalid(
                "chrono",
                token.to_string(),
                &[
                    "date", "time", "datetime", "utc", "local", "+offset", "format",
                ],
            ));
        }
        return Ok(());
    }
    if !CHRONO_KINDS.contains(&named("type").to_ascii_lowercase().as_str()) {
        return Err(invalid("chrono", named("type").to_string(), CHRONO_KINDS));
    }
    if !CHRONO_ZONES.contains(&named("tz").to_ascii_lowercase().as_str()) {
        return Err(invalid("chrono", named("tz").to_string(), CHRONO_ZONES));
    }
    let offset = named("offset");
    if !offset.eq_ignore_ascii_case("none")
        && apply_temporal_calc(time::OffsetDateTime::now_utc(), offset).is_err()
    {
        return Err(invalid("chrono", offset.to_string(), &["none", "+offset"]));
    }
    Ok(())
}

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
        "http" => {
            if !HTTP_OPS.contains(&named("op")) {
                return Err(invalid("http", named("op").to_string(), HTTP_OPS));
            }
            let url = strip_quotes(named("url").trim()).unwrap_or(named("url").trim());
            if url.is_empty() {
                return Err(invalid("http", raw.to_string(), HTTP_HINT));
            }
            Ok(())
        }
        "delay" => {
            let ms = strip_quotes(raw.trim()).unwrap_or(raw.trim());
            if parse_delay_ms(ms).is_some() {
                Ok(())
            } else {
                Err(invalid("delay", raw.to_string(), DELAY_HINT))
            }
        }
        "key" => {
            let token = strip_argument_quotes(raw.trim()).to_ascii_lowercase();
            let parts: Vec<&str> = token.split('+').collect();
            if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
                return Err(invalid("key", raw.to_string(), KEY_EXAMPLES));
            }
            let main_valid = parts
                .last()
                .and_then(|part| LogicalKey::from_alias(part))
                .is_some_and(|k| k.is_mouse_button().is_none());
            let mods_valid = parts[..parts.len().saturating_sub(1)]
                .iter()
                .all(|m| Modifier::from_alias(m).is_some());
            if main_valid && mods_valid {
                Ok(())
            } else {
                Err(invalid("key", raw.to_string(), KEY_EXAMPLES))
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
                return Err(invalid(
                    "mouse",
                    raw.to_string(),
                    &["mouse(click, mN[, n])"],
                ));
            }
            if count_raw.parse::<u32>().is_err() {
                return Err(invalid(
                    "mouse",
                    count_raw.to_string(),
                    &["mouse(click, mN[, n])"],
                ));
            }
        }
        "hold" | "release" => {
            if bound.positional.len() > 2 || count_raw != "1" {
                return Err(invalid("mouse", raw.to_string(), &["mouse(hold, mN)"]));
            }
        }
        "move" => {
            if bound.positional.len() > 3
                || (bound.positional.len() == 2 && count_raw == "1")
                || (bound.positional.is_empty() && count_raw == "1")
            {
                return Err(invalid("mouse", raw.to_string(), &["mouse(move, x, y)"]));
            }
            let x_ok = bound
                .named
                .get("btn")
                .map(String::as_str)
                .unwrap_or("")
                .parse::<u16>()
                .is_ok();
            if !x_ok || count_raw.parse::<u16>().is_err() {
                return Err(invalid("mouse", raw.to_string(), &["mouse(move, x, y)"]));
            }
        }
        "scroll" => {
            if bound.positional.len() > 2 || count_raw != "1" {
                return Err(invalid("mouse", raw.to_string(), &["mouse(scroll, delta)"]));
            }
            let delta_ok = bound
                .named
                .get("btn")
                .map(String::as_str)
                .unwrap_or("")
                .parse::<i32>()
                .is_ok();
            if !delta_ok {
                return Err(invalid("mouse", raw.to_string(), &["mouse(scroll, delta)"]));
            }
        }
        "pos" => {
            let btn = bound.named.get("btn").map(String::as_str).unwrap_or("");
            if bound.positional.len() > 1 || !btn.is_empty() || count_raw != "1" {
                return Err(invalid("mouse", raw.to_string(), &["mouse(pos)"]));
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
            // Execute parses through its own peel-then-bind cycle (variadic argv
            // overflows the flag slots, which the generic binder below would
            // misread as duplicates). Missing subjects keep the binder error shape.
            if spec.name == "execute" {
                return match parse_invocation(raw) {
                    Ok(_) => Ok(()),
                    Err(ExecuteParseError::MissingSubject) => {
                        Err(ValidationError::MissingModifier { root: spec.name })
                    }
                    Err(_) => Err(invalid("execute", raw.to_string(), EXECUTE_LANGS)),
                };
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
            if spec.name == "chrono" {
                validate_chrono_args(&bound, spec)?;
            }
            if spec.name == "mouse" {
                validate_mouse_args(&bound, raw)
            } else {
                validate_value_args(spec.name, &bound, spec, raw)
            }
        }
    }
}
