use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use futures::StreamExt;
use genai::adapter::AdapterKind;
use genai::chat::{ChatRequest, ChatStreamEvent};
use genai::resolver::AuthData;
use genai::{Client, ModelIden, ServiceTarget};
use tokio::task;
use tracing::error;
use zeroize::{Zeroize, Zeroizing};

use crate::engine::ai::InlineAiUiState;
use taurine_core::ai::{
    AiProvider, CredentialStore, OsKeyringStore, resolve_model_for_provider,
    resolve_provider_from_settings,
};
use taurine_core::engine::{EngineMode, EngineState};
use taurine_core::settings::SettingsManager;

const STREAM_BATCH_WINDOW_MS: u64 = 50;
const STREAM_ERROR_PREFIX: &str = "Error: ";

pub async fn run_inline_ai_stream(state: Arc<EngineState>, ai_ui_state: Arc<InlineAiUiState>) {
    let result = run_inline_ai_stream_inner(state.clone(), ai_ui_state).await;

    state.clear_ai_prompt_buffer();
    state.set_engine_mode(EngineMode::AiCapture);

    if let Err(err) = result {
        error!("Inline AI stream failed: {}", err);
    }
}

async fn run_inline_ai_stream_inner(
    state: Arc<EngineState>,
    ai_ui_state: Arc<InlineAiUiState>,
) -> taurine_core::error::Result<()> {
    let prompt = state.ai_prompt_buffer();
    let mut spinner_cleared = false;
    let mut output: Option<LiveOutputHandle> = None;
    let mut accumulated_response = String::new();
    let mut pending = String::new();
    let mut resolved = match resolve_inline_ai_request(&OsKeyringStore) {
        Ok(resolved) => resolved,
        Err(err) => {
            let message = inline_error_message(&err);
            ensure_spinner_cleared(&ai_ui_state, &mut spinner_cleared).await;
            inject_error_message(&mut output, false, &message).await?;
            finish_output(output).await;
            return Err(err);
        }
    };
    let client = build_chat_client(resolved.provider, resolved.secret.as_str());
    let chat_request = ChatRequest::from_user(prompt);

    let mut chat_stream = match client
        .exec_chat_stream(resolved.model.as_str(), chat_request, None)
        .await
    {
        Ok(stream) => stream,
        Err(err) => {
            let message =
                inline_error_message(&taurine_core::error::Error::Service(err.to_string()));
            ensure_spinner_cleared(&ai_ui_state, &mut spinner_cleared).await;
            inject_error_message(&mut output, false, &message).await?;
            finish_output(output).await;
            resolved.secret.zeroize();
            return Err(taurine_core::error::Error::Service(message));
        }
    };

    loop {
        let next_event = if pending.is_empty() {
            chat_stream.stream.next().await
        } else {
            match tokio::time::timeout(
                Duration::from_millis(STREAM_BATCH_WINDOW_MS),
                chat_stream.stream.next(),
            )
            .await
            {
                Ok(event) => event,
                Err(_) => {
                    flush_pending_batch(&mut output, &mut pending).await?;
                    continue;
                }
            }
        };

        match next_event {
            Some(Ok(event)) => {
                if let Some(chunk) = visible_chunk_text(event) {
                    if chunk.is_empty() {
                        continue;
                    }

                    if !spinner_cleared {
                        cancel_spinner_and_wait(&ai_ui_state).await;
                        spinner_cleared = true;
                        output = Some(LiveOutputHandle::spawn());
                    }

                    accumulated_response.push_str(&chunk);
                    pending.push_str(&chunk);

                    if should_flush_pending(&pending) {
                        flush_pending_batch(&mut output, &mut pending).await?;
                    }
                }
            }
            Some(Err(err)) => {
                let message =
                    inline_error_message(&taurine_core::error::Error::Service(err.to_string()));
                ensure_spinner_cleared(&ai_ui_state, &mut spinner_cleared).await;
                flush_pending_batch(&mut output, &mut pending).await?;
                inject_error_message(&mut output, spinner_cleared, &message).await?;
                finish_output(output).await;
                resolved.secret.zeroize();
                return Err(taurine_core::error::Error::Service(message));
            }
            None => break,
        }
    }

    ensure_spinner_cleared(&ai_ui_state, &mut spinner_cleared).await;
    flush_pending_batch(&mut output, &mut pending).await?;
    finish_output(output).await;
    resolved.secret.zeroize();
    let _ = accumulated_response;

    Ok(())
}

struct ResolvedInlineAiRequest {
    provider: AiProvider,
    model: String,
    secret: String,
}

fn resolve_inline_ai_request<S>(store: &S) -> taurine_core::error::Result<ResolvedInlineAiRequest>
where
    S: CredentialStore,
{
    let conn = taurine_core::db::init::setup()?;
    let settings = SettingsManager::new(&conn).load_all();
    let provider = resolve_provider_from_settings(store, settings.ai_provider.as_deref())?;
    let model = resolve_model_for_provider(provider, settings.ai_model.as_deref());
    let secret = store.get_secret(provider)?.ok_or_else(|| {
        taurine_core::error::Error::Config(format!(
            "Error: Provider '{}' is selected but has no API key. Run 'taurine ai add --provider {}'.",
            provider.as_str(),
            provider.as_str()
        ))
    })?;

    Ok(ResolvedInlineAiRequest {
        provider,
        model,
        secret,
    })
}

