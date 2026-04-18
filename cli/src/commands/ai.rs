#[allow(unused_imports)]
use std::collections::BTreeMap;
#[allow(unused_imports)]
use std::sync::Mutex;

use genai::adapter::AdapterKind;
use genai::resolver::AuthData;
use genai::{Client, ServiceTarget};
use inquire::{Password, PasswordDisplayMode};
use keyring::Entry;
use tokio::runtime::Builder;
use zeroize::Zeroize;

use crate::AiProvider;

const AI_KEYRING_SERVICE: &str = "taurine.inline_ai";

trait CredentialStore {
    fn set_secret(&self, provider: AiProvider, secret: &str) -> taurine_core::error::Result<()>;
    fn get_secret(&self, provider: AiProvider) -> taurine_core::error::Result<Option<String>>;
    fn delete_secret(&self, provider: AiProvider) -> taurine_core::error::Result<bool>;
}

trait ModelCatalog {
    fn list_models(
        &self,
        provider: AiProvider,
        api_key: &str,
    ) -> taurine_core::error::Result<Vec<String>>;
}

pub fn execute_add(provider: AiProvider) -> taurine_core::error::Result<()> {
    add_provider_with_prompt(&OsKeyringStore, provider, prompt_provider_secret)?;
    println!("configured {}", provider.as_str());
    Ok(())
}

pub fn execute_list() -> taurine_core::error::Result<()> {
    for provider in configured_providers(&OsKeyringStore)? {
        println!("{}", provider.as_str());
    }

    Ok(())
}

pub fn execute_models(provider: AiProvider) -> taurine_core::error::Result<()> {
    for model in models_for_provider(&OsKeyringStore, &GenaiModelCatalog, provider)? {
        println!("{model}");
    }

    Ok(())
}

pub fn execute_remove(provider: AiProvider) -> taurine_core::error::Result<()> {
    if remove_provider_credential(&OsKeyringStore, provider)? {
        println!("removed {}", provider.as_str());
    } else {
        println!("{} not configured", provider.as_str());
    }

    Ok(())
}

fn add_provider_with_prompt<S, F>(
    store: &S,
    provider: AiProvider,
    prompt: F,
) -> taurine_core::error::Result<()>
where
    S: CredentialStore,
    F: FnOnce(AiProvider) -> taurine_core::error::Result<String>,
{
    let mut secret = prompt(provider)?;
    store_provider_secret(store, provider, &mut secret)
}

fn configured_providers<S>(store: &S) -> taurine_core::error::Result<Vec<AiProvider>>
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

fn models_for_provider<S, M>(
    store: &S,
    catalog: &M,
    provider: AiProvider,
) -> taurine_core::error::Result<Vec<String>>
where
    S: CredentialStore,
    M: ModelCatalog,
{
    let mut secret = store.get_secret(provider)?.ok_or_else(|| {
        taurine_core::error::Error::Config(format!(
            "Provider '{}' is not configured",
            provider.as_str()
        ))
    })?;
    let result = catalog.list_models(provider, secret.as_str());
    secret.zeroize();
    result
}

fn remove_provider_credential<S>(
    store: &S,
    provider: AiProvider,
) -> taurine_core::error::Result<bool>
where
    S: CredentialStore,
{
    store.delete_secret(provider)
}

fn store_provider_secret<S>(
    store: &S,
    provider: AiProvider,
    secret: &mut String,
) -> taurine_core::error::Result<()>
where
    S: CredentialStore,
{
    let result = store.set_secret(provider, secret.as_str());
    secret.zeroize();
    result
}

fn supported_providers() -> [AiProvider; 3] {
    [AiProvider::Openai, AiProvider::Claude, AiProvider::Gemini]
}

fn prompt_provider_secret(provider: AiProvider) -> taurine_core::error::Result<String> {
    Password::new(&format!("{} API key:", provider.as_str()))
        .with_display_mode(PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .map_err(|e| {
            taurine_core::error::Error::Service(format!(
                "Failed to read {} API key: {e}",
                provider.as_str()
            ))
        })
}

struct OsKeyringStore;

impl OsKeyringStore {
    fn entry(provider: AiProvider) -> taurine_core::error::Result<Entry> {
        Entry::new(AI_KEYRING_SERVICE, provider.as_str()).map_err(|e| {
            taurine_core::error::Error::Service(format!(
                "Failed to open OS keyring entry for '{}': {e}",
                provider.as_str()
            ))
        })
    }
}

impl CredentialStore for OsKeyringStore {
    fn set_secret(&self, provider: AiProvider, secret: &str) -> taurine_core::error::Result<()> {
        Self::entry(provider)?
            .set_password(secret)
            .map_err(|e| keyring_error(provider, "store", e))
    }

    fn get_secret(&self, provider: AiProvider) -> taurine_core::error::Result<Option<String>> {
        match Self::entry(provider)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_error(provider, "read", e)),
        }
    }

    fn delete_secret(&self, provider: AiProvider) -> taurine_core::error::Result<bool> {
        match Self::entry(provider)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(keyring_error(provider, "delete", e)),
        }
    }
}

struct GenaiModelCatalog;

