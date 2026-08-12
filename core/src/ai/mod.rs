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
    Xai,
    Groq,
    Deepseek,
    Cohere,
    Together,
    Fireworks,
    Nebius,
    Mimo,
    Zai,
    BigModel,
    GithubCopilot,
    Custom,
}

impl AiProvider {
    pub const ALL: [Self; 15] = [
        Self::Openai,
        Self::Claude,
        Self::Gemini,
        Self::Xai,
        Self::Groq,
        Self::Deepseek,
        Self::Cohere,
        Self::Together,
        Self::Fireworks,
        Self::Nebius,
        Self::Mimo,
        Self::Zai,
        Self::BigModel,
        Self::GithubCopilot,
        Self::Custom,
    ];

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Openai => "OpenAI",
            Self::Claude => "Anthropic Claude",
            Self::Gemini => "Google Gemini",
            Self::Xai => "xAI Grok",
            Self::Groq => "Groq",
            Self::Deepseek => "DeepSeek",
            Self::Cohere => "Cohere",
            Self::Together => "Together AI",
            Self::Fireworks => "Fireworks AI",
            Self::Nebius => "Nebius AI",
            Self::Mimo => "MIMO",
            Self::Zai => "ZAI",
            Self::BigModel => "BigModel",
            Self::GithubCopilot => "GitHub Copilot",
            Self::Custom => "Custom Endpoint",
        }
    }

    pub fn to_genai_adapter(self) -> genai::adapter::AdapterKind {
        match self {
            Self::Openai => genai::adapter::AdapterKind::OpenAI,
            Self::Claude => genai::adapter::AdapterKind::Anthropic,
            Self::Gemini => genai::adapter::AdapterKind::Gemini,
            Self::Xai => genai::adapter::AdapterKind::Xai,
            Self::Groq => genai::adapter::AdapterKind::Groq,
            Self::Deepseek => genai::adapter::AdapterKind::DeepSeek,
            Self::Cohere => genai::adapter::AdapterKind::Cohere,
            Self::Together => genai::adapter::AdapterKind::Together,
            Self::Fireworks => genai::adapter::AdapterKind::Fireworks,
            Self::Nebius => genai::adapter::AdapterKind::Nebius,
            Self::Mimo => genai::adapter::AdapterKind::Mimo,
            Self::Zai => genai::adapter::AdapterKind::Zai,
            Self::BigModel => genai::adapter::AdapterKind::BigModel,
            Self::GithubCopilot => genai::adapter::AdapterKind::OpenAI,
            Self::Custom => genai::adapter::AdapterKind::OpenAI,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Xai => "xai",
            Self::Groq => "groq",
            Self::Deepseek => "deepseek",
            Self::Cohere => "cohere",
            Self::Together => "together",
            Self::Fireworks => "fireworks",
            Self::Nebius => "nebius",
            Self::Mimo => "mimo",
            Self::Zai => "zai",
            Self::BigModel => "bigmodel",
            Self::GithubCopilot => "github_copilot",
            Self::Custom => "custom",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Openai => "gpt-4o",
            Self::Claude => "claude-3-5-sonnet-latest",
            Self::Gemini => "gemini-2.5-flash",
            Self::Xai => "grok-2-latest",
            Self::Groq => "llama-3.3-70b-versatile",
            Self::Deepseek => "deepseek-chat",
            Self::Cohere => "command-r-plus",
            Self::Together => "meta-llama/Llama-3.3-70B-Instruct-Turbo",
            Self::Fireworks => "accounts/fireworks/models/llama-v3p3-70b-instruct",
            Self::Nebius => "meta-llama/Meta-Llama-3.1-70B-Instruct",
            Self::Mimo => "mimo-2",
            Self::Zai => "glm-4",
            Self::BigModel => "glm-4",
            Self::GithubCopilot => "gpt-4o",
            Self::Custom => "gpt-3.5-turbo",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai" => Some(Self::Openai),
            "claude" => Some(Self::Claude),
            "gemini" => Some(Self::Gemini),
            "xai" => Some(Self::Xai),
            "groq" => Some(Self::Groq),
            "deepseek" => Some(Self::Deepseek),
            "cohere" => Some(Self::Cohere),
            "together" => Some(Self::Together),
            "fireworks" => Some(Self::Fireworks),
            "nebius" => Some(Self::Nebius),
            "mimo" => Some(Self::Mimo),
            "zai" => Some(Self::Zai),
            "bigmodel" => Some(Self::BigModel),
            "github_copilot" => Some(Self::GithubCopilot),
            "custom" => Some(Self::Custom),
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
                "Invalid ai_provider setting '{s}'. Use openai, claude, gemini, xai, groq, deepseek, cohere, together, fireworks, nebius, mimo, zai, bigmodel, github_copilot, or custom."
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
    fn get_secret(&self, provider: AiProvider) -> Result<Option<zeroize::Zeroizing<String>>>;
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

    fn get_secret(&self, provider: AiProvider) -> Result<Option<zeroize::Zeroizing<String>>> {
        match Self::entry(provider)?.get_password() {
            Ok(secret) => Ok(Some(zeroize::Zeroizing::new(secret))),
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

pub fn supported_providers() -> [AiProvider; 15] {
    [
        AiProvider::Openai,
        AiProvider::Claude,
        AiProvider::Gemini,
        AiProvider::Xai,
        AiProvider::Groq,
        AiProvider::Deepseek,
        AiProvider::Cohere,
        AiProvider::Together,
        AiProvider::Fireworks,
        AiProvider::Nebius,
        AiProvider::Mimo,
        AiProvider::Zai,
        AiProvider::BigModel,
        AiProvider::GithubCopilot,
        AiProvider::Custom,
    ]
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
        [] => Err(Error::Config("AI not configured. Run setup.".to_string())),
        [provider] => Ok(*provider),
        _ => Err(Error::Config(
            "Multiple AI providers found. Set a preferred one in settings.".to_string(),
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

        fn get_secret(&self, provider: AiProvider) -> Result<Option<zeroize::Zeroizing<String>>> {
            Ok(self
                .secrets
                .lock()
                .expect("memory store poisoned")
                .get(&provider)
                .map(|s| zeroize::Zeroizing::new(s.clone())))
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
            "AI not configured. Run setup."
        );

        let multi = MemoryCredentialStore::default();
        multi.set_secret(AiProvider::Openai, "openai").unwrap();
        multi.set_secret(AiProvider::Gemini, "gemini").unwrap();
        assert_eq!(
            resolve_provider_from_settings(&multi, None)
                .expect_err("multiple providers should fail")
                .to_string(),
            "Multiple AI providers found. Set a preferred one in settings."
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
            "gemini-2.5-flash"
        );
    }

    #[test]
    fn test_ai_provider_all_slice_and_display_name_roundtrip() {
        assert_eq!(AiProvider::ALL.len(), 15);
        for provider in AiProvider::ALL {
            let label = provider.as_str();
            let display = provider.display_name();
            assert!(!display.is_empty());
            assert_eq!(AiProvider::parse(label), Some(provider));
        }
    }
}
