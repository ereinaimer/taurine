use std::fs;
use std::path::PathBuf;
use taurine_core::db::crud::{upsert_automation, upsert_script};
use taurine_core::db::init;
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter, compress};
use tracing::info;

pub fn execute(
    trigger: String,
    file_path: PathBuf,
    interpreter: Option<ScriptInterpreter>,
    behavior: ScriptBehavior,
) -> taurine_core::error::Result<()> {
    if !file_path.exists() {
        return Err(taurine_core::error::Error::NotFound(format!(
            "Script file not found: {}",
            file_path.display()
        )));
    }

    let content = fs::read_to_string(&file_path).map_err(|e| {
        taurine_core::error::Error::Service(format!("Failed to read script file: {}", e))
    })?;

    // 1. Infer interpreter if not provided
    let interpreter = match interpreter {
        Some(i) => i,
        None => infer_interpreter(&file_path, &content).ok_or_else(|| {
            taurine_core::error::Error::Service(
                "Could not infer script interpreter. Please specify with --interpreter".to_string(),
            )
        })?,
    };

    info!(
        "Adding script automation: {} ({} via {})",
        trigger,
        behavior_to_str(behavior),
        interpreter_to_str(interpreter)
    );

    let conn = init::setup()?;
    let id = uuid::Uuid::new_v4().to_string();

    // 2. Compress the script
    let compressed = compress(&content)?;

    // 3. Upsert automation row (type = "script")
    upsert_automation(
        &conn,
        &id,
        &trigger,
        Some(&format!("Shell script: {}", file_path.display())),
        &trigger,
        &format!("[Script: {}]", interpreter_to_str(interpreter)),
        "script",
        "all",
        "[]",
        0,
        None,
    )?;

    // 4. Upsert script attachment
    upsert_script(&conn, &id, interpreter, behavior, &compressed)?;

    info!(
        "Successfully added script automation for trigger: {}",
        trigger
    );
    taurine_core::rpc::notify_daemon_reload();

    Ok(())
}

fn infer_interpreter(path: &std::path::Path, content: &str) -> Option<ScriptInterpreter> {
    // Check extension first
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        match ext.to_lowercase().as_str() {
            "sh" => return Some(ScriptInterpreter::Bash),
            "ps1" => return Some(ScriptInterpreter::PowerShell),
            "py" => return Some(ScriptInterpreter::Python),
            "bat" | "cmd" => return Some(ScriptInterpreter::Cmd),
            _ => {}
        }
    }

    // Check shebang
    if content.starts_with("#!") {
        let first_line = content.lines().next().unwrap_or("");
        if first_line.contains("bash") || first_line.contains("sh") {
            return Some(ScriptInterpreter::Bash);
        }
        if first_line.contains("python") {
            return Some(ScriptInterpreter::Python);
        }
    }

    None
}

fn interpreter_to_str(i: ScriptInterpreter) -> &'static str {
    match i {
        ScriptInterpreter::Bash => "bash",
        ScriptInterpreter::PowerShell => "powershell",
        ScriptInterpreter::Python => "python",
        ScriptInterpreter::Cmd => "cmd",
    }
}

fn behavior_to_str(b: ScriptBehavior) -> &'static str {
    match b {
        ScriptBehavior::Inline => "inline",
        ScriptBehavior::Silent => "silent",
    }
}
