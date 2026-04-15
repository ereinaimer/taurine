use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger: String,
    pub output: String,
    pub action_type: String,
    pub target_os: String,
    pub tags: String, // JSON
    pub usage_count: i64,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
    pub is_deleted: bool,
    pub is_synced: bool,
    pub is_enabled: bool,

    // Script Metadata from joined scripts table
    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
    pub script_binary: Option<Vec<u8>>,
}

/// Minimal data needed by the keystroke listener.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationAction {
    pub output: String,
    pub action_type: String,

    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
    pub script_binary: Option<Vec<u8>>,
}

impl AutomationAction {
    pub fn text(output: &str) -> Self {
        Self {
            output: output.to_string(),
            action_type: "text".to_string(),
            interpreter: None,
            behavior: None,
            script_binary: None,
        }
    }
}

/// Lightweight summary used by the fuzzy finder / command palette.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger: String,
    pub usage_count: i64,
}

/// Data structure for the CLI list view.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationListItem {
    pub trigger: String,
    pub output: String,
    pub action_type: String,
    pub usage_count: i64,
    pub last_used_at: Option<i64>,
    pub created_at: i64,

    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
}
