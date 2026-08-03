use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use futures::StreamExt;

use genai::chat::{ChatOptions, ChatRequest, ChatStreamEvent, Tool};
use genai::resolver::AuthData;
use genai::{Client, ModelIden, ServiceTarget};
use serde_json::json;
use tokio::task;
use tracing::error;
use zeroize::{Zeroize, Zeroizing};

use crate::engine::ai::InlineAiSpinnerHandle;
use taurine_core::ai::{
    AiProvider, CredentialStore, OsKeyringStore, resolve_model_for_provider,
    resolve_provider_from_settings,
};
use taurine_core::settings::SettingsManager;

const STREAM_BATCH_WINDOW_MS: u64 = 50;
const STREAM_ERROR_PREFIX: &str = "Error: ";

pub async fn run_inline_ai_stream(
    prompt: String,
    system_prompt_override: Option<String>,
    spinner_handle: InlineAiSpinnerHandle,
) {
    if let Err(err) =
        run_inline_ai_stream_inner(prompt, system_prompt_override, spinner_handle).await
    {
        error!("Inline AI stream failed: {}", err);
    }
}

/// Resolves all `| ai(...)` transformer markers embedded in `template_with_markers`
/// and injects the final fully-resolved text atomically.
///
/// Markers have the form: `\x03<input>\x1F<prompt>\x04`
/// Independent markers (from separate template tags) resolve sequentially left-to-right,
/// which also correctly handles chained sequential pipelines within a single tag.
pub async fn run_ai_transformer_stream(
    template_with_markers: String,
    spinner_handle: InlineAiSpinnerHandle,
) {
    if let Err(err) = run_ai_transformer_stream_inner(template_with_markers, spinner_handle).await {
        error!("AI transformer stream failed: {}", err);
    }
}

enum Chunk {
    Text(String),
    Marker(String),
}

fn split_outermost_markers(template: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    let mut marker_start = 0;

    let bytes = template.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\x03' {
            if depth == 0 {
                if i > start {
                    chunks.push(Chunk::Text(template[start..i].to_string()));
                }
                marker_start = i;
            }
            depth += 1;
        } else if b == b'\x04' && depth > 0 {
            depth -= 1;
            if depth == 0 {
                chunks.push(Chunk::Marker(template[marker_start..=i].to_string()));
                start = i + 1;
            }
        }
    }
    if start < template.len() {
        chunks.push(Chunk::Text(template[start..].to_string()));
    }
    chunks
}

async fn evaluate_marker_tree(
    mut marker_tree: String,
    client: Option<Client>,
    provider: Option<AiProvider>,
    model: Option<String>,
) -> taurine_core::error::Result<String> {
    while let Some(eot) = marker_tree.find('\x04') {
        if let Some(sot) = marker_tree[..eot].rfind('\x03') {
            let content = &marker_tree[sot + 1..eot];
            let result = if let Some(sep) = content.find('\x1F') {
                let input = &content[..sep];
                let prompt = &content[sep + 1..];

                if let Some(sys_key) = prompt.strip_prefix("sys:") {
                    let sys_key_owned = sys_key.to_string();
                    tokio::task::spawn_blocking(move || {
                        let pipeline =
                            taurine_core::engine::variables::system::transformers::split_pipeline(
                                &sys_key_owned,
                            );
                        let base_key = pipeline[0];

                        let mut val = if base_key == "mouse.pos" {
                            crate::platform::get_mouse_pos()
                                .map(|(x, y)| format!("{},{}", x, y))
                                .unwrap_or_else(|| "0,0".to_string())
                        } else {
                            taurine_core::engine::variables::system::resolve(base_key)
                                .unwrap_or_else(|| format!("[Error: failed to resolve {base_key}]"))
                        };

                        for tr in &pipeline[1..] {
                            if let Some(transformed) =
                                taurine_core::engine::variables::system::transformers::apply(
                                    tr, &val,
                                )
                            {
                                val = transformed;
                            }
                        }
                        val
                    })
                    .await
                    .ok()
                    .unwrap_or_else(|| "[Error: task panicked]".to_string())
                } else if input.is_empty() {
                    return Err(taurine_core::error::Error::Service(
                        "[Error: AI transformer received empty input]".to_string(),
                    ));
                } else if let (Some(client), Some(provider), Some(model)) =
                    (client.as_ref(), provider, model.as_ref())
                {
                    let user_message = if prompt.is_empty() {
                        input.to_string()
                    } else {
                        format!("{prompt}:\n\n{input}")
                    };

                    let chat_request = build_chat_request(provider, &user_message, None, None);
                    let exec_future = client.exec_chat_stream(model, chat_request, None);
                    match tokio::time::timeout(Duration::from_secs(30), exec_future).await {
                        Ok(Ok(mut chat_stream)) => {
                            let mut res = String::new();
                            while let Some(event) = chat_stream.stream.next().await {
                                if let Ok(event) = event
                                    && let Some(chunk) = visible_chunk_text(event)
                                {
                                    res.push_str(&chunk);
                                }
                            }
                            taurine_core::engine::variables::system::transformers::ai::strip_markdown_fence(&res)
                        }
                        Ok(Err(err)) => {
                            let details = sanitize_error_message(&err.to_string());
                            return Err(taurine_core::error::Error::Service(format!(
                                "[Error: AI request failed: {}]",
                                details
                            )));
                        }
                        Err(_) => {
                            return Err(taurine_core::error::Error::Service(
                                "[Error: AI timed out]".to_string(),
                            ));
                        }
                    }
                } else {
                    return Err(taurine_core::error::Error::Config(
                        "AI not configured. Please run setup.".to_string(),
                    ));
                }
            } else {
                content.to_string()
            };

            marker_tree.replace_range(sot..=eot, &result);
        } else {
            marker_tree.replace_range(eot..eot + 1, "");
        }
    }
    Ok(marker_tree)
}

