use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRow {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
    pub is_deleted: bool,
    pub is_synced: bool,
}
