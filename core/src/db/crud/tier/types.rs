use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserTier {
    #[default]
    Free,
    Pro,
    Max,
    Team,
}

impl UserTier {
    pub const fn is_free(&self) -> bool {
        matches!(self, Self::Free)
    }

    pub const fn is_unlimited(&self) -> bool {
        !self.is_free()
    }

    /// Maps a cloud profile tier string onto the local tier cache.
    pub fn from_cloud_tier(raw: &str) -> Self {
        match raw.to_lowercase().as_str() {
            "pro" => Self::Pro,
            "max" => Self::Max,
            "team" => Self::Team,
            _ => Self::Free,
        }
    }
}
