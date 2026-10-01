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
}
