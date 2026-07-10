use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, ScriptMetadata, compress};
use wait_timeout::ChildExt;

const SCRIPT_NOT_FOUND: &str = "[Error: path to script not found!]";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExecuteInvocation {
    pub silent: bool,
    pub interpreter: ScriptInterpreter,
    pub file: bool,
    pub subject: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecuteParseError {
    MissingSubject,
    InvalidLanguage,
    UnbalancedParentheses,
    InvalidTrailingSyntax,
}

pub(crate) fn parse_invocation(key: &str) -> Result<ExecuteInvocation, ExecuteParseError> {
    let mut rest = key
        .strip_prefix("exec.")
        .ok_or(ExecuteParseError::InvalidLanguage)?;

    let mut silent = false;
    let mut interpreter = None;
    let mut file = false;
    let mut subject = None;
    let mut args = Vec::new();

    while !rest.is_empty() {
        if let Some(suffix) = rest.strip_prefix("silent") {
            silent = true;
            rest = suffix;
        } else if let Some(suffix) = rest.strip_prefix("args") {
            let (args_str, trailing) = scan_parenthesized(suffix)?;
            args = split_args(&args_str);
            rest = trailing;
        } else if let Some(suffix) = rest.strip_prefix("file") {
            let (file_subj, trailing) = scan_parenthesized(suffix)?;
            if subject.is_some() {
                return Err(ExecuteParseError::InvalidTrailingSyntax);
            }
            subject = Some(
                crate::engine::variables::system::strip_argument_quotes(&file_subj).to_string(),
            );
            file = true;
            rest = trailing;
        } else if let Some((lang, suffix)) = parse_language_only(rest) {
            interpreter = Some(lang);
            rest = suffix;
            if rest.starts_with('(') {
                let (inline_subj, trailing) = scan_parenthesized(rest)?;
                if subject.is_some() {
                    return Err(ExecuteParseError::InvalidTrailingSyntax);
                }
                subject = Some(
                    crate::engine::variables::system::strip_argument_quotes(&inline_subj)
                        .to_string(),
                );
                file = false;
                rest = trailing;
            }
        } else {
            return Err(ExecuteParseError::InvalidTrailingSyntax);
        }

        if !rest.is_empty() {
            if let Some(suffix) = rest.strip_prefix('.') {
                rest = suffix;
            } else {
                return Err(ExecuteParseError::InvalidTrailingSyntax);
            }
        }
    }

    Ok(ExecuteInvocation {
        silent,
        interpreter: interpreter.ok_or(ExecuteParseError::InvalidLanguage)?,
        file,
        subject: subject.ok_or(ExecuteParseError::MissingSubject)?,
        args,
    })
}

pub fn resolve(key: &str) -> Option<String> {
    let invocation = parse_invocation(key).ok()?;

    if invocation.file && !Path::new(invocation.subject.trim()).exists() {
        return Some(SCRIPT_NOT_FOUND.to_string());
    }

    if invocation.silent {
        return Some(spawn_silent(&invocation).unwrap_or_else(format_error));
    }

    Some(execute_inline(&invocation).unwrap_or_else(format_error))
}

pub(crate) fn to_script_metadata(key: &str) -> Result<ScriptMetadata, String> {
    let invocation = parse_invocation(key).map_err(|_| "invalid exec syntax".to_string())?;

    if invocation.file && !Path::new(invocation.subject.trim()).exists() {
        return Err(SCRIPT_NOT_FOUND.to_string());
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

fn parse_language_only(input: &str) -> Option<(ScriptInterpreter, &str)> {
    const LANGUAGES: &[(&str, ScriptInterpreter)] = &[
        ("node_esm", ScriptInterpreter::NodeEsm),
        ("powershell", ScriptInterpreter::PowerShell),
        ("python", ScriptInterpreter::Python),
        ("bash", ScriptInterpreter::Bash),
        ("node", ScriptInterpreter::Node),
        ("cmd", ScriptInterpreter::Cmd),
    ];

    for (name, interpreter) in LANGUAGES {
        if let Some(rest) = input.strip_prefix(name) {
            return Some((*interpreter, rest));
        }
    }

    None
}

fn scan_parenthesized(input: &str) -> Result<(String, &str), ExecuteParseError> {
    if !input.starts_with('(') {
        return Err(ExecuteParseError::MissingSubject);
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
                    return Err(ExecuteParseError::UnbalancedParentheses);
                }
                depth -= 1;
                if depth == 0 {
                    let start = start.ok_or(ExecuteParseError::MissingSubject)?;
                    return Ok((input[start..idx].trim().to_string(), &input[idx + 1..]));
                }
            }
            _ => {}
        }
    }

    Err(ExecuteParseError::UnbalancedParentheses)
}