async fn run_ai_transformer_stream_inner(
    template_with_markers: String,
    spinner_handle: InlineAiSpinnerHandle,
) -> taurine_core::error::Result<()> {
    let mut spinner = Some(spinner_handle);
    let mut spinner_cleared = false;

    let mut resolved =
        if taurine_core::engine::variables::contains_non_sys_markers(&template_with_markers) {
            match resolve_inline_ai_request(&OsKeyringStore) {
                Ok(r) => Some(r),
                Err(err) => {
                    ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;
                    let mut output: Option<LiveOutputHandle> = None;
                    inject_error_message(
                        &mut output,
                        false,
                        "[Error: AI not configured. Run setup first.]",
                    )
                    .await?;
                    finish_output(output).await;
                    return Err(err);
                }
            }
        } else {
            None
        };

    let client = resolved
        .as_ref()
        .map(|r| build_chat_client(r.provider, r.secret.as_str(), r.custom_endpoint.clone()));

    let chunks = split_outermost_markers(&template_with_markers);
    let mut handles = Vec::new();

    for chunk in chunks {
        match chunk {
            Chunk::Text(t) => {
                handles.push(tokio::spawn(async move { Ok(t) }));
            }
            Chunk::Marker(m) => {
                let client = client.clone();
                let model = resolved.as_ref().map(|r| r.model.clone());
                let provider = resolved.as_ref().map(|r| r.provider);
                handles.push(tokio::spawn(async move {
                    evaluate_marker_tree(m, client, provider, model).await
                }));
            }
        }
    }

    let results = futures::future::join_all(handles).await;
    let mut output_text = String::new();

    for result in results {
        match result {
            Ok(Ok(text)) => {
                output_text.push_str(&text);
            }
            Ok(Err(err)) => {
                // Task returned an error (e.g. timeout, empty input, API failure)
                let message = err.to_string();
                ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;
                let mut output: Option<LiveOutputHandle> = None;
                inject_error_message(&mut output, false, &message).await?;
                finish_output(output).await;
                if let Some(ref mut r) = resolved {
                    r.secret.zeroize();
                }
                return Err(err);
            }
            Err(err) => {
                // JoinError (panic)
                ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;
                if let Some(ref mut r) = resolved {
                    r.secret.zeroize();
                }
                return Err(taurine_core::error::Error::Service(format!(
                    "Task failed: {}",
                    err
                )));
            }
        }
    }

    if let Some(ref mut r) = resolved {
        r.secret.zeroize();
    }
    ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;

    if output_text.is_empty() {
        return Ok(());
    }

    let expansion = taurine_core::engine::variables::system::finalize(&output_text, None);
    let output_chars: usize = expansion
        .steps
        .iter()
        .map(|s| match s {
            taurine_core::engine::variables::ExpansionStep::Text(t) => t.chars().count(),
            _ => 0,
        })
        .sum();

    let steps = expansion.steps;
    let _ = tokio::task::spawn_blocking(move || {
        crate::injector::inject_expansion(steps, 0, taurine_core::settings::SpinnerStyle::default())
    })
    .await;

    record_inline_ai_completion(output_chars);

    Ok(())
}

