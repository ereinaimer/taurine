use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, ScriptMetadata, compress};
use wait_timeout::ChildExt;

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
    // honey: variadic argv overflows into the flag slots, so a trailing
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
        // honey: pure positional call, or argv overflow occupies the slot: not a flag.
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

pub fn resolve(key: &str) -> Option<String> {
    let raw = strip_execute_args(key).unwrap_or(key);
    let invocation = parse_invocation(raw).ok()?;

    if invocation.file && !Path::new(invocation.subject.trim()).exists() {
        tracing::warn!("Script file not found: '{}'", invocation.subject.trim());
        return None;
    }

    if invocation.silent {
        return match spawn_silent(&invocation) {
            Ok(output) => Some(output),
            Err(e) => {
                tracing::warn!("Failed to spawn silent script: {}", e);
                None
            }
        };
    }

    match execute_inline(&invocation) {
        Ok(output) => Some(output),
        Err(e) => {
            tracing::warn!("Inline script execution failed: {}", e);
            None
        }
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

fn execute_inline(invocation: &ExecuteInvocation) -> Result<String, String> {
    let mut command = build_command(invocation);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn interpreter: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture stderr".to_string())?;

    let stdout_reader = thread::Builder::new()
        .name("tau-stdout-rd".to_string())
        .spawn(move || read_pipe(stdout))
        .map_err(|e| format!("Failed to spawn stdout reader thread: {e}"))?;
    let stderr_reader = thread::Builder::new()
        .name("tau-stderr-rd".to_string())
        .spawn(move || read_pipe(stderr))
        .map_err(|e| format!("Failed to spawn stderr reader thread: {e}"))?;

    let timeout_opt = crate::settings::Settings::get_script_timeout();
    let timeout_secs = timeout_opt.map(|t| t.as_secs());

    let wait_result = match timeout_opt {
        Some(timeout) => child
            .wait_timeout(timeout)
            .map_err(|e| format!("Failed to wait for script: {e}")),
        None => child
            .wait()
            .map(Some)
            .map_err(|e| format!("Failed to wait for script: {e}")),
    };

    match wait_result? {
        Some(status) => {
            let stdout = join_reader(stdout_reader)?;
            let stderr = join_reader(stderr_reader)?;

            if status.success() {
                Ok(String::from_utf8_lossy(&stdout).trim().to_string())
            } else {
                let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
                if stderr.is_empty() {
                    Err(format!("Script failed with exit code {status}"))
                } else {
                    Err(stderr)
                }
            }
        }
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_reader(stdout_reader);
            let _ = join_reader(stderr_reader);
            Err(format!(
                "Script timed out after {}s",
                timeout_secs.unwrap_or(0)
            ))
        }
    }
}

fn spawn_silent(invocation: &ExecuteInvocation) -> Result<String, String> {
    let mut command = build_command(invocation);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached(&mut command);
    command
        .spawn()
        .map_err(|e| format!("Failed to spawn interpreter: {e}"))?;
    Ok(String::new())
}

fn build_command(invocation: &ExecuteInvocation) -> Command {
    match invocation.interpreter {
        ScriptInterpreter::Bash => {
            let mut command = Command::new("bash");
            if invocation.file {
                command.arg(bash_file_path_arg(invocation.subject.trim()));
            } else {
                command.arg("-c").arg(&invocation.subject);
            }
            command.args(&invocation.args);
            command
        }
        ScriptInterpreter::Python => {
            let mut command = Command::new("python");
            if invocation.file {
                command.arg(invocation.subject.trim());
            } else {
                command.arg("-c").arg(&invocation.subject);
            }
            command.args(&invocation.args);
            command
        }
        ScriptInterpreter::Node => {
            let mut command = Command::new("node");
            if invocation.file {
                command.arg(invocation.subject.trim());
            } else {
                command.arg("-e").arg(&invocation.subject);
            }
            command.args(&invocation.args);
            command
        }
        ScriptInterpreter::PowerShell => {
            let mut command = Command::new("powershell");
            command
                .arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass");
            if invocation.file {
                command.arg("-File").arg(invocation.subject.trim());
            } else {
                command.arg("-Command").arg(&invocation.subject);
            }
            command.args(&invocation.args);
            command
        }
        ScriptInterpreter::Cmd => {
            let mut command = Command::new("cmd");
            if invocation.file {
                command.arg("/C").arg(invocation.subject.trim());
            } else {
                command.arg("/C").arg(&invocation.subject);
            }
            command.args(&invocation.args);
            command
        }
    }
}

#[cfg(windows)]
fn configure_detached(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}

#[cfg(not(windows))]
fn configure_detached(_command: &mut Command) {}

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

pub const MAX_SCRIPT_OUTPUT_BYTES: usize = 4 * 1024 * 1024; // 4 MiB stream drain cap

fn read_pipe(pipe: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    pipe.take(MAX_SCRIPT_OUTPUT_BYTES as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Failed to read script output: {e}"))?;
    Ok(bytes)
}

fn join_reader(handle: thread::JoinHandle<Result<Vec<u8>, String>>) -> Result<Vec<u8>, String> {
    handle
        .join()
        .map_err(|_| "Failed to join script output reader".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn bash_available() -> bool {
        Command::new("bash")
            .arg("-lc")
            .arg("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

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
    fn execute_bash_echo_resolves_stdout() {
        if !bash_available() {
            eprintln!("skipping bash execution test because bash is unavailable");
            return;
        }

        assert_eq!(resolve("execute(bash, echo 42)").unwrap(), "42");
    }

    #[test]
    fn execute_bash_file_executes_script() {
        if !bash_available() {
            eprintln!("skipping bash file test because bash is unavailable");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sh");
        std::fs::write(&path, "echo file:$1\n").unwrap();

        let key = format!("execute(bash, {}, ok, file=true)", path.display());
        assert_eq!(resolve(&key).unwrap(), "file:ok");
    }

    #[test]
    fn missing_file_returns_plan_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.sh");
        let key = format!("execute(bash, {}, file=true)", path.display());

        assert_eq!(resolve(&key), None);
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
    fn execute_unified_bash_resolves_stdout() {
        if !bash_available() {
            eprintln!("skipping bash execution test because bash is unavailable");
            return;
        }

        assert_eq!(resolve("execute(bash, \"echo 42\")").unwrap(), "42");
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
    fn execute_silent_bash_returns_immediately() {
        if !bash_available() {
            eprintln!("skipping silent bash test because bash is unavailable");
            return;
        }

        let start = Instant::now();
        let output = resolve("execute(bash, sleep 5, silent=true)").unwrap();

        assert_eq!(output, "");
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
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