impl ModelCatalog for GenaiModelCatalog {
    fn list_models(
        &self,
        provider: AiProvider,
        api_key: &str,
    ) -> taurine_core::error::Result<Vec<String>> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                taurine_core::error::Error::Service(format!(
                    "Failed to start async runtime for model lookup: {e}"
                ))
            })?;

        let mut secret = api_key.to_string();
        let result = runtime.block_on(async {
            let client = build_model_client(secret.as_str());
            let mut models = client
                .all_model_names(adapter_kind(provider))
                .await
                .map_err(|e| {
                    taurine_core::error::Error::Service(format!(
                        "Failed to list models for '{}': {e}",
                        provider.as_str()
                    ))
                })?;
            models.sort();
            models.dedup();
            Ok(models)
        });
        secret.zeroize();
        result
    }
}

fn build_model_client(api_key: &str) -> Client {
    let api_key = api_key.to_string();
    Client::builder()
        .with_service_target_resolver_fn(move |service_target: ServiceTarget| {
            Ok(ServiceTarget {
                auth: AuthData::from_single(api_key.clone()),
                ..service_target
            })
        })
        .build()
}

fn adapter_kind(provider: AiProvider) -> AdapterKind {
    match provider {
        AiProvider::Openai => AdapterKind::OpenAI,
        AiProvider::Claude => AdapterKind::Anthropic,
        AiProvider::Gemini => AdapterKind::Gemini,
    }
}

fn keyring_error(provider: AiProvider, action: &str, err: keyring::Error) -> taurine_core::Error {
    taurine_core::error::Error::Service(format!(
        "Failed to {action} '{}' credential in the OS keyring: {err}",
        provider.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemoryCredentialStore {
        secrets: Mutex<BTreeMap<AiProvider, String>>,
    }

    impl CredentialStore for MemoryCredentialStore {
        fn set_secret(
            &self,
            provider: AiProvider,
            secret: &str,
        ) -> taurine_core::error::Result<()> {
            self.secrets
                .lock()
                .expect("memory store poisoned")
                .insert(provider, secret.to_string());
            Ok(())
        }

        fn get_secret(&self, provider: AiProvider) -> taurine_core::error::Result<Option<String>> {
            Ok(self
                .secrets
                .lock()
                .expect("memory store poisoned")
                .get(&provider)
                .cloned())
        }

        fn delete_secret(&self, provider: AiProvider) -> taurine_core::error::Result<bool> {
            Ok(self
                .secrets
                .lock()
                .expect("memory store poisoned")
                .remove(&provider)
                .is_some())
        }
    }

    struct RecordingCatalog {
        models: Vec<String>,
        calls: Mutex<Vec<(AiProvider, String)>>,
    }

    impl RecordingCatalog {
        fn new(models: Vec<&str>) -> Self {
            Self {
                models: models.into_iter().map(str::to_string).collect(),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl ModelCatalog for RecordingCatalog {
        fn list_models(
            &self,
            provider: AiProvider,
            api_key: &str,
        ) -> taurine_core::error::Result<Vec<String>> {
            self.calls
                .lock()
                .expect("recording catalog poisoned")
                .push((provider, api_key.to_string()));
            Ok(self.models.clone())
        }
    }

    #[test]
    fn add_provider_routes_prompted_secret_through_store() {
        let store = MemoryCredentialStore::default();

        add_provider_with_prompt(&store, AiProvider::Openai, |_| Ok("sk-test".to_string()))
            .expect("interactive add should succeed");

        assert_eq!(
            store
                .get_secret(AiProvider::Openai)
                .expect("store read should succeed"),
            Some("sk-test".to_string())
        );
    }

    #[test]
    fn store_provider_secret_zeroizes_buffer_after_write() {
        let store = MemoryCredentialStore::default();
        let mut secret = "super-secret".to_string();

        store_provider_secret(&store, AiProvider::Claude, &mut secret)
            .expect("store should succeed");

        assert!(secret.is_empty(), "secret buffer should be cleared");
        assert_eq!(
            store
                .get_secret(AiProvider::Claude)
                .expect("store read should succeed"),
            Some("super-secret".to_string())
        );
    }

    #[test]
    fn configured_providers_only_returns_present_entries() {
        let store = MemoryCredentialStore::default();
        store
            .set_secret(AiProvider::Openai, "sk-openai")
            .expect("openai secret should store");
        store
            .set_secret(AiProvider::Gemini, "sk-gemini")
            .expect("gemini secret should store");

        let providers = configured_providers(&store).expect("provider listing should succeed");

        assert_eq!(providers, vec![AiProvider::Openai, AiProvider::Gemini]);
    }

    #[test]
    fn models_for_provider_uses_stored_secret() {
        let store = MemoryCredentialStore::default();
        let catalog = RecordingCatalog::new(vec!["model-a", "model-b"]);
        store
            .set_secret(AiProvider::Gemini, "vault-key")
            .expect("gemini secret should store");

        let models = models_for_provider(&store, &catalog, AiProvider::Gemini)
            .expect("model listing should succeed");

        assert_eq!(models, vec!["model-a".to_string(), "model-b".to_string()]);
        assert_eq!(
            catalog.calls.lock().expect("catalog poisoned").as_slice(),
            &[(AiProvider::Gemini, "vault-key".to_string())]
        );
    }

    #[test]
    fn remove_provider_credential_reports_presence() {
        let store = MemoryCredentialStore::default();
        store
            .set_secret(AiProvider::Claude, "vault-key")
            .expect("claude secret should store");

        assert!(
            remove_provider_credential(&store, AiProvider::Claude).expect("delete should succeed")
        );
        assert!(
            !remove_provider_credential(&store, AiProvider::Claude)
                .expect("second delete should report missing")
        );
    }
}
