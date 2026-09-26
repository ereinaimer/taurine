use std::path::Path;

use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, ScriptMetadata, compress};

const SCRIPT_NOT_FOUND: &str = "path to script not found";

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

/// Strips the `execute(...)` wrapper from a tag inner, returning the raw arg
/// list. Matching is paren-balanced so nested parens in the subject survive.
pub(crate) fn strip_execute_args(key: &str) -> Option<&str> {
    let rest = key.strip_prefix("execute(")?;
    let mut depth = 0u32;
    for (idx, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return rest[idx + 1..].trim().is_empty().then(|| &rest[..idx]);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn parse_unified_invocation(raw: &str) -> Result<ExecuteInvocation, ExecuteParseError> {
    use crate::engine::variables::parser::BindError;

    let spec = crate::engine::variables::registry::param_spec("execute")
        .ok_or(ExecuteParseError::InvalidLanguage)?;
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

pub(crate) fn to_script_metadata(key: &str) -> Result<ScriptMetadata, String> {
    let raw = strip_execute_args(key).unwrap_or(key);
    let mut invocation = parse_invocation(raw).map_err(|_| "invalid exec syntax".to_string())?;

    if invocation.file {
        let subject = invocation.subject.trim();
        if subject.starts_with("asset(") && subject.ends_with(')') {
            let hash = subject[6..subject.len() - 1].trim();

            let conn = crate::db::get_conn().map_err(|e| e.to_string())?;
            let (mime_type, compressed): (String, Vec<u8>) = conn
                .query_row(
                    "SELECT mime_type, compressed_content FROM assets WHERE id = ?1",
                    [hash],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| format!("asset not in DB: {}", e))?;

            let decompressed = crate::engine::shell::decompress_bytes(&compressed)
                .map_err(|e| format!("decompress failed: {}", e))?;

            let ext = match mime_type.as_str() {
                "text/x-shellscript" => "sh",
                "text/x-python" => "py",
                "text/javascript" => "js",
                "text/x-powershell" => "ps1",
                _ => "tmp",
            };

            let temp_path = crate::system::paths::write_temp_file("tau_asset", ext, &decompressed)
                .map_err(|e| format!("write temp script failed: {}", e))?;

            invocation.subject = temp_path.to_string_lossy().to_string();
        } else if !Path::new(subject).exists() {
            return Err(SCRIPT_NOT_FOUND.to_string());
        }
    }

    let content = invocation_script_content(&invocation);
    let compressed_content =
        compress(&content).map_err(|e| format!("failed to prepare exec script: {e}"))?;

    Ok(ScriptMetadata {
        interpreter: invocation.interpreter,
        behavior: if invocation.silent {
            ScriptBehavior::Silent
        } else {
            ScriptBehavior::Inline
        },
        compressed_content,
    })
}

fn invocation_script_content(invocation: &ExecuteInvocation) -> String {
    if !invocation.file {
        return invocation.subject.clone();
    }

    match invocation.interpreter {
        ScriptInterpreter::Bash => shell_command_line(
            bash_file_path_arg(invocation.subject.trim()),
            &invocation.args,
            quote_posix,
        ),
        ScriptInterpreter::PowerShell => {
            let mut command = format!("& {}", quote_powershell(invocation.subject.trim()));
            for arg in &invocation.args {
                command.push(' ');
                command.push_str(&quote_powershell(arg));
            }
            command
        }
        ScriptInterpreter::Python => {
            let path = quote_python(invocation.subject.trim());
            let args = python_list(&invocation.args);
            format!(
                "import runpy, sys\nsys.argv = [{path}, *{args}]\nrunpy.run_path({path}, run_name='__main__')"
            )
        }
        ScriptInterpreter::Node => {
            let path = quote_js(invocation.subject.trim());
            let args = js_array(&invocation.args);
            format!("process.argv = [process.argv[0], {path}, ...{args}]; require({path});")
        }
        ScriptInterpreter::Cmd => {
            shell_command_line(invocation.subject.trim(), &invocation.args, quote_cmd)
        }
    }
}

fn shell_command_line(
    path: impl AsRef<str>,
    args: &[String],
    quote: impl Fn(&str) -> String,
) -> String {
    let mut command = quote(path.as_ref());
    for arg in args {
        command.push(' ');
        command.push_str(&quote(arg));
    }
    command
}

fn quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn quote_cmd(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_python(value: &str) -> String {
    format!("{value:?}")
}

fn quote_js(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn python_list(args: &[String]) -> String {
    format!(
        "[{}]",
        args.iter()
            .map(|arg| quote_python(arg))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn js_array(args: &[String]) -> String {
    format!(
        "[{}]",
        args.iter()
            .map(|arg| quote_js(arg))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(windows)]
fn bash_file_path_arg(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = path[3..].replace('\\', "/");
        format!("/mnt/{drive}/{rest}")
    } else {
        path.to_string()
    }
}

#[cfg(not(windows))]
fn bash_file_path_arg(path: &str) -> &str {
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_command() {
        let parsed = parse_invocation("bash, \"curl -s wttr.in/?format=3\"").unwrap();
        assert!(!parsed.silent);
        assert_eq!(parsed.interpreter, ScriptInterpreter::Bash);
        assert!(!parsed.file);
        assert_eq!(parsed.subject, "curl -s wttr.in/?format=3");
        assert!(parsed.args.is_empty());
    }

    #[test]
    fn parses_silent_file_with_args() {
        let parsed =
            parse_invocation("python, /tmp/script.py, arg1, arg2, file=true, silent=true").unwrap();
        assert!(parsed.silent);
        assert_eq!(parsed.interpreter, ScriptInterpreter::Python);
        assert!(parsed.file);
        assert_eq!(parsed.subject, "/tmp/script.py");
        assert_eq!(parsed.args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn parses_nested_parentheses_in_subject_and_args() {
        let parsed = parse_invocation("node, \"console.log((1 + 2))\", a(b), c").unwrap();
        assert_eq!(parsed.interpreter, ScriptInterpreter::Node);
        assert_eq!(parsed.subject, "console.log((1 + 2))");
        assert_eq!(parsed.args, vec!["a(b)", "c"]);
    }

    #[test]
    fn parses_unified_execute() {
        let p = parse_invocation("bash, \"echo 42\"").unwrap();
        assert_eq!(p.subject, "echo 42");
        assert!(
            parse_invocation("python, /s.py, a, b, file=true")
                .unwrap()
                .file
        );
        assert!(parse_invocation("ruby, \"puts 1\"").is_err());
        assert!(parse_invocation("bash").is_err());
    }

    #[test]
    fn parses_unified_execute_flags_and_argv() {
        let p = parse_invocation("python, /s.py, a, b, silent=TRUE").unwrap();
        assert_eq!(p.interpreter, ScriptInterpreter::Python);
        assert_eq!(p.subject, "/s.py");
        assert_eq!(p.args, vec!["a", "b"]);
        assert!(!p.file);
        assert!(p.silent);
        let p = parse_invocation("bash, s, a, b, file=true, silent=false").unwrap();
        assert!(p.file);
        assert!(!p.silent);
        assert!(parse_invocation("bash, s, file=yes").is_err());
        assert!(parse_invocation("bash, s, bogus=1").is_err());
        assert!(parse_invocation("lang=bash, subject=hi").is_err());
        assert!(parse_invocation("bash, s, file=true, file=true").is_err());
    }

    #[test]
    fn rejects_invalid_execute_syntax() {
        assert_eq!(
            parse_invocation("ruby, puts 1"),
            Err(ExecuteParseError::InvalidLanguage)
        );
        assert_eq!(
            parse_invocation("bash"),
            Err(ExecuteParseError::MissingSubject)
        );
        assert_eq!(
            parse_invocation("bash, s, file=yes"),
            Err(ExecuteParseError::InvalidTrailingSyntax)
        );
    }

    #[test]
    fn missing_file_returns_plan_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.sh");
        let key = format!("execute(bash, {}, file=true)", path.display());

        assert!(to_script_metadata(&key).is_err());
    }

    #[test]
    fn converts_unified_execute_to_script_metadata() {
        let metadata = to_script_metadata("execute(bash, \"echo 42\")").unwrap();
        assert_eq!(metadata.interpreter, ScriptInterpreter::Bash);
        assert_eq!(metadata.behavior, ScriptBehavior::Inline);
        assert_eq!(
            crate::engine::shell::decompress(&metadata.compressed_content).unwrap(),
            "echo 42"
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sh");
        std::fs::write(&path, "echo file:$1\n").unwrap();
        let metadata = to_script_metadata(&format!(
            "execute(bash, {}, ok, file=true, silent=true)",
            path.display()
        ))
        .unwrap();
        let content = crate::engine::shell::decompress(&metadata.compressed_content).unwrap();
        assert_eq!(metadata.behavior, ScriptBehavior::Silent);
        assert!(content.contains("test.sh"));
        assert!(content.contains("'ok'"));
    }

    #[test]
    fn converts_inline_execute_to_script_metadata() {
        let metadata = to_script_metadata("execute(bash, echo 42)").unwrap();
        assert_eq!(metadata.interpreter, ScriptInterpreter::Bash);
        assert_eq!(metadata.behavior, ScriptBehavior::Inline);
        assert_eq!(
            crate::engine::shell::decompress(&metadata.compressed_content).unwrap(),
            "echo 42"
        );
    }

    #[test]
    fn converts_silent_file_execute_to_wrapper_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sh");
        std::fs::write(&path, "echo file:$1\n").unwrap();

        let metadata = to_script_metadata(&format!(
            "execute(bash, {}, ok, file=true, silent=true)",
            path.display()
        ))
        .unwrap();
        let content = crate::engine::shell::decompress(&metadata.compressed_content).unwrap();

        assert_eq!(metadata.behavior, ScriptBehavior::Silent);
        assert!(content.contains("test.sh"));
        assert!(content.contains("'ok'"));
    }

    #[test]
    fn interpolation_keeps_execute_tags_for_finalization() {
        assert_eq!(
            crate::engine::variables::interpolate::interpolate(
                "[execute(bash, echo hi)]",
                &crate::engine::variables::types::ArgMap::default()
            ),
            "[execute(bash, echo hi)]"
        );
    }
}
