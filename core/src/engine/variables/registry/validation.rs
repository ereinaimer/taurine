use super::super::system;
use super::{
    DATE_METHODS, EXEC_MODIFIERS, FILE_MODIFIERS, KEY_MODIFIERS, LOREM_MODIFIERS, NET_MODIFIERS,
    RANDOM_MODIFIERS, TIME_METHODS, UUID_MODIFIERS,
};
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

pub fn validate_system_tag(root: &str, modifier: Option<&str>) -> Result<(), ValidationError> {
    match root {
        "newline" => validate_no_modifier("newline", modifier),
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