fn split_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;

    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                push_arg(&mut args, &input[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    push_arg(&mut args, &input[start..]);
    args
}

fn push_arg(args: &mut Vec<String>, raw: &str) {
    let trimmed = crate::engine::variables::system::strip_argument_quotes(raw);
    if !trimmed.is_empty() {
        args.push(trimmed.to_string());
    }
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
        ScriptInterpreter::NodeEsm => {
            let path = quote_js(invocation.subject.trim());
            let args = js_array(&invocation.args);
            format!(
                "import {{ pathToFileURL }} from 'url'; process.argv = [process.argv[0], {path}, ...{args}]; await import(pathToFileURL({path}).href);"
            )
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
        .expect("Failed to spawn stdout reader thread");
    let stderr_reader = thread::Builder::new()
        .name("tau-stderr-rd".to_string())
        .spawn(move || read_pipe(stderr))
        .expect("Failed to spawn stderr reader thread");

    let timeout_opt = crate::settings::Settings::get_script_timeout();

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
            Err("Script timed out after 20s".to_string())
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
        ScriptInterpreter::NodeEsm => {
            let mut command = Command::new("node");
            if invocation.file {
                command.arg(invocation.subject.trim());
            } else {
                command
                    .arg("--input-type=module")
                    .arg("-e")
                    .arg(&invocation.subject);
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

fn read_pipe(mut pipe: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)
        .map_err(|e| format!("Failed to read script output: {e}"))?;
    Ok(bytes)
}

fn join_reader(handle: thread::JoinHandle<Result<Vec<u8>, String>>) -> Result<Vec<u8>, String> {
    handle
        .join()
        .map_err(|_| "Failed to join script output reader".to_string())?
}

fn format_error(error: String) -> String {
    format!("[Error: {error}]")
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
        let parsed = parse_invocation("exec.bash(curl -s wttr.in/?format=3)").unwrap();
        assert!(!parsed.silent);
        assert_eq!(parsed.interpreter, ScriptInterpreter::Bash);
        assert!(!parsed.file);
        assert_eq!(parsed.subject, "curl -s wttr.in/?format=3");
        assert!(parsed.args.is_empty());
    }

    #[test]
    fn parses_silent_file_with_args() {
        let parsed =
            parse_invocation("exec.silent.python.file(/tmp/script.py).args(arg1, arg2)").unwrap();
        assert!(parsed.silent);
        assert_eq!(parsed.interpreter, ScriptInterpreter::Python);
        assert!(parsed.file);
        assert_eq!(parsed.subject, "/tmp/script.py");
        assert_eq!(parsed.args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn parses_nested_parentheses_in_subject_and_args() {
        let parsed = parse_invocation("exec.node(console.log((1 + 2))).args(a(b), c)").unwrap();
        assert_eq!(parsed.interpreter, ScriptInterpreter::Node);
        assert_eq!(parsed.subject, "console.log((1 + 2))");
        assert_eq!(parsed.args, vec!["a(b)", "c"]);
    }

    #[test]
    fn rejects_invalid_execute_syntax() {
        assert_eq!(
            parse_invocation("exec.ruby(puts 1)"),
            Err(ExecuteParseError::InvalidTrailingSyntax)
        );
        assert_eq!(
            parse_invocation("exec.bash(echo 1"),
            Err(ExecuteParseError::UnbalancedParentheses)
        );
        assert_eq!(
            parse_invocation("exec.bash"),
            Err(ExecuteParseError::MissingSubject)
        );
    }

    #[test]
    fn execute_bash_echo_resolves_stdout() {
        if !bash_available() {
            eprintln!("skipping bash execution test because bash is unavailable");
            return;
        }

        assert_eq!(resolve("exec.bash(echo 42)").unwrap(), "42");
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

        let key = format!("exec.bash.file({}).args(ok)", path.display());
        assert_eq!(resolve(&key).unwrap(), "file:ok");
    }

    #[test]
    fn missing_file_returns_plan_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.sh");
        let key = format!("exec.bash.file({})", path.display());

        assert_eq!(resolve(&key).unwrap(), SCRIPT_NOT_FOUND);
    }

    #[test]
    fn converts_inline_execute_to_script_metadata() {
        let metadata = to_script_metadata("exec.bash(echo 42)").unwrap();
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
            "exec.silent.bash.file({}).args(ok)",
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
        let output = resolve("exec.silent.bash(sleep 5)").unwrap();

        assert_eq!(output, "");
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn interpolation_keeps_execute_tags_for_finalization() {
        assert_eq!(
            crate::engine::variables::interpolate::interpolate(
                "[exec.bash(echo hi)]",
                &crate::engine::variables::types::ArgMap::default()
            ),
            "[exec.bash(echo hi)]"
        );
    }
}
