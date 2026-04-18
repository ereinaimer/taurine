use std::fmt;
use std::str::FromStr;

use keyring::Entry;

use crate::error::{Error, Result};

pub const AI_KEYRING_SERVICE: &str = "taurine.inline_ai";

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    Openai,
    Claude,
    Gemini,
}

impl AiProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Openai => "gpt-4o",
            Self::Claude => "claude-3-5-sonnet-latest",
            Self::Gemini => "gemini-1.5-pro",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai" => Some(Self::Openai),
            "claude" => Some(Self::Claude),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }
}

impl fmt::Display for AiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AiProvider {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s).ok_or_else(|| {
            Error::Config(format!(
                "Invalid ai_provider setting '{s}'. Use openai, claude, or gemini."
            ))
        })
    }
}

impl TryFrom<&str> for AiProvider {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self> {
        Self::from_str(value)
    }
}

pub trait CredentialStore {
    fn set_secret(&self, provider: AiProvider, secret: &str) -> Result<()>;
    fn get_secret(&self, provider: AiProvider) -> Result<Option<String>>;
    fn delete_secret(&self, provider: AiProvider) -> Result<bool>;
}

pub struct OsKeyringStore;

impl OsKeyringStore {
    fn entry(provider: AiProvider) -> Result<Entry> {
        Entry::new(AI_KEYRING_SERVICE, provider.as_str()).map_err(|e| {
            Error::Service(format!(
                "Failed to open OS keyring entry for '{}': {e}",
                provider.as_str()
            ))
        })
    }
}

impl CredentialStore for OsKeyringStore {
    fn set_secret(&self, provider: AiProvider, secret: &str) -> Result<()> {
        Self::entry(provider)?
            .set_password(secret)
            .map_err(|e| keyring_error(provider, "store", e))
    }

    fn get_secret(&self, provider: AiProvider) -> Result<Option<String>> {
        match Self::entry(provider)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_error(provider, "read", e)),
        }
    }

    fn delete_secret(&self, provider: AiProvider) -> Result<bool> {
        match Self::entry(provider)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(keyring_error(provider, "delete", e)),
        }
    }
}

pub fn supported_providers() -> [AiProvider; 3] {
    [AiProvider::Openai, AiProvider::Claude, AiProvider::Gemini]
}

pub fn configured_providers<S>(store: &S) -> Result<Vec<AiProvider>>
where
    S: CredentialStore,
{
    let mut configured = Vec::new();
    for provider in supported_providers() {
        if store.get_secret(provider)?.is_some() {
            configured.push(provider);
        }
    }
    Ok(configured)
}

pub fn resolve_provider_from_settings<S>(
    store: &S,
    configured_provider: Option<&str>,
) -> Result<AiProvider>
where
    S: CredentialStore,
{
    if let Some(provider) = configured_provider {
        return provider.parse();
    }

    let configured = configured_providers(store)?;
    match configured.as_slice() {
        [] => Err(Error::Config(
            "Error: No API keys configured. Run 'taurine ai add'.".to_string(),
        )),
        [provider] => Ok(*provider),
        _ => Err(Error::Config(
            "Error: Multiple providers found. Run 'taurine config set ai_provider <name>' to select one.".to_string(),
        )),
    }
}

pub fn resolve_model_for_provider(provider: AiProvider, configured_model: Option<&str>) -> String {
    configured_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| provider.default_model().to_string())
}

fn keyring_error(provider: AiProvider, action: &str, err: keyring::Error) -> Error {
    Error::Service(format!(
        "Failed to {action} '{}' credential in the OS keyring: {err}",
        provider.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryCredentialStore {
        secrets: Mutex<BTreeMap<AiProvider, String>>,
    }

    impl CredentialStore for MemoryCredentialStore {
        fn set_secret(&self, provider: AiProvider, secret: &str) -> Result<()> {
            self.secrets
                .lock()
                .expect("memory store poisoned")
                .insert(provider, secret.to_string());
            Ok(())
        }

        fn get_secret(&self, provider: AiProvider) -> Result<Option<String>> {
            Ok(self
                .secrets
                .lock()
                .expect("memory store poisoned")
                .get(&provider)
                .cloned())
        }

        fn delete_secret(&self, provider: AiProvider) -> Result<bool> {
            Ok(self
                .secrets
                .lock()
                .expect("memory store poisoned")
                .remove(&provider)
                .is_some())
        }
    }

    #[test]
    fn configured_providers_returns_only_present_entries() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Gemini, "gemini").unwrap();
        store.set_secret(AiProvider::Openai, "openai").unwrap();

        assert_eq!(
            configured_providers(&store).unwrap(),
            vec![AiProvider::Openai, AiProvider::Gemini]
        );
    }

    #[test]
    fn configured_setting_provider_wins() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Openai, "openai").unwrap();
        store.set_secret(AiProvider::Gemini, "gemini").unwrap();

        assert_eq!(
            resolve_provider_from_settings(&store, Some("claude")).unwrap(),
            AiProvider::Claude
        );
    }

    #[test]
    fn provider_resolution_auto_selects_single_keyring_entry() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Claude, "claude").unwrap();

        assert_eq!(
            resolve_provider_from_settings(&store, None).unwrap(),
            AiProvider::Claude
        );
    }

    #[test]
    fn provider_resolution_errors_for_zero_or_multiple_keyring_entries() {
        let empty = MemoryCredentialStore::default();
        assert_eq!(
            resolve_provider_from_settings(&empty, None)
                .expect_err("empty keyring should fail")
                .to_string(),
            "Configuration error: Error: No API keys configured. Run 'taurine ai add'."
        );

        let multi = MemoryCredentialStore::default();
        multi.set_secret(AiProvider::Openai, "openai").unwrap();
        multi.set_secret(AiProvider::Gemini, "gemini").unwrap();
        assert_eq!(
            resolve_provider_from_settings(&multi, None)
                .expect_err("multiple providers should fail")
                .to_string(),
            "Configuration error: Error: Multiple providers found. Run 'taurine config set ai_provider <name>' to select one."
        );
    }

    #[test]
    fn model_resolution_prefers_setting_then_provider_default() {
        assert_eq!(
            resolve_model_for_provider(AiProvider::Openai, Some("gpt-4.1-mini")),
            "gpt-4.1-mini"
        );
        assert_eq!(
            resolve_model_for_provider(AiProvider::Claude, None),
            "claude-3-5-sonnet-latest"
        );
        assert_eq!(
            resolve_model_for_provider(AiProvider::Gemini, Some("   ")),
            "gemini-1.5-pro"
        );
    }
}
