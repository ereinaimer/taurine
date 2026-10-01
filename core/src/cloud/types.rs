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

pub const DEFAULT_SUPABASE_URL: &str = "https://gqwefhugncszmhpgvjky.supabase.co";
pub const DEFAULT_SUPABASE_ANON_KEY: &str = "sb_publishable_2iMn33fLtz8BtePGIGzGvQ_aqp2ur5I";

impl CloudConfig {
    /// Loads configuration from environment variables, falling back to built-in project defaults.
    pub fn default_or_from_env() -> Self {
        let supabase_url = std::env::var("TAURINE_SUPABASE_URL")
            .or_else(|_| std::env::var("SUPABASE_URL"))
            .unwrap_or_else(|_| DEFAULT_SUPABASE_URL.to_string());
        let anon_key = std::env::var("TAURINE_SUPABASE_ANON_KEY")
            .or_else(|_| std::env::var("SUPABASE_ANON_KEY"))
            .unwrap_or_else(|_| DEFAULT_SUPABASE_ANON_KEY.to_string());
        Self {
            supabase_url: supabase_url.trim_end_matches('/').to_string(),
            anon_key,
        }
    }

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
