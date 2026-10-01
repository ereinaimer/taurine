use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudProfile {
    pub id: String,
    pub tier: String,
    pub stripe_customer_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudConfig {
    pub supabase_url: String,
    pub anon_key: String,
}

impl CloudConfig {
    pub fn from_env() -> crate::Result<Self> {
        let supabase_url = std::env::var("TAURINE_SUPABASE_URL")
            .or_else(|_| std::env::var("SUPABASE_URL"))
            .map_err(|_| {
                crate::Error::Config(
                    "Missing TAURINE_SUPABASE_URL or SUPABASE_URL environment variable".to_string(),
                )
            })?;
        let anon_key = std::env::var("TAURINE_SUPABASE_ANON_KEY")
            .or_else(|_| std::env::var("SUPABASE_ANON_KEY"))
            .map_err(|_| {
                crate::Error::Config(
                    "Missing TAURINE_SUPABASE_ANON_KEY or SUPABASE_ANON_KEY environment variable"
                        .to_string(),
                )
            })?;
        Ok(Self {
            supabase_url: supabase_url.trim_end_matches('/').to_string(),
            anon_key,
        })
    }
}
