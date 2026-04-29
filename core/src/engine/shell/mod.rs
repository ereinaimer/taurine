use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptInterpreter {
    Bash,
    PowerShell,
    Python,
    Node,
    NodeEsm,
    Cmd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptBehavior {
    Inline,
    Silent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptMetadata {
    pub interpreter: ScriptInterpreter,
    pub behavior: ScriptBehavior,
    pub compressed_content: Vec<u8>,
}

pub fn infer_interpreter(
    path: Option<&std::path::Path>,
    content: &str,
) -> Option<ScriptInterpreter> {
    if let Some(ext) = path.and_then(|p| p.extension()).and_then(|s| s.to_str()) {
        match ext.to_lowercase().as_str() {
            "sh" => return Some(ScriptInterpreter::Bash),
            "ps1" => return Some(ScriptInterpreter::PowerShell),
            "py" => return Some(ScriptInterpreter::Python),
            "js" | "cjs" => return Some(ScriptInterpreter::Node),
            "mjs" => return Some(ScriptInterpreter::NodeEsm),
            "bat" | "cmd" => return Some(ScriptInterpreter::Cmd),
            _ => {}
        }
    }

    if content.starts_with("#!") {
        let first_line = content.lines().next().unwrap_or("");
        let shebang = first_line
            .trim_start_matches("#!")
            .trim()
            .replace('\\', "/")
            .to_ascii_lowercase();
        let shebang_words: Vec<&str> = shebang.split_whitespace().collect();

        if shebang.contains("pwsh") || shebang.contains("powershell") {
            return Some(ScriptInterpreter::PowerShell);
        }
        if shebang_words.iter().any(|word| {
            *word == "bash" || *word == "sh" || word.ends_with("/bash") || word.ends_with("/sh")
        }) {
            return Some(ScriptInterpreter::Bash);
        }
        if first_line.contains("python") {
            return Some(ScriptInterpreter::Python);
        }
        if first_line.contains("node") {
            if content.contains("import ")
                || content.contains("export ")
                || content.contains("await ")
            {
                return Some(ScriptInterpreter::NodeEsm);
            }
            return Some(ScriptInterpreter::Node);
        }
    }

    None
}

/// Compresses a script string using zstd for efficient storage.
pub fn compress(content: &str) -> crate::Result<Vec<u8>> {
    zstd::bulk::compress(content.as_bytes(), 3)
        .map_err(|e| crate::Error::Service(format!("zstd compression failed: {}", e)))
}

/// Decompresses binary script content back into a String.
///
/// Implements a 1MB safety limit for decompression to prevent zip-bomb style attacks.
pub fn decompress(compressed: &[u8]) -> crate::Result<String> {
    const MAX_SCRIPT_SIZE: usize = 1024 * 1024; // 1MB

    let decompressed = zstd::bulk::decompress(compressed, MAX_SCRIPT_SIZE)
        .map_err(|e| crate::Error::Service(format!("zstd decompression failed: {}", e)))?;

    String::from_utf8(decompressed).map_err(|e| {
        crate::Error::Service(format!("UTF-8 decoding failed after decompression: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_round_trip() {
        let original = "echo 'Hello, World!'";
        let compressed = compress(original).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(original, decompressed);
    }

    #[test]
    fn test_compression_efficiency() {
        let long_script = "echo 'line'\n".repeat(100);
        let compressed = compress(&long_script).unwrap();
        assert!(compressed.len() < long_script.len());
    }

    #[test]
    fn infer_interpreter_prefers_extension_then_shebang() {
        assert_eq!(
            infer_interpreter(Some(std::path::Path::new("test.ps1")), "#!/bin/bash"),
            Some(ScriptInterpreter::PowerShell)
        );
        assert_eq!(
            infer_interpreter(None, "#!/usr/bin/env pwsh\nWrite-Host 'hello'"),
            Some(ScriptInterpreter::PowerShell)
        );
        assert_eq!(
            infer_interpreter(None, "#!/usr/bin/env python3\nprint(1)"),
            Some(ScriptInterpreter::Python)
        );
        assert_eq!(
            infer_interpreter(None, "#!/usr/bin/env node\nimport fs from 'fs'"),
            Some(ScriptInterpreter::NodeEsm)
        );
        assert_eq!(infer_interpreter(None, "plain text"), None);
    }
}
