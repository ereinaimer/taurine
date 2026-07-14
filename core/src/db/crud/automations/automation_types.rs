use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerType {
    #[default]
    Word,
    Hotkey,
    Regex,
}

impl TriggerType {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Word => "word",
            Self::Hotkey => "hotkey",
            Self::Regex => "regex",
        }
    }

    pub fn parse_db(value: &str) -> crate::Result<Self> {
        match value {
            "word" => Ok(Self::Word),
            "hotkey" => Ok(Self::Hotkey),
            "regex" => Ok(Self::Regex),
            other => Err(crate::Error::Config(format!(
                "Invalid trigger_type '{other}'. Expected 'word', 'hotkey', or 'regex'."
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger_type: TriggerType,
    pub trigger: String,
    pub output: String,
    pub action_type: String,
    pub target_os: String,
    pub only_apps: Option<String>,
    pub except_apps: Option<String>,
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
    pub only_apps: Option<String>,
    pub except_apps: Option<String>,

    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
    pub script_binary: Option<Vec<u8>>,
}

impl AutomationAction {
    pub fn text(output: &str) -> Self {
        Self {
            output: output.to_string(),
            action_type: "text".to_string(),
            only_apps: None,
            except_apps: None,
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
    pub trigger_type: TriggerType,
    pub trigger: String,
    pub usage_count: i64,
}

/// Data structure for the CLI list view.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationListItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger_type: TriggerType,
    pub trigger: String,
    pub output: String,
    pub action_type: String,
    pub target_os: String,
    pub only_apps: Option<String>,
    pub except_apps: Option<String>,
    pub usage_count: i64,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
    pub tags: String, // JSON
    pub script_content: Option<String>,

    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerConflict {
    pub id: String,
    pub trigger_type: TriggerType,
    pub trigger: String,
    pub target_os: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regex_trigger_type_serialization() {
        let t = TriggerType::Regex;
        assert_eq!(t.as_db_str(), "regex");
        assert_eq!(TriggerType::parse_db("regex").unwrap(), TriggerType::Regex);
    }
}