fn build_chat_client(provider: AiProvider, api_key: &str) -> Client {
    let api_key = Zeroizing::new(api_key.to_string());
    Client::builder()
        .with_service_target_resolver_fn(move |service_target: ServiceTarget| {
            let ServiceTarget {
                endpoint, model, ..
            } = service_target;
            Ok(ServiceTarget {
                endpoint,
                auth: AuthData::from_single((*api_key).clone()),
                model: ModelIden::new(adapter_kind(provider), model.model_name),
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

fn visible_chunk_text(event: ChatStreamEvent) -> Option<String> {
    match event {
        ChatStreamEvent::Chunk(chunk) => Some(chunk.content),
        ChatStreamEvent::Start
        | ChatStreamEvent::ReasoningChunk(_)
        | ChatStreamEvent::ThoughtSignatureChunk(_)
        | ChatStreamEvent::ToolCallChunk(_)
        | ChatStreamEvent::End(_) => None,
    }
}

fn should_flush_pending(pending: &str) -> bool {
    let Some(last) = pending.chars().last() else {
        return false;
    };

    last.is_whitespace()
        || matches!(
            last,
            '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']' | '}' | '"' | '\''
        )
        || pending.chars().count() >= 48
}

fn inline_error_message(err: &taurine_core::error::Error) -> String {
    match err {
        taurine_core::error::Error::Config(message)
        | taurine_core::error::Error::Service(message) => format_error_message(message),
        _ => format_error_message(&err.to_string()),
    }
}

fn format_error_message(message: &str) -> String {
    if message.starts_with(STREAM_ERROR_PREFIX) {
        message.to_string()
    } else {
        format!("{STREAM_ERROR_PREFIX}{message}")
    }
}

async fn ensure_spinner_cleared(ai_ui_state: &InlineAiUiState, spinner_cleared: &mut bool) {
    if !*spinner_cleared {
        cancel_spinner_and_wait(ai_ui_state).await;
        *spinner_cleared = true;
    }
}

async fn cancel_spinner_and_wait(ai_ui_state: &InlineAiUiState) {
    if let Some(handle) = ai_ui_state.take_spinner() {
        let _ = handle.cancel.send(());
        let _ = handle.task.await;
    }
}

async fn flush_pending_batch(
    output: &mut Option<LiveOutputHandle>,
    pending: &mut String,
) -> taurine_core::error::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }

    if output.is_none() {
        *output = Some(LiveOutputHandle::spawn());
    }

    let batch = std::mem::take(pending);
    if let Some(handle) = output.as_ref() {
        handle.send_text(batch)?;
    }
    Ok(())
}

async fn inject_error_message(
    output: &mut Option<LiveOutputHandle>,
    output_started: bool,
    message: &str,
) -> taurine_core::error::Result<()> {
    if output.is_none() {
        *output = Some(LiveOutputHandle::spawn());
    }

    let payload = if output_started {
        format!("\n\n{message}")
    } else {
        message.to_string()
    };

    if let Some(handle) = output.as_ref() {
        handle.send_text(payload)?;
    }

    Ok(())
}

async fn finish_output(output: Option<LiveOutputHandle>) {
    if let Some(handle) = output {
        handle.finish().await;
    }
}

enum LiveOutputCommand {
    Text(String),
    Finish,
}

struct LiveOutputHandle {
    tx: mpsc::Sender<LiveOutputCommand>,
    join: Option<thread::JoinHandle<()>>,
}

impl LiveOutputHandle {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<LiveOutputCommand>();
        let join = thread::spawn(move || {
            let mut session = crate::injector::StreamingTextSession::begin();

            while let Ok(command) = rx.recv() {
                match command {
                    LiveOutputCommand::Text(text) => {
                        if !session.push_text(&text) {
                            break;
                        }
                    }
                    LiveOutputCommand::Finish => break,
                }

                if session.abort_requested() {
                    break;
                }
            }

            session.finish();
        });

        Self {
            tx,
            join: Some(join),
        }
    }

    fn send_text(&self, text: String) -> taurine_core::error::Result<()> {
        self.tx.send(LiveOutputCommand::Text(text)).map_err(|_| {
            taurine_core::error::Error::Service("Error: AI output interrupted.".to_string())
        })
    }

    async fn finish(mut self) {
        let _ = self.tx.send(LiveOutputCommand::Finish);
        if let Some(join) = self.join.take() {
            let _ = task::spawn_blocking(move || {
                let _ = join.join();
            })
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use taurine_core::settings::Settings;

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

    #[test]
    fn visible_chunk_filter_only_returns_assistant_text_chunks() {
        assert_eq!(
            visible_chunk_text(ChatStreamEvent::Chunk(genai::chat::StreamChunk {
                content: "hello".to_string()
            })),
            Some("hello".to_string())
        );
        assert_eq!(visible_chunk_text(ChatStreamEvent::Start), None);
    }

    #[test]
    fn pending_batches_flush_on_boundaries_or_large_chunks() {
        assert!(should_flush_pending("hello "));
        assert!(should_flush_pending("done."));
        assert!(!should_flush_pending("hello"));
        assert!(should_flush_pending(&"a".repeat(48)));
    }

    #[test]
    fn error_messages_keep_single_error_prefix() {
        assert_eq!(format_error_message("boom"), "Error: boom");
        assert_eq!(format_error_message("Error: boom"), "Error: boom");
    }

    #[test]
    fn resolved_model_prefers_settings_and_provider_defaults() {
        let store = MemoryCredentialStore::default();
        store.set_secret(AiProvider::Openai, "sk-openai").unwrap();

        let settings = Settings {
            ai_provider: Some("openai".to_string()),
            ai_model: Some("gpt-4.1-mini".to_string()),
            ..Settings::default()
        };

        let provider =
            resolve_provider_from_settings(&store, settings.ai_provider.as_deref()).unwrap();
        let model = resolve_model_for_provider(provider, settings.ai_model.as_deref());

        assert_eq!(provider, AiProvider::Openai);
        assert_eq!(model, "gpt-4.1-mini");
    }
}
