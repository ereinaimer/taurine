#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Mutex;

use tracing::info;

use taurine_core::ai::{AiProvider, CredentialStore, OsKeyringStore, configured_providers};
use taurine_core::db::init;
use taurine_core::settings::SettingsManager;

#[derive(Debug, Clone, Default)]
pub struct AiCommandArgs {
    pub yes: bool,
    pub provider: Option<AiProvider>,
    pub key: Option<String>,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub remove: Option<AiProvider>,
    pub remove_all: bool,
    pub json: bool,
}

pub fn execute(args: AiCommandArgs) -> taurine_core::error::Result<()> {
    if !args.yes {
        return taurine_tui::run_ai_overlay();
    }

    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);

    if args.remove_all {
        let removed = remove_all_providers(&OsKeyringStore)?;
        manager.update_setting("ai_provider", "")?;
        manager.update_setting("ai_model", "")?;
        if args.json {
            println!(
                "{}",
                serde_json::json!({"status": "removed", "providers": removed})
            );
        } else {
            info!("removed {} providers", removed.len());
        }
        return Ok(());
    }

    if let Some(p) = args.remove {
        let was_removed = remove_provider_credential(&OsKeyringStore, p)?;
        let settings = manager.load_all();
        if let Some(active) = settings.ai_provider
            && active.eq_ignore_ascii_case(p.as_str())
        {
            let remaining = configured_providers(&OsKeyringStore)?;
            if let Some(next) = remaining.first() {
                manager.update_setting("ai_provider", next.as_str())?;
                manager.update_setting("ai_model", next.default_model())?;
            } else {
                manager.update_setting("ai_provider", "")?;
                manager.update_setting("ai_model", "")?;
            }
        }
        if args.json {
            println!(
                "{}",
                serde_json::json!({
                    "status": if was_removed { "removed" } else { "not_configured" },
                    "provider": p.as_str(),
                })
            );
        } else if was_removed {
            info!("removed {}", p.as_str());
        } else {
            info!("{} not configured", p.as_str());
        }
        return Ok(());
    }

    if let Some(p) = args.provider {
        if p != AiProvider::Custom && args.key.is_none() {
            return Err(taurine_core::Error::Config(format!(
                "API key is required for provider '{}'. Use --key <KEY>.",
                p.as_str()
            )));
        }

        if let Some(ref k) = args.key {
            OsKeyringStore.set_secret(p, k.trim())?;
        }

        manager.update_setting("ai_provider", p.as_str())?;

        let model_str = args
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| p.default_model());
        manager.update_setting("ai_model", model_str)?;

        if p == AiProvider::Custom
            && let Some(ref ep) = args.endpoint
        {
            manager.update_setting("ai_custom_endpoint", ep.trim())?;
        }

        if args.json {
            println!(
                "{}",
                serde_json::json!({
                    "status": "configured",
                    "provider": p.as_str(),
                    "model": model_str,
                })
            );
        } else {
            info!(
                "configured and activated {} (model: {})",
                p.as_str(),
                model_str
            );
        }
        return Ok(());
    }

    // Status / info output when -y is provided without mutation flags
    let settings = manager.load_all();
    let configured = configured_providers(&OsKeyringStore)?;

    if args.json {
        let prov_names: Vec<&str> = configured.iter().map(|p| p.as_str()).collect();
        println!(
            "{}",
            serde_json::json!({
                "active_provider": settings.ai_provider,
                "active_model": settings.ai_model,
                "custom_endpoint": settings.ai_custom_endpoint,
                "configured_providers": prov_names,
            })
        );
    } else {
        if let Some(ref active) = settings.ai_provider {
            let model = settings.ai_model.as_deref().unwrap_or("<default>");
            println!("Active Provider: {active} (model: {model})");
        } else {
            println!("Active Provider: None");
        }
        if configured.is_empty() {
            println!("Configured Providers: none");
        } else {
            println!("Configured Providers:");
            for p in &configured {
                let is_active = settings
                    .ai_provider
                    .as_deref()
                    .map(|a| a.eq_ignore_ascii_case(p.as_str()))
                    .unwrap_or(false);
                let tag = if is_active { " [Active]" } else { "" };
                println!("  - {}{tag}", p.display_name());
            }
        }
    }

    Ok(())
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

