// Script Executor
use crate::injector;
use std::process::Stdio;
use std::time::Duration;
use taurine_core::engine::shell::{ScriptInterpreter, ScriptMetadata, decompress};
use tokio::process::Command;

use tokio::io::AsyncReadExt;

pub const MAX_SCRIPT_OUTPUT_BYTES: usize = 4 * 1024 * 1024; // 4 MiB stream drain cap

pub async fn execute_script(metadata: &ScriptMetadata) -> taurine_core::Result<String> {
    let script_content = decompress(&metadata.compressed_content)?;
    let timeout =
        taurine_core::settings::Settings::get_script_timeout().unwrap_or(Duration::from_secs(20));

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
        _ = tokio::time::sleep(timeout) => {
            let _ = child.kill().await;
            Err(taurine_core::Error::Service(format!(
                "Script timed out after {}s",
                timeout.as_secs()
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
