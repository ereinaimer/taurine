use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::engine::shell::ScriptInterpreter;
use wait_timeout::ChildExt;

const RUN_TIMEOUT: Duration = Duration::from_secs(20);
const SCRIPT_NOT_FOUND: &str = "[Error: path to script not found!]";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RunInvocation {
    pub silent: bool,
    pub interpreter: ScriptInterpreter,
    pub file: bool,
    pub subject: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunParseError {
    MissingSubject,
    InvalidLanguage,
    UnbalancedParentheses,
    InvalidTrailingSyntax,
}

pub(crate) fn parse_invocation(key: &str) -> Result<RunInvocation, RunParseError> {
    let mut rest = key
        .strip_prefix("run.")
        .ok_or(RunParseError::InvalidLanguage)?;
    let silent = if let Some(suffix) = rest.strip_prefix("silent.") {
        rest = suffix;
        true
    } else {
        false
    };

    let (language, rest) = parse_language(rest)?;
    let (file, rest) = if let Some(suffix) = rest.strip_prefix(".file") {
        (true, suffix)
    } else {
        (false, rest)
    };

    let (subject, rest) = scan_parenthesized(rest)?;
    let (args, rest) = if let Some(suffix) = rest.strip_prefix(".args") {
        let (args_subject, trailing) = scan_parenthesized(suffix)?;
        (split_args(&args_subject), trailing)
    } else {
        (Vec::new(), rest)
    };

    if !rest.trim().is_empty() {
        return Err(RunParseError::InvalidTrailingSyntax);
    }

    Ok(RunInvocation {
        silent,
        interpreter: language,
        file,
        subject,
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

fn parse_language(input: &str) -> Result<(ScriptInterpreter, &str), RunParseError> {
    const LANGUAGES: &[(&str, ScriptInterpreter)] = &[
        ("node_esm", ScriptInterpreter::NodeEsm),
        ("powershell", ScriptInterpreter::PowerShell),
        ("python", ScriptInterpreter::Python),
        ("bash", ScriptInterpreter::Bash),
        ("node", ScriptInterpreter::Node),
        ("cmd", ScriptInterpreter::Cmd),
    ];

    for (name, interpreter) in LANGUAGES {
        if let Some(rest) = input.strip_prefix(name)
            && (rest.starts_with('(') || rest.starts_with(".file"))
        {
            return Ok((*interpreter, rest));
        }
    }

    Err(RunParseError::InvalidLanguage)
}

fn scan_parenthesized(input: &str) -> Result<(String, &str), RunParseError> {
    if !input.starts_with('(') {
        return Err(RunParseError::MissingSubject);
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
                    return Err(RunParseError::UnbalancedParentheses);
                }
                depth -= 1;
                if depth == 0 {
                    let start = start.ok_or(RunParseError::MissingSubject)?;
                    return Ok((input[start..idx].trim().to_string(), &input[idx + 1..]));
                }
            }
            _ => {}
        }
    }

    Err(RunParseError::UnbalancedParentheses)
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
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        args.push(trimmed.to_string());
    }
}

fn execute_inline(invocation: &RunInvocation) -> Result<String, String> {
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

    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));

    match child
        .wait_timeout(RUN_TIMEOUT)
        .map_err(|e| format!("Failed to wait for script: {e}"))?
    {
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

fn spawn_silent(invocation: &RunInvocation) -> Result<String, String> {
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

fn build_command(invocation: &RunInvocation) -> Command {
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
    use crate::engine::variables::interpolate::interpolate;
    use crate::engine::variables::types::ArgMap;
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
        let parsed = parse_invocation("run.bash(curl -s wttr.in/?format=3)").unwrap();
        assert!(!parsed.silent);
        assert_eq!(parsed.interpreter, ScriptInterpreter::Bash);
        assert!(!parsed.file);
        assert_eq!(parsed.subject, "curl -s wttr.in/?format=3");
        assert!(parsed.args.is_empty());
    }

    #[test]
    fn parses_silent_file_with_args() {
        let parsed =
            parse_invocation("run.silent.python.file(/tmp/script.py).args(arg1, arg2)").unwrap();
        assert!(parsed.silent);
        assert_eq!(parsed.interpreter, ScriptInterpreter::Python);
        assert!(parsed.file);
        assert_eq!(parsed.subject, "/tmp/script.py");
        assert_eq!(parsed.args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn parses_nested_parentheses_in_subject_and_args() {
        let parsed = parse_invocation("run.node(console.log((1 + 2))).args(a(b), c)").unwrap();
        assert_eq!(parsed.interpreter, ScriptInterpreter::Node);
        assert_eq!(parsed.subject, "console.log((1 + 2))");
        assert_eq!(parsed.args, vec!["a(b)", "c"]);
    }

    #[test]
    fn rejects_invalid_run_syntax() {
        assert_eq!(
            parse_invocation("run.ruby(puts 1)"),
            Err(RunParseError::InvalidLanguage)
        );
        assert_eq!(
            parse_invocation("run.bash(echo 1"),
            Err(RunParseError::UnbalancedParentheses)
        );
        assert_eq!(
            parse_invocation("run.bash"),
            Err(RunParseError::InvalidLanguage)
        );
    }

    #[test]
    fn run_bash_echo_resolves_stdout() {
        if !bash_available() {
            eprintln!("skipping bash execution test because bash is unavailable");
            return;
        }

        assert_eq!(resolve("run.bash(echo 42)").unwrap(), "42");
    }

    #[test]
    fn run_bash_file_executes_script() {
        if !bash_available() {
            eprintln!("skipping bash file test because bash is unavailable");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sh");
        std::fs::write(&path, "echo file:$1\n").unwrap();

        let key = format!("run.bash.file({}).args(ok)", path.display());
        assert_eq!(resolve(&key).unwrap(), "file:ok");
    }

    #[test]
    fn missing_file_returns_plan_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.sh");
        let key = format!("run.bash.file({})", path.display());

        assert_eq!(resolve(&key).unwrap(), SCRIPT_NOT_FOUND);
    }

    #[test]
    fn run_silent_bash_returns_immediately() {
        if !bash_available() {
            eprintln!("skipping silent bash test because bash is unavailable");
            return;
        }

        let start = Instant::now();
        let output = resolve("run.silent.bash(sleep 5)").unwrap();

        assert_eq!(output, "");
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn transformer_applies_to_run_output() {
        if !bash_available() {
            eprintln!("skipping transformer execution test because bash is unavailable");
            return;
        }

        assert_eq!(
            interpolate("[run.bash(echo hi).upper]", &ArgMap::default()),
            "HI"
        );
    }
}