fn remove_all_providers<S: CredentialStore>(
    store: &S,
) -> taurine_core::error::Result<Vec<&'static str>> {
    let configured = configured_providers(store)?;
    let mut removed = Vec::new();
    for provider in &configured {
        if remove_provider_credential(store, *provider)? {
            removed.push(provider.as_str());
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroize::Zeroize;

    trait ModelCatalog {
        fn list_models(
            &self,
            provider: AiProvider,
            api_key: &str,
        ) -> taurine_core::error::Result<Vec<String>>;
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

    fn models_for_provider<S, M>(
        store: &S,
        catalog: &M,
        provider: AiProvider,
    ) -> taurine_core::error::Result<Vec<String>>
    where
        S: CredentialStore,
        M: ModelCatalog,
    {
        let secret = store.get_secret(provider)?.ok_or_else(|| {
            taurine_core::error::Error::Config(format!(
                "Provider '{}' is not configured",
                provider.as_str()
            ))
        })?;
        catalog.list_models(provider, secret.as_str())
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

        fn get_secret(
            &self,
            provider: AiProvider,
        ) -> taurine_core::error::Result<Option<zeroize::Zeroizing<String>>> {
            Ok(self
                .secrets
                .lock()
                .expect("memory store poisoned")
                .get(&provider)
                .map(|s| zeroize::Zeroizing::new(s.clone())))
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
            Some(zeroize::Zeroizing::new("sk-test".to_string()))
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
            Some(zeroize::Zeroizing::new("super-secret".to_string()))
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
    fn test_configured_providers_json_format() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Openai, "sk-1").unwrap();
        store.set_secret(AiProvider::Claude, "sk-2").unwrap();
        store.set_secret(AiProvider::Gemini, "sk-3").unwrap();

        let providers = configured_providers(&store).unwrap();
        let names: Vec<&str> = providers.iter().map(|p| p.as_str()).collect();
        let json = serde_json::to_string(&names).unwrap();

        assert_eq!(json, r#"["openai","claude","gemini"]"#);
    }

    #[test]
    fn test_configured_providers_json_empty() {
        let store = MemoryCredentialStore::default();
        let providers = configured_providers(&store).unwrap();
        let names: Vec<&str> = providers.iter().map(|p| p.as_str()).collect();
        let json = serde_json::to_string(&names).unwrap();

        assert_eq!(json, "[]");
    }

    #[test]
    fn test_models_json_format() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Gemini, "key").unwrap();
        let catalog = RecordingCatalog::new(vec!["model-a", "model-b"]);

        let models = models_for_provider(&store, &catalog, AiProvider::Gemini).unwrap();
        let json = serde_json::to_string(&models).unwrap();

        assert_eq!(json, r#"["model-a","model-b"]"#);
    }

    #[test]
    fn test_models_json_empty() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Gemini, "key").unwrap();
        let catalog = RecordingCatalog::new(vec![]);

        let models = models_for_provider(&store, &catalog, AiProvider::Gemini).unwrap();
        let json = serde_json::to_string(&models).unwrap();

        assert_eq!(json, "[]");
    }

    #[test]
    fn test_add_output_json_structure_matches() {
        use serde_json::json;
        let response = json!({"status": "configured", "provider": "openai"});
        assert_eq!(response["status"], "configured");
        assert_eq!(response["provider"], "openai");
    }

    #[test]
    fn test_remove_output_json_structure() {
        use serde_json::json;
        let removed = json!({"status": "removed", "provider": "claude"});
        assert_eq!(removed["status"], "removed");
        assert_eq!(removed["provider"], "claude");

        let not_found = json!({"status": "not_configured", "provider": "claude"});
        assert_eq!(not_found["status"], "not_configured");
    }

    #[test]
    fn test_models_json_sorted_deduped() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Gemini, "key").unwrap();
        // RecordingCatalog returns models unsorted with duplicates
        let mut catalog = RecordingCatalog::new(vec!["z-model", "a-model", "m-model", "a-model"]);
        catalog.models.sort();
        catalog.models.dedup();

        let models = models_for_provider(&store, &catalog, AiProvider::Gemini).unwrap();
        assert_eq!(models, vec!["a-model", "m-model", "z-model"]);
    }

    #[test]
    fn test_models_for_provider_uses_stored_secret() {
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

    #[test]
    fn remove_all_clears_all_configured() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Openai, "sk-openai").unwrap();
        store.set_secret(AiProvider::Claude, "sk-claude").unwrap();
        store.set_secret(AiProvider::Gemini, "sk-gemini").unwrap();

        let removed = remove_all_providers(&store).expect("remove_all should succeed");
        assert_eq!(removed.len(), 3);

        assert_eq!(store.get_secret(AiProvider::Openai).unwrap(), None);
        assert_eq!(store.get_secret(AiProvider::Claude).unwrap(), None);
        assert_eq!(store.get_secret(AiProvider::Gemini).unwrap(), None);
    }

    #[test]
    fn remove_all_returns_empty_on_none() {
        let store = MemoryCredentialStore::default();

        let removed = remove_all_providers(&store).expect("remove_all should succeed");
        assert!(removed.is_empty());
    }

    #[test]
    fn remove_all_json_output_shape() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Openai, "sk-openai").unwrap();
        store.set_secret(AiProvider::Claude, "sk-claude").unwrap();

        let removed = remove_all_providers(&store).expect("remove_all should succeed");
        let response = serde_json::json!({"status": "removed", "providers": removed});
        assert_eq!(response["status"], "removed");
        assert_eq!(
            response["providers"],
            serde_json::json!(["openai", "claude"])
        );
    }
}
