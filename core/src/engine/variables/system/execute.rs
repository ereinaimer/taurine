use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, ScriptMetadata, compress};

#[derive(Debug, Clone, PartialEq)]
pub struct ExecuteInvocation {
    pub silent: bool,
    pub interpreter: ScriptInterpreter,
    pub file: bool,
    pub subject: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecuteParseError {
    MissingSubject,
    InvalidLanguage,
    InvalidTrailingSyntax,
}

/// Parses a unified `lang, subject, *args` argument list (no `execute(...)` wrapper).
pub fn parse_invocation(raw: &str) -> Result<ExecuteInvocation, ExecuteParseError> {
    parse_unified_invocation(raw)
}

fn execute_param_spec() -> crate::engine::variables::parser::ParamSpec<'static> {
    use crate::engine::variables::parser::Param;
    crate::engine::variables::parser::ParamSpec {
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
    }
}

fn parse_unified_invocation(raw: &str) -> Result<ExecuteInvocation, ExecuteParseError> {
    use crate::engine::variables::parser::BindError;

    let spec = execute_param_spec();
    // Variadic argv overflows into the flag slots, so a trailing
    // `file=`/`silent=` collides (Duplicate). Peel trailing flags, bind the
    // positional skeleton, then apply the flags.
    let mut head = raw.to_string();
    let mut file_flag: Option<String> = None;
    let mut silent_flag: Option<String> = None;
    let bound = loop {
        match crate::engine::variables::parser::bind_call("execute", &head, &spec) {
            Ok(bound) => break bound,
            Err(BindError::Duplicate { param, .. }) if param == "file" || param == "silent" => {
                let (rest, key, value) = peel_trailing_flag(&head)?;
                let slot = if key == "file" {
                    &mut file_flag
                } else {
                    &mut silent_flag
                };
                if slot.is_some() {
                    return Err(ExecuteParseError::InvalidTrailingSyntax);
                }
                *slot = Some(value);
                head = rest;
            }
            Err(BindError::MissingRequired { param, .. }) => {
                return Err(if param == "lang" {
                    ExecuteParseError::InvalidLanguage
                } else if param == "subject" {
                    ExecuteParseError::MissingSubject
                } else {
                    ExecuteParseError::InvalidTrailingSyntax
                });
            }
            Err(_) => return Err(ExecuteParseError::InvalidTrailingSyntax),
        }
    };

    let lang = bound
        .positional
        .first()
        .ok_or(ExecuteParseError::InvalidLanguage)?;
    let interpreter = match lang.as_str() {
        "bash" => ScriptInterpreter::Bash,
        "powershell" => ScriptInterpreter::PowerShell,
        "python" => ScriptInterpreter::Python,
        "node" => ScriptInterpreter::Node,
        "cmd" => ScriptInterpreter::Cmd,
        _ => return Err(ExecuteParseError::InvalidLanguage),
    };
    let subject = bound
        .positional
        .get(1)
        .cloned()
        .ok_or(ExecuteParseError::MissingSubject)?;
    let has_named = crate::engine::variables::parser::has_named_args(&bound, &spec);
    let file = flag_value(file_flag, 2, "file", &bound, &spec, has_named)?;
    let silent = flag_value(silent_flag, 3, "silent", &bound, &spec, has_named)?;
    Ok(ExecuteInvocation {
        silent,
        interpreter,
        file,
        subject,
        args: bound.positional.iter().skip(2).cloned().collect(),
    })
}

fn peel_trailing_flag(head: &str) -> Result<(String, String, String), ExecuteParseError> {
    let mut parts = crate::engine::variables::parser::tokenize(head, ',');
    let last = parts
        .pop()
        .ok_or(ExecuteParseError::InvalidTrailingSyntax)?;
    let (raw_key, raw_value) = last
        .split_once('=')
        .ok_or(ExecuteParseError::InvalidTrailingSyntax)?;
    let key = crate::engine::variables::system::strip_argument_quotes(raw_key.trim());
    if key != "file" && key != "silent" {
        return Err(ExecuteParseError::InvalidTrailingSyntax);
    }
    let value =
        crate::engine::variables::system::strip_argument_quotes(raw_value.trim()).to_string();
    Ok((parts.join(","), key.to_string(), value))
}

