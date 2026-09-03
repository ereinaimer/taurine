// Script Executor
use crate::injector;
use std::process::Stdio;
use std::time::Duration;
use taurine_core::engine::shell::{ScriptInterpreter, ScriptMetadata, decompress};
use tokio::process::Command;

use tokio::io::AsyncReadExt;

pub const MAX_SCRIPT_OUTPUT_BYTES: usize = 4 * 1024 * 1024; // 4 MiB stream drain cap

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    Url(String),
    AppOrFile { path: String, args: Vec<String> },
    ComplexScript,
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
        && s.len() >= 2
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn is_url_target(s: &str) -> bool {
    let s = strip_quotes(s);
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("mailto:")
        || s.starts_with("ms-settings:")
        || s.starts_with("file://")
}

pub fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Handle ~ at start or after whitespace/quotes
        if chars[i] == '~'
            && (i == 0 || chars[i - 1] == ' ' || chars[i - 1] == '"' || chars[i - 1] == '\'')
            && (i + 1 == len
                || chars[i + 1] == '/'
                || chars[i + 1] == '\\'
                || chars[i + 1] == ' '
                || chars[i + 1] == '"'
                || chars[i + 1] == '\'')
        {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_else(|_| "~".to_string());
            result.push_str(&home);
            i += 1;
            continue;
        }

        // Handle ${env:VAR} or ${ENV:VAR} or ${VAR}
        if chars[i] == '$'
            && i + 1 < len
            && chars[i + 1] == '{'
            && let Some(close_pos) = chars[i + 2..].iter().position(|&c| c == '}')
        {
            let end = i + 2 + close_pos;
            let var_expr: String = chars[i + 2..end].iter().collect();
            let var_name = if let Some(stripped) = var_expr.strip_prefix("env:") {
                stripped
            } else if let Some(stripped) = var_expr.strip_prefix("ENV:") {
                stripped
            } else {
                &var_expr
            };
            if let Ok(val) = std::env::var(var_name) {
                result.push_str(&val);
                i = end + 1;
                continue;
            }
        }

        // Handle $env:VAR or $ENV:VAR
        if chars[i] == '$' && i + 4 < len {
            let prefix: String = chars[i + 1..i + 5].iter().collect();
            if prefix.eq_ignore_ascii_case("env:") {
                let start = i + 5;
                let mut end = start;
                while end < len {
                    let c = chars[end];
                    if c.is_alphanumeric() || c == '_' || c == '(' || c == ')' || c == '-' {
                        end += 1;
                    } else {
                        break;
                    }
                }
                if end > start {
                    let var_name: String = chars[start..end].iter().collect();
                    if let Ok(val) = std::env::var(&var_name) {
                        result.push_str(&val);
                        i = end;
                        continue;
                    }
                }
            }
        }

        // Handle $HOME / $home
        if chars[i] == '$' && i + 4 <= len {
            let name: String = chars[i + 1..i.saturating_add(5).min(len)].iter().collect();
            if name.eq_ignore_ascii_case("home")
                && (i + 5 == len || (!chars[i + 5].is_alphanumeric() && chars[i + 5] != '_'))
                && let Ok(val) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"))
            {
                result.push_str(&val);
                i += 5;
                continue;
            }
        }

        // Handle %VAR%
        if chars[i] == '%'
            && i + 1 < len
            && let Some(close_pos) = chars[i + 1..].iter().position(|&c| c == '%')
        {
            let end = i + 1 + close_pos;
            let candidate: String = chars[i + 1..end].iter().collect();
            if !candidate.is_empty()
                && candidate
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '(' || c == ')' || c == '-')
                && let Ok(val) = std::env::var(&candidate)
            {
                result.push_str(&val);
                i = end + 1;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

pub fn parse_instant_launch_intent(
    script_content: &str,
    interpreter: ScriptInterpreter,
) -> LaunchTarget {
    let trimmed = script_content.trim();
    if trimmed.is_empty() {
        return LaunchTarget::ComplexScript;
    }

    if trimmed.contains('\n')
        || trimmed.contains('\r')
        || trimmed.contains(';')
        || trimmed.contains('|')
        || trimmed.contains("&&")
    {
        return LaunchTarget::ComplexScript;
    }

    match interpreter {
        ScriptInterpreter::PowerShell => {
            let lower = trimmed.to_lowercase();
            if lower.starts_with("start-process") || lower.starts_with("start ") {
                let rest = if lower.starts_with("start-process") {
                    trimmed["start-process".len()..].trim()
                } else {
                    trimmed["start".len()..].trim()
                };

                if rest.is_empty() {
                    return LaunchTarget::ComplexScript;
                }

                let parts = split_cmd_args(rest);
                if parts.is_empty() {
                    return LaunchTarget::ComplexScript;
                }

                let mut target = None;
                let mut args = Vec::new();
                let mut i = 0;
                while i < parts.len() {
                    let p = &parts[i];
                    if (p.eq_ignore_ascii_case("-filepath") || p.eq_ignore_ascii_case("-file"))
                        && i + 1 < parts.len()
                    {
                        target = Some(strip_quotes(&parts[i + 1]).to_string());
                        i += 2;
                        continue;
                    } else if (p.eq_ignore_ascii_case("-argumentlist")
                        || p.eq_ignore_ascii_case("-args"))
                        && i + 1 < parts.len()
                    {
                        args.push(strip_quotes(&parts[i + 1]).to_string());
                        i += 2;
                        continue;
                    } else if !p.starts_with('-') && target.is_none() {
                        target = Some(strip_quotes(p).to_string());
                    } else if target.is_some() && !p.starts_with('-') {
                        args.push(strip_quotes(p).to_string());
                    }
                    i += 1;
                }

                if let Some(target) = target {
                    let target_expanded = expand_env_vars(&target);
                    if is_url_target(&target_expanded) {
                        return LaunchTarget::Url(target_expanded);
                    }
                    if target_expanded.contains('$') {
                        return LaunchTarget::ComplexScript;
                    }
                    let args = args.into_iter().map(|a| expand_env_vars(&a)).collect();
                    return LaunchTarget::AppOrFile {
                        path: target_expanded,
                        args,
                    };
                }
            }
            LaunchTarget::ComplexScript
        }
        ScriptInterpreter::Cmd => {
            let lower = trimmed.to_lowercase();
            if lower.starts_with("start ") {
                let rest = trimmed["start".len()..].trim();
                let mut parts = split_cmd_args(rest);
                if parts.first().is_some_and(|p| strip_quotes(p).is_empty()) {
                    parts.remove(0);
                }
                if let Some(target) = parts.first() {
                    let target_clean = strip_quotes(target);
                    let target_expanded = expand_env_vars(target_clean);
                    if is_url_target(&target_expanded) {
                        return LaunchTarget::Url(target_expanded);
                    }
                    let args = parts[1..]
                        .iter()
                        .map(|a| expand_env_vars(strip_quotes(a)))
                        .collect();
                    return LaunchTarget::AppOrFile {
                        path: target_expanded,
                        args,
                    };
                }
            }
            LaunchTarget::ComplexScript
        }
        ScriptInterpreter::Bash => {
            let lower = trimmed.to_lowercase();
            if lower.starts_with("open ") || lower.starts_with("xdg-open ") {
                let rest = if lower.starts_with("open ") {
                    trimmed["open".len()..].trim()
                } else {
                    trimmed["xdg-open".len()..].trim()
                };
                let parts = split_cmd_args(rest);
                if let Some(target) = parts.first() {
                    let target_clean = strip_quotes(target);
                    let target_expanded = expand_env_vars(target_clean);
                    if is_url_target(&target_expanded) {
                        return LaunchTarget::Url(target_expanded);
                    }
                    let args = parts[1..]
                        .iter()
                        .map(|a| expand_env_vars(strip_quotes(a)))
                        .collect();
                    return LaunchTarget::AppOrFile {
                        path: target_expanded,
                        args,
                    };
                }
            }
            LaunchTarget::ComplexScript
        }
        _ => LaunchTarget::ComplexScript,
    }
}

fn split_cmd_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    for c in input.chars() {
        match in_quote {
            Some(q) if c == q => {
                current.push(c);
                in_quote = None;
            }
            Some(_) => {
                current.push(c);
            }
            None if c == '"' || c == '\'' => {
                in_quote = Some(c);
                current.push(c);
            }
            None if c.is_whitespace() => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            None => {
                current.push(c);
            }
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

pub fn native_shell_open(target: &str, args: Option<&str>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::ptr;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let wide_verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        let wide_target: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_args: Option<Vec<u16>> =
            args.map(|a| a.encode_utf16().chain(std::iter::once(0)).collect());

        // SAFETY: ShellExecuteW takes valid null-terminated UTF-16 pointers.
        // Memory is valid for the duration of the system call. Return > 32 indicates success.
        unsafe {
            let res = ShellExecuteW(
                ptr::null_mut(),
                wide_verb.as_ptr(),
                wide_target.as_ptr(),
                wide_args
                    .as_ref()
                    .map(|a| a.as_ptr())
                    .unwrap_or(ptr::null()),
                ptr::null(),
                SW_SHOWNORMAL,
            );
            if res as usize > 32 {
                Ok(())
            } else {
                Err(format!(
                    "ShellExecuteW failed with error code: {}",
                    res as usize
                ))
            }
        }
    }
    #[cfg(not(windows))]
    {
        let mut cmd = if cfg!(target_os = "macos") {
            std::process::Command::new("open")
        } else {
            std::process::Command::new("xdg-open")
        };
        cmd.arg(target);
        if let Some(args) = args {
            cmd.args(args.split_whitespace());
        }
        cmd.spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub async fn execute_script(metadata: &ScriptMetadata) -> taurine_core::Result<String> {
    let script_content = decompress(&metadata.compressed_content)?;

    // Check for instant native launch intent
    let launch_intent = parse_instant_launch_intent(&script_content, metadata.interpreter);
    match launch_intent {
        LaunchTarget::Url(url) => {
            if native_shell_open(&url, None).is_ok() {
                return Ok(String::new());
            }
        }
        LaunchTarget::AppOrFile { path, args } => {
            let args_str = if args.is_empty() {
                None
            } else {
                Some(args.join(" "))
            };
            if native_shell_open(&path, args_str.as_deref()).is_ok() {
                return Ok(String::new());
            }
        }
        LaunchTarget::ComplexScript => {}
    }

    // None (script_timeout = 0) means no timeout: the script runs until it finishes.
    let timeout = taurine_core::settings::Settings::get_script_timeout();

    let mut cmd = match metadata.interpreter {
        ScriptInterpreter::Bash => {
            let mut c = Command::new("bash");
            c.arg("-c").arg(&script_content);
            c
        }
        ScriptInterpreter::Python => {
            let mut c = Command::new("python");
            c.arg("-c").arg(&script_content);
            c
        }
        ScriptInterpreter::Node => {
            let mut c = Command::new("node");
            c.arg("-e").arg(&script_content);
            c
        }
        ScriptInterpreter::PowerShell => {
            let mut c = Command::new("powershell");
            // Force UTF-8 stdout so non-ASCII chars (e.g. °, →, ✓) round-trip correctly.
            // PowerShell defaults to the system OEM code page which corrupts Unicode output.
            let utf8_prefix = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; ";
            let full_cmd = format!("{}{}", utf8_prefix, script_content);
            c.arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-Command")
                .arg(&full_cmd);
            c
        }
        ScriptInterpreter::Cmd => {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&script_content);
            c
        }
    };

    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            let interpreter_name = match metadata.interpreter {
                ScriptInterpreter::Bash => "bash",
                ScriptInterpreter::Python => "python",
                ScriptInterpreter::Node => "node",
                ScriptInterpreter::PowerShell => "powershell",
                ScriptInterpreter::Cmd => "cmd",
            };
            taurine_core::Error::Service(format!(
                "interpreter '{}' not found in PATH",
                interpreter_name
            ))
        } else {
            taurine_core::Error::Service(format!("Failed to spawn interpreter: {}", e))
        }
    })?;

    // We take the pipes from the child so we can read them concurrently with wait()
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| taurine_core::Error::Service("Failed to capture stdout".to_string()))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| taurine_core::Error::Service("Failed to capture stderr".to_string()))?;

    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();

    let mut bounded_stdout = stdout_pipe.take(MAX_SCRIPT_OUTPUT_BYTES as u64);
    let mut bounded_stderr = stderr_pipe.take(MAX_SCRIPT_OUTPUT_BYTES as u64);

    // script_timeout = 0 (None) disables the timeout: the sleep branch stays
    // pending forever and the script runs until it finishes on its own.
    let timeout_fut = async {
        if let Some(t) = timeout {
            tokio::time::sleep(t).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::pin!(timeout_fut);

    tokio::select! {
        res = async {
            tokio::join!(
                child.wait(),
                tokio::io::copy(&mut bounded_stdout, &mut stdout_bytes),
                tokio::io::copy(&mut bounded_stderr, &mut stderr_bytes)
            )
        } => {
            let (status_res, _, _) = res;
            let status = status_res.map_err(|e| {
                taurine_core::Error::Service(format!("Failed to wait for script: {}", e))
            })?;

            let stdout_final = String::from_utf8_lossy(&stdout_bytes).trim().to_string();
            let stderr_final = String::from_utf8_lossy(&stderr_bytes).trim().to_string();

            if status.success() {
                Ok(stdout_final)
            } else {
                let err_cleaned = if stderr_final.is_empty() {
                    format!("Script failed with exit code {}", status)
                } else {
                    stderr_final
                };
                Err(taurine_core::Error::Service(err_cleaned))
            }
        }
        _ = &mut timeout_fut => {
            let _ = child.kill().await;
            Err(taurine_core::Error::Service(format!(
                "Script timed out after {}s",
                timeout.map(|t| t.as_secs()).unwrap_or(0)
            )))
        }
        _ = async {
            let captured_gen = injector::capture_generation();
            loop {
                if injector::is_aborted(captured_gen) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        } => {
            let _ = child.kill().await;
            Err(taurine_core::Error::Service("Script aborted by user".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_vars() {
        let _guard = taurine_core::testing::TEST_LOCK.lock().unwrap();
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { std::env::set_var("TAURINE_TEST_DIR", "C:\\TaurineData") };

        assert_eq!(
            expand_env_vars("$ENV:TAURINE_TEST_DIR\\sub"),
            "C:\\TaurineData\\sub"
        );
        assert_eq!(
            expand_env_vars("$env:TAURINE_TEST_DIR\\sub"),
            "C:\\TaurineData\\sub"
        );
        assert_eq!(
            expand_env_vars("${env:TAURINE_TEST_DIR}\\sub"),
            "C:\\TaurineData\\sub"
        );
        assert_eq!(
            expand_env_vars("%TAURINE_TEST_DIR%\\sub"),
            "C:\\TaurineData\\sub"
        );

        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { std::env::remove_var("TAURINE_TEST_DIR") };
    }

    #[test]
    fn test_parse_powershell_start_process_url() {
        assert_eq!(
            parse_instant_launch_intent(
                "Start-Process 'https://github.com'",
                ScriptInterpreter::PowerShell
            ),
            LaunchTarget::Url("https://github.com".to_string())
        );
        assert_eq!(
            parse_instant_launch_intent(
                "Start-Process \"https://github.com\"",
                ScriptInterpreter::PowerShell
            ),
            LaunchTarget::Url("https://github.com".to_string())
        );
        assert_eq!(
            parse_instant_launch_intent("start \"https://google.com\"", ScriptInterpreter::Cmd),
            LaunchTarget::Url("https://google.com".to_string())
        );
        assert_eq!(
            parse_instant_launch_intent("Start-Process notepad.exe", ScriptInterpreter::PowerShell),
            LaunchTarget::AppOrFile {
                path: "notepad.exe".to_string(),
                args: vec![]
            }
        );
    }

    #[test]
    fn test_parse_powershell_start_process_env_var() {
        let _guard = taurine_core::testing::TEST_LOCK.lock().unwrap();
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { std::env::set_var("LOCALAPPDATA", "C:\\Users\\Test\\AppData\\Local") };

        assert_eq!(
            parse_instant_launch_intent(
                "Start-Process $ENV:LOCALAPPDATA\\Taurine",
                ScriptInterpreter::PowerShell
            ),
            LaunchTarget::AppOrFile {
                path: "C:\\Users\\Test\\AppData\\Local\\Taurine".to_string(),
                args: vec![]
            }
        );

        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { std::env::remove_var("LOCALAPPDATA") };
    }

    #[test]
    fn test_parse_powershell_custom_var_falls_back() {
        assert_eq!(
            parse_instant_launch_intent(
                "Start-Process $myCustomPath",
                ScriptInterpreter::PowerShell
            ),
            LaunchTarget::ComplexScript
        );
    }

    #[test]
    fn test_parse_complex_script_falls_back() {
        assert_eq!(
            parse_instant_launch_intent(
                "$x = Get-Process\n$x | Select-Object -First 1",
                ScriptInterpreter::PowerShell
            ),
            LaunchTarget::ComplexScript
        );
    }
}
