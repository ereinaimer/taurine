use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use std::time::Duration;

use super::types::{AuthTokens, CloudConfig, CloudProfile};

/// Supabase cloud client managing GoTrue authentication and PostgREST endpoints.
#[derive(Debug, Clone)]
pub struct CloudClient {
    config: CloudConfig,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct GoTrueTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_at: Option<i64>,
    expires_in: Option<i64>,
    user: Option<GoTrueUser>,
    user_id: Option<String>,
}

#[derive(Deserialize)]
struct GoTrueUser {
    id: String,
}

impl CloudClient {
    /// Constructs a new `CloudClient` from configuration.
    pub fn new(config: CloudConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, client }
    }

    /// Constructs a `CloudClient` with a custom HTTP client (useful for custom timeouts/mocks).
    pub fn with_http_client(config: CloudConfig, client: reqwest::Client) -> Self {
        Self { config, client }
    }

    /// Constructs a `CloudClient` by loading configuration from environment variables.
    pub fn from_env() -> crate::Result<Self> {
        let config = CloudConfig::from_env()?;
        Ok(Self::new(config))
    }

    /// Constructs a `CloudClient` using environment variables or built-in defaults.
    pub fn default_or_from_env() -> Self {
        Self::new(CloudConfig::default_or_from_env())
    }

    /// Returns a reference to the active cloud configuration.
    pub fn config(&self) -> &CloudConfig {
        &self.config
    }

    /// Generates the GoTrue OAuth authorize URL with PKCE challenge, redirect URI, and provider.
    pub fn auth_url_for_provider(
        &self,
        provider: &str,
        redirect_uri: &str,
        code_challenge: &str,
    ) -> String {
        let base = format!(
            "{}/auth/v1/authorize",
            self.config.supabase_url.trim_end_matches('/')
        );
        if let Ok(mut url) = reqwest::Url::parse(&base) {
            url.query_pairs_mut()
                .append_pair("provider", provider)
                .append_pair("code_challenge", code_challenge)
                .append_pair("code_challenge_method", "s256")
                .append_pair("redirect_to", redirect_uri)
                .append_pair("response_type", "code");
            url.to_string()
        } else {
            format!(
                "{base}?provider={provider}&code_challenge={code_challenge}&code_challenge_method=s256&redirect_to={redirect_uri}&response_type=code"
            )
        }
    }

    /// Generates the GoTrue OAuth authorize URL with PKCE challenge and redirect URI.
    pub fn auth_url(&self, redirect_uri: &str, code_challenge: &str) -> String {
        self.auth_url_for_provider("github", redirect_uri, code_challenge)
    }

    /// Exchanges an OAuth authorization code and PKCE code verifier for an active session.
    pub async fn exchange_code_for_session(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> crate::Result<AuthTokens> {
        let url = format!(
            "{}/auth/v1/token?grant_type=pkce",
            self.config.supabase_url.trim_end_matches('/')
        );
        let payload = serde_json::json!({
            "auth_code": code,
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri,
        });

        let response = self
            .client
            .post(&url)
            .header("apikey", &self.config.anon_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| crate::Error::Service(format!("Failed to exchange PKCE code: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(crate::Error::Service(format!(
                "GoTrue PKCE exchange error ({status}): {body}"
            )));
        }

        let token_resp: GoTrueTokenResponse = response.json().await.map_err(|e| {
            crate::Error::Service(format!("Failed to parse GoTrue token response: {e}"))
        })?;

        let user_id = token_resp
            .user
            .map(|u| u.id)
            .or(token_resp.user_id)
            .or_else(|| extract_jwt_user_id(&token_resp.access_token))
            .unwrap_or_default();

        let expires_at = token_resp.expires_at.or_else(|| {
            token_resp
                .expires_in
                .map(|secs| chrono::Utc::now().timestamp() + secs)
        });

        Ok(AuthTokens {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            user_id,
            expires_at,
        })
    }

    /// Refreshes an expired session using a refresh token.
    pub async fn refresh_session(&self, refresh_token: &str) -> crate::Result<AuthTokens> {
        let url = format!(
            "{}/auth/v1/token?grant_type=refresh_token",
            self.config.supabase_url.trim_end_matches('/')
        );
        let payload = serde_json::json!({
            "refresh_token": refresh_token,
        });

        let response = self
            .client
            .post(&url)
            .header("apikey", &self.config.anon_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| crate::Error::Service(format!("Failed to refresh session: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(crate::Error::Service(format!(
                "GoTrue session refresh error ({status}): {body}"
            )));
        }

        let token_resp: GoTrueTokenResponse = response.json().await.map_err(|e| {
            crate::Error::Service(format!("Failed to parse GoTrue token response: {e}"))
        })?;

        let user_id = token_resp
            .user
            .map(|u| u.id)
            .or(token_resp.user_id)
            .or_else(|| extract_jwt_user_id(&token_resp.access_token))
            .unwrap_or_default();

        let expires_at = token_resp.expires_at.or_else(|| {
            token_resp
                .expires_in
                .map(|secs| chrono::Utc::now().timestamp() + secs)
        });

        Ok(AuthTokens {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            user_id,
            expires_at,
        })
    }

    /// Signs in with email and password via GoTrue.
    pub async fn sign_in_with_password(
        &self,
        email: &str,
        password: &str,
    ) -> crate::Result<AuthTokens> {
        let url = format!(
            "{}/auth/v1/token?grant_type=password",
            self.config.supabase_url.trim_end_matches('/')
        );
        let payload = serde_json::json!({
            "email": email,
            "password": password,
        });

        let response = self
            .client
            .post(&url)
            .header("apikey", &self.config.anon_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| crate::Error::Service(format!("Failed to sign in: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(crate::Error::Service(format!(
                "GoTrue authentication error ({status}): {body}"
            )));
        }

        let token_resp: GoTrueTokenResponse = response.json().await.map_err(|e| {
            crate::Error::Service(format!("Failed to parse GoTrue token response: {e}"))
        })?;

        let user_id = token_resp
            .user
            .map(|u| u.id)
            .or(token_resp.user_id)
            .or_else(|| extract_jwt_user_id(&token_resp.access_token))
            .unwrap_or_default();

        let expires_at = token_resp.expires_at.or_else(|| {
            token_resp
                .expires_in
                .map(|secs| chrono::Utc::now().timestamp() + secs)
        });

        Ok(AuthTokens {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            user_id,
            expires_at,
        })
    }

    /// Fetches the user profile from PostgREST (`/rest/v1/profiles?id=eq.<user_id>`).
    pub async fn get_profile(
        &self,
        access_token: &str,
        user_id: &str,
    ) -> crate::Result<CloudProfile> {
        let url = format!(
            "{}/rest/v1/profiles?id=eq.{}",
            self.config.supabase_url.trim_end_matches('/'),
            user_id
        );

        let response = self
            .client
            .get(&url)
            .header("apikey", &self.config.anon_key)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| crate::Error::Service(format!("Failed to fetch profile: {e}")))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(crate::Error::NotFound(format!(
                "Profile for user {user_id} not found"
            )));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(crate::Error::Service(format!(
                "PostgREST profile error ({status}): {body}"
            )));
        }

        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| crate::Error::Service(format!("Failed to parse profile response: {e}")))?;

        match value {
            serde_json::Value::Array(items) => {
                if let Some(first) = items.into_iter().next() {
                    serde_json::from_value(first).map_err(|e| {
                        crate::Error::Service(format!("Failed to deserialize profile: {e}"))
                    })
                } else {
                    Err(crate::Error::NotFound(format!(
                        "Profile for user {user_id} not found"
                    )))
                }
            }
            serde_json::Value::Object(_) => serde_json::from_value(value)
                .map_err(|e| crate::Error::Service(format!("Failed to deserialize profile: {e}"))),
            _ => Err(crate::Error::Service(
                "Unexpected profile response format".to_string(),
            )),
        }
    }
}

fn extract_jwt_user_id(access_token: &str) -> Option<String> {
    let parts: Vec<&str> = access_token.split('.').collect();
    if parts.len() >= 2 {
        let payload_str = parts[1];
        let decoded = URL_SAFE_NO_PAD
            .decode(payload_str)
            .or_else(|_| URL_SAFE_NO_PAD.decode(payload_str.trim_end_matches('=')))
            .ok()?;
        let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
        json.get("sub")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
    } else {
        None
    }
}

/// Generates a cryptographically secure PKCE code verifier and challenge pair (S256).
pub fn generate_pkce_challenge() -> (String, String) {
    use sha2::{Digest, Sha256};
    let random_bytes: [u8; 32] = rand::random();
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());
    (verifier, challenge)
}
