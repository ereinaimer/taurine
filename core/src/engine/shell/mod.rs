use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptInterpreter {
    Bash,
    PowerShell,
    Python,
    Cmd,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
