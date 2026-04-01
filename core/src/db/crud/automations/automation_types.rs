#[derive(Debug, Clone, PartialEq)]
pub struct AutomationRow {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger: String,
    pub payload: String,
    pub action_type: String,
    pub is_regex: bool,
    pub target_os: String,
    pub tags: String, // JSON
    pub usage_count: i64,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
    pub is_deleted: bool,
    pub is_synced: bool,
}

/// Minimal data needed by the keystroke listener.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationAction {
    pub payload: String,
    pub action_type: String,
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
