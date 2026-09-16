use super::super::parser::{BindError, BoundArgs, Param, ParamSpec, bind_call};
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
            if spec.name == "chrono"
                && let Some(t) = bound.named.get("type")
            {
                let lower_t = t.to_ascii_lowercase();
                let is_offset = t.starts_with('+') || t.starts_with('-');
                let is_tz = matches!(lower_t.as_str(), "utc" | "local");
                let is_kind = matches!(lower_t.as_str(), "date" | "time" | "datetime" | "");
                if !is_offset && !is_tz && !is_kind {
                    return Err(ValidationError::InvalidModifier {
                        root: "chrono",
                        modifier: t.to_string(),
                        allowed: &["date", "time", "datetime", "utc", "local", "<offset>"],
                    });
                }
            }
            if spec.name == "ip"
                && let Some(t) = bound.named.get("type")
            {
                let lower_t = t.to_ascii_lowercase();
                if !matches!(lower_t.as_str(), "public" | "local" | "online" | "") {
                    return Err(ValidationError::InvalidModifier {
                        root: "ip",
                        modifier: t.to_string(),
                        allowed: &["public", "local", "online"],
                    });
                }
            }
            if spec.name == "mouse" {
                validate_mouse_args(&bound)
            } else {
                Ok(())
            }
        }
    }
}
