use std::fs;
use std::path::PathBuf;
use taurine_core::db::crud::{upsert_automation, upsert_script};
use taurine_core::db::init;
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter, compress};
use tracing::info;

pub fn execute(
    trigger: String,
    content: Option<String>,
    file_path: Option<PathBuf>,
    lang: Option<ScriptInterpreter>,
    mode: ScriptBehavior,
) -> taurine_core::error::Result<()> {
    // 1. Resolve content and source description
    let (content, source_desc) = if let Some(ref path) = file_path {
        if !path.exists() {
            return Err(taurine_core::error::Error::NotFound(format!(
                "Script file not found: {}",
                path.display()
            )));
        }
        let text = fs::read_to_string(path).map_err(|e| {
            taurine_core::error::Error::Service(format!("Failed to read script file: {}", e))
        })?;
        (text, format!("File: {}", path.display()))
    } else if let Some(text) = content {
        (text, "CLI argument".to_string())
    } else {
        // unreachable due to clap constraints (required_unless_present)
        return Err(taurine_core::error::Error::Service(
            "Neither script file nor content provided".to_string(),
        ));
    };

    // 2. Infer interpreter if not provided
    let lang = match lang {
        Some(i) => i,
        None => infer_interpreter(file_path.as_deref(), &content).ok_or_else(|| {
            taurine_core::error::Error::Service(
                "Could not infer script language. Please specify with --lang".to_string(),
            )
        })?,
    };

    info!(
        "Adding script automation: {} ({} via {})",
        trigger,
        mode_to_str(mode),
        lang_to_str(lang)
    );

    let conn = init::setup()?;
    let id = uuid::Uuid::new_v4().to_string();

    // 3. Compress the script
    let compressed = compress(&content)?;

    // 4. Upsert automation row (type = "script")
    upsert_automation(
        &conn,
        &id,
        &trigger,
        Some(&format!("Shell script ({})", source_desc)),
        &trigger,
        &format!("[Script: {}]", lang_to_str(lang)),
        "script",
        "all",
        "[]",
        0,
        None,
    )?;

    // 5. Upsert script attachment
    upsert_script(&conn, &id, lang, mode, &compressed)?;

    info!(
        "Successfully added script automation for trigger: {}",
        trigger
    );
    taurine_core::rpc::notify_daemon_reload();

    Ok(())
}

fn infer_interpreter(path: Option<&std::path::Path>, content: &str) -> Option<ScriptInterpreter> {
    // Check extension if path is available
    if let Some(ext) = path.and_then(|p| p.extension()).and_then(|s| s.to_str()) {
        match ext.to_lowercase().as_str() {
            "sh" => return Some(ScriptInterpreter::Bash),
            "ps1" => return Some(ScriptInterpreter::PowerShell),
            "py" => return Some(ScriptInterpreter::Python),
            "bat" | "cmd" => return Some(ScriptInterpreter::Cmd),
            _ => {}
        }
    }

    // Check shebang in content
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

fn lang_to_str(i: ScriptInterpreter) -> &'static str {
    match i {
        ScriptInterpreter::Bash => "bash",
        ScriptInterpreter::PowerShell => "powershell",
        ScriptInterpreter::Python => "python",
        ScriptInterpreter::Cmd => "cmd",
    }
}

fn mode_to_str(b: ScriptBehavior) -> &'static str {
    match b {
        ScriptBehavior::Inline => "inline",
        ScriptBehavior::Silent => "silent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_inference_by_extension() {
        assert_eq!(
            infer_interpreter(Some(Path::new("test.sh")), ""),
            Some(ScriptInterpreter::Bash)
        );
        assert_eq!(
            infer_interpreter(Some(Path::new("test.ps1")), ""),
            Some(ScriptInterpreter::PowerShell)
        );
        assert_eq!(
            infer_interpreter(Some(Path::new("test.py")), ""),
            Some(ScriptInterpreter::Python)
        );
        assert_eq!(
            infer_interpreter(Some(Path::new("test.bat")), ""),
            Some(ScriptInterpreter::Cmd)
        );
        assert_eq!(
            infer_interpreter(Some(Path::new("test.cmd")), ""),
            Some(ScriptInterpreter::Cmd)
        );
    }

    #[test]
    fn test_inference_by_shebang() {
        assert_eq!(
            infer_interpreter(None, "#!/bin/bash\necho hello"),
            Some(ScriptInterpreter::Bash)
        );
        assert_eq!(
            infer_interpreter(None, "#!/usr/bin/env python3\nprint(1)"),
            Some(ScriptInterpreter::Python)
        );
        assert_eq!(
            infer_interpreter(None, "#!/bin/sh\nls"),
            Some(ScriptInterpreter::Bash)
        );
    }

    #[test]
    fn test_inference_extension_over_shebang() {
        // Extension should be checked first or at least be high priority
        assert_eq!(
            infer_interpreter(Some(Path::new("test.py")), "#!/bin/bash"),
            Some(ScriptInterpreter::Python)
        );
    }

    #[test]
    fn test_inference_fallback() {
        assert_eq!(infer_interpreter(None, "just some text"), None);
        assert_eq!(
            infer_interpreter(Some(Path::new("test.unknown")), "no shebang"),
            None
        );
    }
}