async fn run_inline_ai_stream_inner(
    prompt: String,
    system_prompt_override: Option<String>,
    spinner_handle: InlineAiSpinnerHandle,
) -> taurine_core::error::Result<()> {
    let prompt = Zeroizing::new(prompt);
    let mut spinner = Some(spinner_handle);
    let mut spinner_cleared = false;
    let mut output: Option<LiveOutputHandle> = None;
    let mut pending = String::new();
    let mut resolved = match resolve_inline_ai_request(&OsKeyringStore) {
        Ok(resolved) => resolved,
        Err(err) => {
            let message = inline_error_message(&err);
            ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;
            inject_error_message(&mut output, false, &message).await?;
            record_inline_ai_completion(finish_output(output).await);
            return Err(err);
        }
    };
    let client = build_chat_client(
        resolved.provider,
        resolved.secret.as_str(),
        resolved.custom_endpoint,
    );
    let chat_request = build_chat_request(
        resolved.provider,
        prompt.as_str(),
        system_prompt_override,
        resolved.system_prompt,
    );

    let mut chat_options = ChatOptions::default();
    if let Some(temperature) = resolved.temperature {
        chat_options = chat_options.with_temperature(temperature as f64);
    }
    if let Some(max_tokens) = resolved.max_tokens {
        chat_options = chat_options.with_max_tokens(max_tokens);
    }

    let chat_stream_future =
        client.exec_chat_stream(resolved.model.as_str(), chat_request, Some(&chat_options));
    let captured_gen = crate::injector::capture_generation();
    tokio::pin!(chat_stream_future);

    let mut chat_stream = loop {
        if crate::injector::is_aborted(captured_gen) {
            ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;
            resolved.secret.zeroize();
            return Ok(());
        }

        tokio::select! {
            res = &mut chat_stream_future => {
                match res {
                    Ok(stream) => break stream,
                    Err(err) => {
                        let message = inline_error_message(&taurine_core::error::Error::Service(err.to_string()));
                        ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;
                        inject_error_message(&mut output, false, &message).await?;
                        record_inline_ai_completion(finish_output(output).await);
                        resolved.secret.zeroize();
                        return Err(taurine_core::error::Error::Service(message));
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => continue,
        }
    };

    loop {
        let next_event = match tokio::time::timeout(
            Duration::from_millis(STREAM_BATCH_WINDOW_MS),
            chat_stream.stream.next(),
        )
        .await
        {
            Ok(event) => event,
            Err(_) => {
                if crate::injector::is_aborted(captured_gen) {
                    ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;
                    flush_pending_batch(&mut output, &mut pending).await?;
                    record_inline_ai_completion(finish_output(output).await);
                    resolved.secret.zeroize();
                    return Ok(());
                }
                flush_pending_batch(&mut output, &mut pending).await?;
                continue;
            }
        };

        match next_event {
            Some(Ok(event)) => {
                if let Some(chunk) = visible_chunk_text(event) {
                    if chunk.is_empty() {
                        continue;
                    }

                    if !spinner_cleared {
                        cancel_spinner_and_wait(&mut spinner).await;
                        spinner_cleared = true;
                        output = Some(LiveOutputHandle::spawn());
                    }

                    pending.push_str(&chunk);

                    if should_flush_pending(&pending) {
                        flush_pending_batch(&mut output, &mut pending).await?;
                    }
                }
            }
            Some(Err(err)) => {
                let message =
                    inline_error_message(&taurine_core::error::Error::Service(err.to_string()));
                ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;
                flush_pending_batch(&mut output, &mut pending).await?;
                inject_error_message(&mut output, spinner_cleared, &message).await?;
                record_inline_ai_completion(finish_output(output).await);
                resolved.secret.zeroize();
                return Err(taurine_core::error::Error::Service(message));
            }
            None => break,
        }
    }

    ensure_spinner_cleared(&mut spinner, &mut spinner_cleared).await;
    flush_pending_batch(&mut output, &mut pending).await?;
    record_inline_ai_completion(finish_output(output).await);
    resolved.secret.zeroize();

    Ok(())
}

struct ResolvedInlineAiRequest {
    provider: AiProvider,
    model: String,
    secret: zeroize::Zeroizing<String>,
    custom_endpoint: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    system_prompt: Option<String>,
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
        taurine_core::error::Error::Config("AI not configured. Run setup.".to_string())
    })?;

    if provider == AiProvider::Custom && settings.ai_custom_endpoint.is_none() {
        return Err(taurine_core::error::Error::Config(
            "AI not configured. Run setup.".to_string(),
        ));
    }

    Ok(ResolvedInlineAiRequest {
        provider,
        model,
        secret,
        custom_endpoint: settings.ai_custom_endpoint,
        temperature: settings.ai_temperature,
        max_tokens: settings.ai_max_tokens,
        system_prompt: settings.ai_system_prompt,
    })
}

fn build_chat_client(
    provider: AiProvider,
    api_key: &str,
    custom_endpoint: Option<String>,
) -> Client {
    let api_key = Zeroizing::new(api_key.to_string());
    Client::builder()
        .with_service_target_resolver_fn(move |service_target: ServiceTarget| {
            let ServiceTarget {
                endpoint, model, ..
            } = service_target;

            let mut endpoint_url = endpoint;
            if let Some(custom_url) = custom_endpoint.clone() {
                endpoint_url = genai::resolver::Endpoint::from_owned(custom_url);
            }

            Ok(ServiceTarget {
                endpoint: endpoint_url,
                auth: AuthData::from_single((*api_key).clone()),
                model: ModelIden::new(provider.to_genai_adapter(), model.model_name),
            })
        })
        .build()
}

fn build_chat_request(
    provider: AiProvider,
    prompt: &str,
    snippet_prompt_override: Option<String>,
    user_system_prompt: Option<String>,
) -> ChatRequest {
    let base_prompt = user_system_prompt
        .unwrap_or_else(|| taurine_core::settings::DEFAULT_AI_SYSTEM_PROMPT.to_string());

    let system_prompt = if let Some(prompt_override) = snippet_prompt_override {
        format!("{}\n\n{}", prompt_override, base_prompt)
    } else {
        base_prompt
    };

    let request = ChatRequest::from_user(prompt).with_system(system_prompt);
    if provider == AiProvider::Gemini {
        request.append_tool(Tool::new("googleSearch").with_config(json!({})))
    } else {
        request
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

fn sanitize_error_message(raw: &str) -> String {
    if raw.contains("Body: {") || raw.contains("Error event in stream") || raw.contains("http") {
        let lower = raw.to_lowercase();
        if lower.contains("401")
            || lower.contains("unauthorized")
            || lower.contains("invalid api key")
        {
            return "Invalid API key.".to_string();
        }
        if lower.contains("429") || lower.contains("quota") || lower.contains("rate limit") {
            return "API rate limit exceeded.".to_string();
        }
        if lower.contains("503")
            || lower.contains("502")
            || lower.contains("overloaded")
            || lower.contains("high demand")
        {
            return "AI provider is currently overloaded.".to_string();
        }
        if lower.contains("timeout") {
            return "Request timed out.".to_string();
        }
        return "An upstream API error occurred.".to_string();
    }
    raw.to_string()
}

fn inline_error_message(err: &taurine_core::error::Error) -> String {
    let raw_msg = match err {
        taurine_core::error::Error::Config(message)
        | taurine_core::error::Error::Service(message) => message.clone(),
        _ => err.to_string(),
    };
    format_error_message(&sanitize_error_message(&raw_msg))
}

fn format_error_message(message: &str) -> String {
    if message.starts_with(STREAM_ERROR_PREFIX) {
        message.to_string()
    } else {
        format!("{STREAM_ERROR_PREFIX}{message}")
    }
}

async fn ensure_spinner_cleared(
    spinner: &mut Option<InlineAiSpinnerHandle>,
    spinner_cleared: &mut bool,
) {
    if !*spinner_cleared {
        cancel_spinner_and_wait(spinner).await;
        *spinner_cleared = true;
    }
}

async fn cancel_spinner_and_wait(spinner: &mut Option<InlineAiSpinnerHandle>) {
    if let Some(handle) = spinner.take() {
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
        handle.send_text(batch, true)?;
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
        format!(" {message}")
    } else {
        message.to_string()
    };

    if let Some(handle) = output.as_ref() {
        handle.send_text(payload, false)?;
    }

    Ok(())
}

async fn finish_output(output: Option<LiveOutputHandle>) -> usize {
    if let Some(handle) = output {
        handle.finish().await
    } else {
        0
    }
}

fn record_inline_ai_completion(output_chars: usize) {
    if output_chars == 0 {
        return;
    }

    taurine_core::db::crud::record_trigger_stat(taurine_core::db::crud::TriggerStatEvent {
        trigger: None,
        trigger_chars: 0,
        success: true,
        output_chars,
        kind: taurine_core::db::crud::TriggerStatKind::InlineAi,
        wpm: None,
    });
}

enum LiveOutputCommand {
    Text { text: String, track_stats: bool },
    Finish,
}

struct LiveOutputHandle {
    tx: mpsc::Sender<LiveOutputCommand>,
    join: Option<thread::JoinHandle<usize>>,
}

impl LiveOutputHandle {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<LiveOutputCommand>();
        let join = thread::Builder::new()
            .name("tau-ai-stream".to_string())
            .spawn(move || {
                let mut session = crate::injector::StreamingTextSession::begin();

                while let Ok(command) = rx.recv() {
                    match command {
                        LiveOutputCommand::Text { text, track_stats } => {
                            if !session.push_text(&text, track_stats) {
                                break;
                            }
                        }
                        LiveOutputCommand::Finish => break,
                    }

                    if session.abort_requested() {
                        break;
                    }
                }

                session.finish()
            })
            .expect("Failed to spawn live output thread");

        Self {
            tx,
            join: Some(join),
        }
    }

    fn send_text(&self, text: String, track_stats: bool) -> taurine_core::error::Result<()> {
        self.tx
            .send(LiveOutputCommand::Text { text, track_stats })
            .map_err(|_| {
                taurine_core::error::Error::Service("Error: AI output interrupted.".to_string())
            })
    }

    async fn finish(mut self) -> usize {
        let _ = self.tx.send(LiveOutputCommand::Finish);
        if let Some(join) = self.join.take() {
            task::spawn_blocking(move || join.join().unwrap_or_default())
                .await
                .unwrap_or_default()
        } else {
            0
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

    #[test]
    fn chat_request_uses_inline_system_prompt_and_gemini_search_only() {
        let gemini = build_chat_request(AiProvider::Gemini, "latest rust release", None, None);
        assert_eq!(
            gemini.system.as_deref(),
            Some(taurine_core::settings::DEFAULT_AI_SYSTEM_PROMPT)
        );
        assert_eq!(gemini.tools.as_ref().map(|tools| tools.len()), Some(1));
        assert_eq!(
            gemini.tools.as_ref().unwrap()[0].name,
            genai::chat::ToolName::Custom("googleSearch".to_string())
        );

        let openai = build_chat_request(AiProvider::Openai, "latest rust release", None, None);
        assert_eq!(
            openai.system.as_deref(),
            Some(taurine_core::settings::DEFAULT_AI_SYSTEM_PROMPT)
        );
        assert!(openai.tools.is_none());
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

    #[test]
    fn test_split_outermost_markers() {
        let text = "hello \x03in1\x1Fp1\x04 world \x03\x03in2\x1Fp2\x04\x1Fp3\x04 end";
        let chunks = super::split_outermost_markers(text);
        assert_eq!(chunks.len(), 5);

        match &chunks[0] {
            super::Chunk::Text(t) => assert_eq!(t, "hello "),
            _ => panic!("Expected Text"),
        }
        match &chunks[1] {
            super::Chunk::Marker(m) => assert_eq!(m, "\x03in1\x1Fp1\x04"),
            _ => panic!("Expected Marker"),
        }
        match &chunks[2] {
            super::Chunk::Text(t) => assert_eq!(t, " world "),
            _ => panic!("Expected Text"),
        }
        match &chunks[3] {
            super::Chunk::Marker(m) => assert_eq!(m, "\x03\x03in2\x1Fp2\x04\x1Fp3\x04"),
            _ => panic!("Expected Marker"),
        }
        match &chunks[4] {
            super::Chunk::Text(t) => assert_eq!(t, " end"),
            _ => panic!("Expected Text"),
        }
    }
}
