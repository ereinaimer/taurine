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
pub struct TriggerRow {
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
    pub auto_case: bool,

    // Script Metadata from joined scripts table
    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
    pub script_binary: Option<Vec<u8>>,
}

/// Minimal data needed by the keystroke listener.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerAction {
    pub output: String,
    pub action_type: String,
    pub only_apps: Option<String>,
    pub except_apps: Option<String>,
    pub auto_case: bool,

    pub interpreter: Option<ScriptInterpreter>,
    pub behavior: Option<ScriptBehavior>,
    pub script_binary: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionType {
    #[default]
    Text,
    Script,
}

impl ActionType {
    pub const ALL: [Self; 2] = [Self::Text, Self::Script];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Script => "script",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "text" => Some(Self::Text),
            "script" => Some(Self::Script),
            _ => None,
        }
    }

    pub const fn is_script(self) -> bool {
        matches!(self, Self::Script)
    }

    pub const fn is_text(self) -> bool {
        matches!(self, Self::Text)
    }
}

impl TriggerAction {
    pub fn text(output: &str) -> Self {
        Self {
            output: output.to_string(),
            action_type: ActionType::Text.as_str().to_string(),
            only_apps: None,
            except_apps: None,
            auto_case: false,
            interpreter: None,
            behavior: None,
            script_binary: None,
        }
    }

    pub fn is_script(&self) -> bool {
        ActionType::parse_str(&self.action_type) == Some(ActionType::Script)
    }

    pub fn is_text(&self) -> bool {
        ActionType::parse_str(&self.action_type) == Some(ActionType::Text)
    }
}

/// Lightweight summary used by the fuzzy finder / command palette.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger_type: TriggerType,
    pub trigger: String,
    pub usage_count: i64,
}

/// Data structure for the CLI list view.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TriggerListItem {
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

pub const MAX_NAME_LENGTH: usize = 200;
pub const MAX_DESCRIPTION_LENGTH: usize = 1000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regex_trigger_type_serialization() {
        let t = TriggerType::Regex;
        assert_eq!(t.as_db_str(), "regex");
        assert_eq!(TriggerType::parse_db("regex").unwrap(), TriggerType::Regex);
    }

    #[test]
    fn test_trigger_list_item_json_serializes_all_fields() {
        let item = TriggerListItem {
            id: "abc-123".to_string(),
            name: "my trigger".to_string(),
            description: Some("does a thing".to_string()),
            trigger_type: TriggerType::Hotkey,
            trigger: "ctrl+shift+g".to_string(),
            output: "git status".to_string(),
            action_type: "text".to_string(),
            target_os: "all".to_string(),
            only_apps: Some("terminal".to_string()),
            except_apps: None,
            usage_count: 42,
            last_used_at: Some(1720000000),
            created_at: 1710000000,
            tags: "[\"dev\",\"git\"]".to_string(),
            script_content: None,
            interpreter: None,
            behavior: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"id\":\"abc-123\""));
        assert!(json.contains("\"trigger\":\"ctrl+shift+g\""));
        assert!(json.contains("\"output\":\"git status\""));
        assert!(json.contains("\"action_type\":\"text\""));
        assert!(json.contains("\"usage_count\":42"));
        assert!(json.contains("\"target_os\":\"all\""));
        assert!(json.contains("\"only_apps\":\"terminal\""));
        assert!(json.contains("\"trigger_type\":\"hotkey\""));
        assert!(json.contains("\"script_content\":null"));
        assert!(json.contains("\"interpreter\":null"));
    }

    #[test]
    fn test_trigger_list_item_json_with_script_fields() {
        let item = TriggerListItem {
            id: "script-1".to_string(),
            name: "".to_string(),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "deploy".to_string(),
            output: "Inline Bash".to_string(),
            action_type: "script".to_string(),
            target_os: "linux".to_string(),
            only_apps: None,
            except_apps: None,
            usage_count: 7,
            last_used_at: None,
            created_at: 1700000000,
            tags: "[]".to_string(),
            script_content: Some("echo deployed".to_string()),
            interpreter: Some(crate::engine::shell::ScriptInterpreter::Bash),
            behavior: Some(crate::engine::shell::ScriptBehavior::Inline),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"action_type\":\"script\""));
        assert!(json.contains("\"script_content\":\"echo deployed\""));
        assert!(json.contains("\"interpreter\":\"bash\""));
        assert!(json.contains("\"behavior\":\"inline\""));
        assert!(json.contains("\"last_used_at\":null"));
    }

    #[test]
    fn test_trigger_list_item_empty_tag_list_serializes() {
        let item = TriggerListItem {
            id: "empty-tags".to_string(),
            name: "".to_string(),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "x".to_string(),
            output: "y".to_string(),
            action_type: "text".to_string(),
            target_os: "all".to_string(),
            only_apps: None,
            except_apps: None,
            usage_count: 0,
            last_used_at: None,
            created_at: 0,
            tags: "[]".to_string(),
            script_content: None,
            interpreter: None,
            behavior: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"tags\":\"[]\""));
        assert!(json.contains("\"usage_count\":0"));
    }

    #[test]
    fn test_trigger_list_item_regex_trigger_type() {
        let item = TriggerListItem {
            id: "r1".to_string(),
            name: "".to_string(),
            description: None,
            trigger_type: TriggerType::Regex,
            trigger: "issue-(\\d+)".to_string(),
            output: "https://bugs.example.com/[0]".to_string(),
            action_type: "text".to_string(),
            target_os: "all".to_string(),
            only_apps: None,
            except_apps: None,
            usage_count: 100,
            last_used_at: Some(1730000000),
            created_at: 1720000000,
            tags: "[\"regex\"]".to_string(),
            script_content: None,
            interpreter: None,
            behavior: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"trigger_type\":\"regex\""));
        assert!(json.contains("\"trigger\":\"issue-(\\\\d+)\""));
        assert!(json.contains("\"last_used_at\":1730000000"));
    }

    #[test]
    fn test_action_type_as_str_and_parse_roundtrip() {
        for action in ActionType::ALL {
            let label = action.as_str();
            assert_eq!(ActionType::parse_str(label), Some(action));
        }
    }

    #[test]
    fn test_action_type_parse_aliases() {
        assert_eq!(ActionType::parse_str("text"), Some(ActionType::Text));
        assert_eq!(
            ActionType::parse_str("  SCRIPT  "),
            Some(ActionType::Script)
        );
        assert_eq!(ActionType::parse_str("invalid"), None);
    }

    #[test]
    fn test_action_type_helper_predicates() {
        assert!(ActionType::Script.is_script());
        assert!(!ActionType::Script.is_text());
        assert!(ActionType::Text.is_text());
        assert!(!ActionType::Text.is_script());
    }
}