fn flag_value(
    peeled: Option<String>,
    index: usize,
    key: &str,
    bound: &crate::engine::variables::parser::BoundArgs,
    spec: &crate::engine::variables::parser::ParamSpec<'_>,
    has_named: bool,
) -> Result<bool, ExecuteParseError> {
    if let Some(value) = peeled {
        let default = spec.params.get(index).map(|p| p.default).unwrap_or("false");
        let named_in_head =
            bound.positional.len() <= index && bound.named.get(key).is_some_and(|s| s != default);
        if named_in_head {
            return Err(ExecuteParseError::InvalidTrailingSyntax);
        }
        return parse_bool_flag(&value);
    }
    if !has_named || bound.positional.len() > index {
        // Pure positional call, or argv overflow occupies the slot: not a flag.
        return Ok(false);
    }
    parse_bool_flag(bound.named.get(key).map(String::as_str).unwrap_or("false"))
}

fn parse_bool_flag(value: &str) -> Result<bool, ExecuteParseError> {
    match crate::engine::variables::system::strip_argument_quotes(value.trim())
        .to_ascii_lowercase()
        .as_str()
    {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ExecuteParseError::InvalidTrailingSyntax),
    }
}

pub fn parse_run_invocation(raw: &str) -> Result<(bool, String), String> {
    let raw = raw.strip_prefix("run").unwrap_or(raw).trim();
    let tokens = crate::engine::variables::parser::tokenize_words(raw);
    if tokens.is_empty() {
        return Err("missing command for run".to_string());
    }

    let mut silent = false;
    let mut cmd_parts = Vec::new();

    for token in tokens {
        if token.eq_ignore_ascii_case("silent") {
            silent = true;
        } else {
            cmd_parts.push(token);
        }
    }

    let command = cmd_parts.join(" ");
    if cmd_parts.is_empty() || command.trim().is_empty() {
        return Err("missing command for run".to_string());
    }

    Ok((silent, command))
}

pub(crate) fn to_run_script_metadata(key: &str) -> Result<ScriptMetadata, String> {
    let (silent, command) = parse_run_invocation(key)?;
    let interpreter = ScriptInterpreter::default_for_target_os(crate::db::TargetOs::All);
    let behavior = if silent {
        ScriptBehavior::Silent
    } else {
        ScriptBehavior::Inline
    };
    let compressed_content =
        compress(&command).map_err(|e| format!("failed to prepare run script: {e}"))?;
    Ok(ScriptMetadata {
        interpreter,
        behavior,
        compressed_content,
    })
}

pub(crate) fn to_script_metadata(key: &str) -> Result<ScriptMetadata, String> {
    let key_trimmed = key.trim();
    if key_trimmed.starts_with("run") {
        return to_run_script_metadata(key_trimmed);
    }
    Err("invalid run syntax".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_run_metadata() {
        let metadata = to_script_metadata("run \"echo 42\"").unwrap();
        assert_eq!(metadata.behavior, ScriptBehavior::Inline);
        assert_eq!(
            crate::engine::shell::decompress(&metadata.compressed_content).unwrap(),
            "echo 42"
        );

        let silent_meta = to_script_metadata("run silent \"echo 42\"").unwrap();
        assert_eq!(silent_meta.behavior, ScriptBehavior::Silent);
        assert_eq!(
            crate::engine::shell::decompress(&silent_meta.compressed_content).unwrap(),
            "echo 42"
        );
    }

    #[test]
    fn legacy_execute_syntax_rejected() {
        assert!(to_script_metadata("execute(bash, \"echo 42\")").is_err());
        assert!(to_script_metadata("execute(bash, echo 42)").is_err());
    }
}
