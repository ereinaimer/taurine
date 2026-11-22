//! Cloud Speech-to-Text inference client communicating through Taurine's Supabase Edge Function gateway.
//!
//! Authenticates requests via the user's Supabase session access token and delegates Whisper transcription
//! and Llama post-processing to the server-side edge function.

use crate::error::{Error, Result};
use crate::voice::VoiceDictionary;

pub const DEFAULT_VOICE_FUNCTION_PATH: &str = "/functions/v1/voice";
pub const DEFAULT_GROQ_API_URL: &str = DEFAULT_VOICE_FUNCTION_PATH;
pub const DEFAULT_GROQ_CHAT_URL: &str = DEFAULT_VOICE_FUNCTION_PATH;
pub const DEFAULT_GROQ_MODEL: &str = "whisper-large-v3";
pub const DEFAULT_CLEANUP_MODEL: &str = "llama-3.1-8b-instant";

/// Returns the default Supabase Edge Function URL for voice dictation.
pub fn default_voice_gateway_url() -> String {
    let config = crate::cloud::CloudConfig::default_or_from_env();
    format!(
        "{}{}",
        config.supabase_url.trim_end_matches('/'),
        DEFAULT_VOICE_FUNCTION_PATH
    )
}

/// Sanitizes the cleaned transcript returned by the LLM by stripping spurious wrapper quotes
/// and markdown code fences.
pub fn sanitize_cleaned_text(raw_input: &str, cleaned: &str) -> String {
    let mut text = cleaned.trim();

    // Strip markdown code fence if present (handles multi-line, single-line, or unclosed fences)
    if let Some(rest) = text.strip_prefix("```") {
        text = if let Some(first_newline) = rest.find('\n') {
            &rest[first_newline + 1..]
        } else {
            rest
        };
        if let Some(last_fence) = text.rfind("```") {
            text = &text[..last_fence];
        }
        text = text.trim();
    }

    let raw_trimmed = raw_input.trim_start();
    // Strip surrounding double quotes or single quotes if the raw input did not start with them
    if ((text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\'')))
        && text.len() >= 2
        && !raw_trimmed.starts_with('"')
        && !raw_trimmed.starts_with('\'')
    {
        text = text[1..text.len() - 1].trim();
    } else if text.starts_with('“') && text.ends_with('”') && !raw_trimmed.starts_with('“') {
        text = text.trim_start_matches('“').trim_end_matches('”').trim();
    } else if text.starts_with('‘') && text.ends_with('’') && !raw_trimmed.starts_with('‘') {
        text = text.trim_start_matches('‘').trim_end_matches('’').trim();
    }

    text.to_string()
}

/// Compiles personal dictionary terms and active voice trigger phrases into a
/// concise comma-separated prompt string for Whisper acoustic and language biasing.
///
/// Bounded strictly within Whisper's 224-token prompt context window to prevent
/// silent front-truncation and acoustic hallucination.
pub fn build_whisper_prompt(
    dict: &VoiceDictionary,
    _trigger_phrases: &[impl AsRef<str>],
) -> Option<String> {
    const MAX_ITEMS: usize = 40;
    const MAX_CHARS: usize = 500;
    let mut collected: Vec<String> = Vec::new();
    let mut total_chars = 0;
    let mut seen = std::collections::HashSet::new();

    // Only include explicit personal dictionary terms to prevent biasing
    // Whisper's acoustic language classifier toward Latin/English script
    for term in dict.terms() {
        if collected.len() >= MAX_ITEMS {
            break;
        }
        let trimmed = term.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_lowercase()) {
            if total_chars + trimmed.len() + 2 > MAX_CHARS {
                continue;
            }
            total_chars += trimmed.len() + 2;
            collected.push(trimmed.to_string());
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(collected.join(", "))
    }
}

/// Cloud speech transcription client communicating with Taurine's Supabase Edge Function gateway.
#[derive(Debug, Clone)]
pub struct CloudVoiceClient {
    client: reqwest::Client,
    auth_token: Option<String>,
    endpoint_url: String,
    anon_key: String,
    cleanup_enabled: bool,
}

pub type GroqVoiceClient = CloudVoiceClient;

impl Default for CloudVoiceClient {
    fn default() -> Self {
        Self::from_env_or_keyring()
    }
}

impl CloudVoiceClient {
    /// Creates a client with an optionally supplied session auth token.
    pub fn new(auth_token: Option<String>) -> Self {
        let config = crate::cloud::CloudConfig::default_or_from_env();
        Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(4))
                .timeout(std::time::Duration::from_secs(12))
                .pool_idle_timeout(Some(std::time::Duration::from_secs(15)))
                .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
                .pool_max_idle_per_host(4)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            auth_token,
            endpoint_url: default_voice_gateway_url(),
            anon_key: config.anon_key,
            cleanup_enabled: true,
        }
    }

    /// Creates a client with custom endpoint and model (for testing against mock servers).
    pub fn with_endpoint(auth_token: Option<String>, endpoint_url: String, _model: String) -> Self {
        let config = crate::cloud::CloudConfig::default_or_from_env();
        Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(4))
                .timeout(std::time::Duration::from_secs(12))
                .pool_idle_timeout(Some(std::time::Duration::from_secs(15)))
                .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
                .pool_max_idle_per_host(4)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            auth_token,
            endpoint_url,
            anon_key: config.anon_key,
            cleanup_enabled: true,
        }
    }

    /// Creates a client with customized endpoints (signature retained for compatibility).
    pub fn with_full_endpoints(
        auth_token: Option<String>,
        endpoint_url: String,
        model: String,
        _chat_url: String,
        _cleanup_model: String,
    ) -> Self {
        Self::with_endpoint(auth_token, endpoint_url, model)
    }

    /// Enable or disable the secondary Llama LLM cleanup pass.
    pub fn with_cleanup_enabled(mut self, enabled: bool) -> Self {
        self.cleanup_enabled = enabled;
        self
    }

    /// Default constructor that automatically resolves authentication from the user's stored session.
    pub fn from_env_or_keyring() -> Self {
        Self::new(None)
    }

    /// Resolves the user's Supabase session access token from instance or local keyring.
    pub fn resolve_auth_token(&self) -> Option<String> {
        if let Some(token) = &self.auth_token {
            let trimmed = token.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        if let Ok(Some(tokens)) = crate::cloud::keyring::get_tokens() {
            let trimmed = tokens.access_token.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        None
    }

    /// Pre-warms the HTTP connection and TLS session in the background
    /// so subsequent transcription calls avoid connection setup latency.
    pub async fn prewarm(&self) {
        let _ = self
            .client
            .request(reqwest::Method::OPTIONS, &self.endpoint_url)
            .header("apikey", &self.anon_key)
            .send()
            .await;
    }

    /// Run the raw transcript through the edge function's Llama cleanup agent.
    pub async fn clean_transcript(
        &self,
        raw_transcript: &str,
        custom_terms: Option<&[String]>,
        language: Option<&str>,
    ) -> Result<String> {
        let trimmed_raw = raw_transcript.trim();
        if trimmed_raw.is_empty() {
            return Ok(String::new());
        }

        let auth_token = match self.resolve_auth_token() {
            Some(t) => t,
            None => {
                tracing::warn!(
                    "User not authenticated; skipping cleanup pass and returning raw transcript"
                );
                return Ok(trimmed_raw.to_string());
            }
        };

        let payload = serde_json::json!({
            "raw_text": trimmed_raw,
            "custom_terms": custom_terms,
            "language": language,
        });

        let response = match self
            .client
            .post(&self.endpoint_url)
            .header("Authorization", format!("Bearer {auth_token}"))
            .header("apikey", &self.anon_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "Voice cleanup gateway request failed ({e}); falling back to raw transcript"
                );
                return Ok(trimmed_raw.to_string());
            }
        };

        if !response.status().is_success() {
            tracing::warn!(
                "Voice cleanup gateway returned status {}; falling back to raw transcript",
                response.status()
            );
            return Ok(trimmed_raw.to_string());
        }

        #[derive(serde::Deserialize)]
        struct CleanupResponse {
            text: Option<String>,
        }

        let parsed: CleanupResponse = match response.json().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to parse voice cleanup JSON response ({e}); falling back");
                return Ok(trimmed_raw.to_string());
            }
        };

        if let Some(content) = parsed.text {
            let sanitized = sanitize_cleaned_text(trimmed_raw, &content);
            return Ok(sanitized);
        }

        Ok(trimmed_raw.to_string())
    }

    /// Transcribe 16-bit PCM WAV audio bytes via the Supabase Edge Function gateway.
    pub async fn transcribe_wav(
        &self,
        wav_bytes: &[u8],
        prompt: Option<&str>,
        custom_terms: Option<&[String]>,
        preferred_languages: Option<&str>,
    ) -> Result<String> {
        let auth_token = self.resolve_auth_token().ok_or_else(|| {
            let diag = crate::diagnostic::Diagnostic::problem(
                "Taurine account authentication required for cloud voice dictation",
            )
            .help("Please log in to your Taurine account to activate voice dictation")
            .example("taurine login");
            Error::Config(diag.render())
        })?;

        let build_form = || -> Result<reqwest::multipart::Form> {
            let mut form = reqwest::multipart::Form::new().text(
                "cleanup",
                if self.cleanup_enabled {
                    "true"
                } else {
                    "false"
                },
            );

            if let Some(p) = prompt
                && !p.trim().is_empty()
            {
                form = form.text("prompt", p.trim().to_string());
            }

            if let Some(langs) = preferred_languages
                && !langs.trim().is_empty()
                && !langs.trim().eq_ignore_ascii_case("auto")
            {
                form = form.text("preferred_languages", langs.trim().to_string());
            }

            if let Some(terms) = custom_terms
                && !terms.is_empty()
                && let Ok(terms_json) = serde_json::to_string(terms)
            {
                form = form.text("custom_terms", terms_json);
            }

            form = form.part(
                "file",
                reqwest::multipart::Part::bytes(wav_bytes.to_vec())
                    .file_name("dictation.wav")
                    .mime_str("audio/wav")
                    .map_err(|e| {
                        Error::Service(format!("Failed to build multipart audio part: {e}"))
                    })?,
            );
            Ok(form)
        };

        let max_attempts = 2;
        let mut attempts = 0;
        let response = loop {
            attempts += 1;
            let form = build_form()?;

            tracing::info!(
                "CloudVoiceClient: sending transcription request (attempt {}/{}, preferred_languages: {:?}, wav_bytes: {})",
                attempts,
                max_attempts,
                preferred_languages,
                wav_bytes.len()
            );

            match self
                .client
                .post(&self.endpoint_url)
                .header("Authorization", format!("Bearer {auth_token}"))
                .header("apikey", &self.anon_key)
                .multipart(form)
                .send()
                .await
            {
                Ok(resp) => break resp,
                Err(e) => {
                    tracing::warn!(
                        "CloudVoiceClient: attempt {}/{} failed ({e})",
                        attempts,
                        max_attempts
                    );
                    if attempts < max_attempts {
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                        continue;
                    }
                    return Err(Error::Service(format!(
                        "Failed to send transcription request to voice gateway: {e}"
                    )));
                }
            }
        };

        let status = response.status();
        if status.as_u16() == 401 {
            return Err(Error::Config(
                "Voice gateway authentication failed: please sign in again with 'taurine login'"
                    .to_string(),
            ));
        }

        if status.as_u16() == 429 {
            return Err(Error::Service(
                "Voice dictation rate limit exceeded: please wait a moment before dictating again"
                    .to_string(),
            ));
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Service(format!(
                "Voice gateway request failed with HTTP {status}: {body}"
            )));
        }

        #[derive(serde::Deserialize)]
        struct GatewayResponse {
            text: String,
            #[serde(default)]
            raw_text: Option<String>,
            #[serde(default)]
            language: Option<String>,
        }

        let parsed: GatewayResponse = response.json().await.map_err(|e| {
            Error::Service(format!("Failed to parse voice gateway JSON response: {e}"))
        })?;

        tracing::info!(
            "CloudVoiceClient: gateway returned HTTP {status}, language: {:?}, raw_len: {}, final_len: {}",
            parsed.language,
            parsed.raw_text.as_ref().map(|s| s.len()).unwrap_or(0),
            parsed.text.len()
        );

        Ok(parsed.text)
    }

    /// Transcribe raw 16kHz mono audio samples asynchronously, automatically splitting
    /// long-form recordings into silence-aware chunks and running a single unified cleanup pass.
    pub async fn transcribe_samples(
        &self,
        samples: &[f32],
        sample_rate: u32,
        prompt: Option<&str>,
        custom_terms: Option<&[String]>,
        preferred_languages: Option<&str>,
    ) -> Result<String> {
        self.transcribe_samples_bounded(
            samples,
            sample_rate,
            prompt,
            custom_terms,
            preferred_languages,
            crate::voice::vad::DEFAULT_CHUNK_MAX_SAMPLES,
            crate::voice::vad::DEFAULT_CHUNK_MIN_SAMPLES,
        )
        .await
    }

    /// Transcribe raw audio samples with customizable chunk limits.
    #[allow(clippy::too_many_arguments)]
    pub async fn transcribe_samples_bounded(
        &self,
        samples: &[f32],
        sample_rate: u32,
        prompt: Option<&str>,
        custom_terms: Option<&[String]>,
        preferred_languages: Option<&str>,
        max_chunk_samples: usize,
        min_split_samples: usize,
    ) -> Result<String> {
        if samples.len() > max_chunk_samples {
            let chunks = crate::voice::split_audio_into_chunks(
                samples,
                max_chunk_samples,
                min_split_samples,
            );
            tracing::debug!(
                "Splitting long-form audio ({} samples) into {} silence-aware chunks",
                samples.len(),
                chunks.len()
            );

            // To avoid redundant per-chunk Llama cleanup passes and preserve long-form context,
            // individual chunks are transcribed with raw Whisper, then unified and cleaned once.
            let raw_client = self.clone().with_cleanup_enabled(false);
            let mut chunk_texts = Vec::new();
            let mut dynamic_prompt = prompt.map(str::to_string);

            for (idx, chunk) in chunks.iter().enumerate() {
                if !crate::voice::has_voice_activity(chunk) {
                    tracing::debug!(
                        "Skipping chunk {}/{} due to lack of voice activity",
                        idx + 1,
                        chunks.len()
                    );
                    continue;
                }
                let wav = crate::voice::encode_pcm16_wav(chunk, sample_rate, 1);
                let text = match raw_client
                    .transcribe_wav(
                        &wav,
                        dynamic_prompt.as_deref(),
                        custom_terms,
                        preferred_languages,
                    )
                    .await
                {
                    Ok(t) => t,
                    Err(first_err) => {
                        tracing::warn!(
                            "Transcription of chunk {}/{} failed ({first_err}); retrying once",
                            idx + 1,
                            chunks.len()
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        match raw_client
                            .transcribe_wav(
                                &wav,
                                dynamic_prompt.as_deref(),
                                custom_terms,
                                preferred_languages,
                            )
                            .await
                        {
                            Ok(t) => t,
                            Err(retry_err) => {
                                if !chunk_texts.is_empty() {
                                    tracing::warn!(
                                        "Chunk {}/{} failed permanently ({retry_err}); salvaging {} earlier chunk(s)",
                                        idx + 1,
                                        chunks.len(),
                                        chunk_texts.len()
                                    );
                                    break;
                                }
                                return Err(retry_err);
                            }
                        }
                    }
                };
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let tail = match trimmed.char_indices().rev().nth(99) {
                        Some((idx, _)) => &trimmed[idx..],
                        None => trimmed,
                    };
                    if let Some(base) = prompt {
                        dynamic_prompt = Some(format!("{base} {tail}"));
                    } else {
                        dynamic_prompt = Some(tail.to_string());
                    }
                    chunk_texts.push(trimmed.to_string());
                }
            }

            let full_raw = chunk_texts.join(" ");
            let trimmed_full = full_raw.trim();
            if trimmed_full.is_empty() {
                return Ok(String::new());
            }

            if self.cleanup_enabled {
                self.clean_transcript(trimmed_full, custom_terms, preferred_languages)
                    .await
            } else {
                Ok(trimmed_full.to_string())
            }
        } else {
            let wav = crate::voice::encode_pcm16_wav(samples, sample_rate, 1);
            self.transcribe_wav(&wav, prompt, custom_terms, preferred_languages)
                .await
        }
    }

    /// Synchronous transcription entry point for session manager threads.
    pub fn transcribe_blocking(
        &self,
        samples: &[f32],
        sample_rate: u32,
        prompt: Option<&str>,
        custom_terms: Option<&[String]>,
        preferred_languages: Option<&str>,
    ) -> Result<String> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| {
                    handle.block_on(self.transcribe_samples(
                        samples,
                        sample_rate,
                        prompt,
                        custom_terms,
                        preferred_languages,
                    ))
                })
            }
            Ok(_) => {
                let this = self.clone();
                let samples_vec = samples.to_vec();
                let prompt = prompt.map(str::to_string);
                let custom_terms = custom_terms.map(|t| t.to_vec());
                let preferred_languages = preferred_languages.map(str::to_string);
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| {
                            Error::Service(format!("Failed to create async runtime: {e}"))
                        })?;
                    rt.block_on(this.transcribe_samples(
                        &samples_vec,
                        sample_rate,
                        prompt.as_deref(),
                        custom_terms.as_deref(),
                        preferred_languages.as_deref(),
                    ))
                })
                .join()
                .map_err(|_| {
                    Error::Service("Voice transcription worker thread panicked".to_string())
                })?
            }
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| Error::Service(format!("Failed to create async runtime: {e}")))?;
                rt.block_on(self.transcribe_samples(
                    samples,
                    sample_rate,
                    prompt,
                    custom_terms,
                    preferred_languages,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn read_mock_request(stream: &mut std::net::TcpStream) -> String {
        let mut data = Vec::new();
        let mut buf = [0u8; 1024];
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
            if data.len() > 3000 {
                break;
            }
        }
        String::from_utf8_lossy(&data).to_string()
    }

    #[test]
    fn test_build_whisper_prompt_formatting_and_deduplication() {
        let dict = VoiceDictionary::from_csv("Taurine, PostgreSQL, API, taurine, POSTGRESQL");
        let triggers = vec!["open browser", "send email"];

        let prompt = build_whisper_prompt(&dict, &triggers);
        assert!(prompt.is_some(), "Prompt must not be empty");
        let prompt_str = prompt.unwrap();

        // Must be comma-separated
        assert!(prompt_str.contains("Taurine, PostgreSQL, API"));

        // Deduplication: taurine and postgresql should only appear once
        let lower = prompt_str.to_lowercase();
        assert_eq!(
            lower.matches("taurine").count(),
            1,
            "Taurine must be deduplicated"
        );
        assert_eq!(
            lower.matches("postgresql").count(),
            1,
            "PostgreSQL must be deduplicated"
        );
    }

    #[tokio::test]
    async fn test_prewarm_connection_succeeds_without_blocking() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_mock_request(&mut stream);
            assert!(req.starts_with("OPTIONS /voice HTTP/1.1"));
            let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n";
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-token".to_string()),
            format!("http://127.0.0.1:{port}/voice"),
            "whisper-large-v3".to_string(),
        );

        client.prewarm().await;
        server_thread.join().unwrap();
    }

    #[test]
    fn test_build_whisper_prompt_uses_only_dictionary_terms() {
        let dict = VoiceDictionary::from_csv("Taurine, PostgreSQL");
        // Trigger phrases should not contaminate the Whisper acoustic prompt
        let prompt = build_whisper_prompt(&dict, &["brb", "btw", "curl", "gh"]).unwrap();
        assert!(prompt.contains("Taurine"));
        assert!(prompt.contains("PostgreSQL"));
        assert!(!prompt.contains("brb"));
        assert!(!prompt.contains("btw"));
    }

    #[tokio::test]
    async fn test_unauthenticated_client_returns_diagnostic_error() {
        {
            let _lock = crate::testing::TEST_LOCK.lock().unwrap();
            crate::testing::use_shared_test_keyring();
            let _ = crate::cloud::keyring::clear_tokens();
        }

        let client = GroqVoiceClient::new(None);
        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let result = client.transcribe_wav(&wav, None, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("authentication required")
                || err.contains("log in to your Taurine account"),
            "Error was: {err}"
        );
    }

    #[tokio::test]
    async fn test_transcribe_wav_mock_success() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_mock_request(&mut stream);

            assert!(req.starts_with("POST /transcribe HTTP/1.1"));
            assert!(
                req.to_lowercase()
                    .contains("authorization: bearer test-groq-key")
            );
            assert!(req.contains("multipart/form-data"));

            let response_body = r#"{"text": "Hello world, this is Taurine cloud voice."}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        // Disable cleanup for raw transcription mock test
        let client = GroqVoiceClient::with_endpoint(
            Some("test-groq-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        )
        .with_cleanup_enabled(false);

        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let transcript = client.transcribe_wav(&wav, None, None, None).await.unwrap();

        assert_eq!(transcript, "Hello world, this is Taurine cloud voice.");
        server_thread.join().unwrap();
    }

    #[tokio::test]
    async fn test_transcribe_wav_includes_prompt() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_mock_request(&mut stream);

            assert!(req.contains("name=\"prompt\""));
            assert!(req.contains("Taurine, Whisper, Rust"));

            let response_body = r#"{"text": "Taurine Whisper Rust test."}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-groq-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        )
        .with_cleanup_enabled(false);

        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let transcript = client
            .transcribe_wav(&wav, Some("Taurine, Whisper, Rust"), None, None)
            .await
            .unwrap();

        assert_eq!(transcript, "Taurine Whisper Rust test.");
        server_thread.join().unwrap();
    }

    #[tokio::test]
    async fn test_transcribe_wav_includes_preferred_languages() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_mock_request(&mut stream);

            assert!(req.contains("name=\"preferred_languages\""));
            assert!(req.contains("en,ml"));

            let response_body = r#"{"text": "Malayalam dictation verified"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-groq-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        )
        .with_cleanup_enabled(false);

        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let transcript = client
            .transcribe_wav(&wav, None, None, Some("en,ml"))
            .await
            .unwrap();

        assert_eq!(transcript, "Malayalam dictation verified");
        server_thread.join().unwrap();
    }

    #[tokio::test]
    async fn test_transcribe_wav_omits_empty_preferred_languages() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_mock_request(&mut stream);

            assert!(!req.contains("name=\"preferred_languages\""));

            let response_body = r#"{"text": "Default dictation"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-groq-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        )
        .with_cleanup_enabled(false);

        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let transcript = client
            .transcribe_wav(&wav, None, None, Some("auto"))
            .await
            .unwrap();

        assert_eq!(transcript, "Default dictation");
        server_thread.join().unwrap();
    }

    #[tokio::test]
    async fn test_transcribe_wav_401_unauthorized() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_mock_request(&mut stream);
            let response_body =
                r#"{"error":{"message":"Invalid API Key","type":"invalid_request_error"}}"#;
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("bad-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        );

        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let result = client.transcribe_wav(&wav, None, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("authentication failed") || err.contains("invalid or unauthorized"));
        server_thread.join().unwrap();
    }

    #[tokio::test]
    async fn test_transcribe_wav_429_rate_limited() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_mock_request(&mut stream);
            let response_body = r#"{"error":{"message":"Rate limit reached","type":"tokens"}}"#;
            let response = format!(
                "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        );

        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let result = client.transcribe_wav(&wav, None, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("rate limit"));
        server_thread.join().unwrap();
    }

    #[test]
    fn test_sanitize_cleaned_text_strips_fences_and_wrapper_quotes() {
        // Multi-line code fence
        assert_eq!(
            sanitize_cleaned_text("hello", "```text\nHello world.\n```"),
            "Hello world."
        );
        assert_eq!(
            sanitize_cleaned_text("hello", "```\nHello world.\n```"),
            "Hello world."
        );
        // Single-line code fence
        assert_eq!(
            sanitize_cleaned_text("hello", "```Hello world```"),
            "Hello world"
        );
        // Unclosed code fence without newline
        assert_eq!(
            sanitize_cleaned_text("hello", "```Hello world"),
            "Hello world"
        );
        // Double quotes
        assert_eq!(
            sanitize_cleaned_text("hello", "\"Hello world.\""),
            "Hello world."
        );
        // Single quotes
        assert_eq!(
            sanitize_cleaned_text("hello", "'Hello world.'"),
            "Hello world."
        );
        // Typographic curly quotes
        assert_eq!(
            sanitize_cleaned_text("hello", "“Hello world.”"),
            "Hello world."
        );
        assert_eq!(
            sanitize_cleaned_text("hello", "‘Hello world.’"),
            "Hello world."
        );
        // Preserves quotes if original input explicitly contained quotes
        assert_eq!(
            sanitize_cleaned_text("\"hello\"", "\"Hello world.\""),
            "\"Hello world.\""
        );
    }

    #[test]
    fn test_build_whisper_prompt_character_cap() {
        let long_terms: Vec<String> = (0..50).map(|i| format!("term_number_{i}")).collect();
        let dict = VoiceDictionary::from_csv(&long_terms.join(", "));
        let prompt = build_whisper_prompt(&dict, &["trigger 1", "trigger 2"]).unwrap();
        assert!(
            prompt.len() <= 500,
            "Prompt must be bounded below 500 characters, got {}",
            prompt.len()
        );
    }

    #[tokio::test]
    async fn test_clean_transcript_calls_edge_gateway() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_mock_request(&mut stream);

            assert!(req.starts_with("POST /voice HTTP/1.1"));
            assert!(
                req.to_lowercase()
                    .contains("authorization: bearer test-llama-key")
            );
            assert!(req.contains("raw_text"));
            assert!(req.contains("um hello like this is a test"));
            assert!(req.contains("Taurine"));

            let response_body = r#"{"text": "Hello, this is a test."}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-llama-key".to_string()),
            format!("http://127.0.0.1:{port}/voice"),
            "whisper-large-v3".to_string(),
        );

        let custom_terms = vec!["Taurine".to_string(), "Rust".to_string()];
        let cleaned = client
            .clean_transcript("um hello like this is a test", Some(&custom_terms), None)
            .await
            .unwrap();
        assert_eq!(cleaned, "Hello, this is a test.");
        server_thread.join().unwrap();
    }

    #[tokio::test]
    async fn test_clean_transcript_falls_back_on_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_mock_request(&mut stream);

            let response_body = r#"{"error":{"message":"Internal server error"}}"#;
            let response = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-llama-key".to_string()),
            format!("http://127.0.0.1:{port}/voice"),
            "whisper-large-v3".to_string(),
        );

        let result = client
            .clean_transcript("raw text fallback", None, None)
            .await
            .unwrap();
        assert_eq!(result, "raw text fallback");
        server_thread.join().unwrap();
    }

    #[tokio::test]
    async fn test_clean_transcript_strips_filler_only_utterances() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_mock_request(&mut stream);

            let response_body = r#"{"text": ""}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-llama-key".to_string()),
            format!("http://127.0.0.1:{port}/voice"),
            "whisper-large-v3".to_string(),
        );

        let cleaned = client.clean_transcript("um uh", None, None).await.unwrap();
        assert_eq!(cleaned, "");
        server_thread.join().unwrap();
    }

    #[test]
    fn test_transcribe_blocking_current_thread_runtime_safety() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_mock_request(&mut stream);
            let response_body = r#"{"text": "blocking test"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        )
        .with_cleanup_enabled(false);

        // Execute transcribe_blocking from inside a single-threaded runtime to ensure no panics
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result =
            rt.block_on(async { client.transcribe_blocking(&[0.0; 160], 16000, None, None, None) });

        assert_eq!(result.unwrap(), "blocking test");
        server_thread.join().unwrap();
    }

    #[test]
    fn test_transcribe_blocking_multi_thread_runtime_safety() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_mock_request(&mut stream);
            let response_body = r#"{"text": "multithread blocking test"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        )
        .with_cleanup_enabled(false);

        // Execute transcribe_blocking from inside a multi-threaded runtime
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();

        let result =
            rt.block_on(async { client.transcribe_blocking(&[0.0; 160], 16000, None, None, None) });

        assert_eq!(result.unwrap(), "multithread blocking test");
        server_thread.join().unwrap();
    }

    #[tokio::test]
    async fn test_transcribe_samples_multi_chunk_stitching_and_cleanup() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let mut streams = Vec::new();

            // First chunk request (raw chunk)
            let (mut stream1, _) = listener.accept().unwrap();
            let req1 = read_mock_request(&mut stream1);
            assert!(req1.contains("/voice"));
            let body1 = r#"{"text": "First paragraph intro"}"#;
            let resp1 = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body1.len(),
                body1
            );
            stream1.write_all(resp1.as_bytes()).unwrap();
            let _ = stream1.flush();
            streams.push(stream1);

            // Second chunk request (raw chunk)
            let (mut stream2, _) = listener.accept().unwrap();
            let req2 = read_mock_request(&mut stream2);
            assert!(req2.contains("/voice"));
            assert!(req2.contains("First paragraph intro"));
            let body2 = r#"{"text": "item one and item two"}"#;
            let resp2 = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body2.len(),
                body2
            );
            stream2.write_all(resp2.as_bytes()).unwrap();
            let _ = stream2.flush();
            streams.push(stream2);

            // Unified cleanup request (JSON cleanup)
            let (mut stream3, _) = listener.accept().unwrap();
            let req3 = read_mock_request(&mut stream3);
            assert!(req3.contains("/voice"));
            assert!(req3.contains("First paragraph intro item one and item two"));
            let body3 = r#"{"text": "First paragraph intro.\n\n1. Item one\n2. Item two"}"#;
            let resp3 = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body3.len(),
                body3
            );
            stream3.write_all(resp3.as_bytes()).unwrap();
            let _ = stream3.flush();
            streams.push(stream3);

            std::thread::sleep(std::time::Duration::from_millis(50));
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-key".to_string()),
            format!("http://127.0.0.1:{port}/voice"),
            "whisper-large-v3".to_string(),
        );

        // Create 2 chunks with voice activity separated by silence valley
        // Chunk size: max 6000 samples, min split 3000 samples
        let mut samples = vec![0.05f32; 10000];
        for sample in &mut samples[4000..4600] {
            *sample = 0.0001; // Silence valley
        }

        let result = client
            .transcribe_samples_bounded(&samples, 16000, None, None, None, 6000, 3000)
            .await
            .unwrap();

        assert_eq!(result, "First paragraph intro.\n\n1. Item one\n2. Item two");
        server_thread.join().unwrap();
    }

    #[tokio::test]
    async fn test_transcribe_samples_salvages_partial_chunks_on_subsequent_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let mut streams = Vec::new();

            // First chunk request: succeeds
            let (mut stream1, _) = listener.accept().unwrap();
            let _ = read_mock_request(&mut stream1);
            let body1 = r#"{"text": "First chunk succeeded"}"#;
            let resp1 = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body1.len(),
                body1
            );
            stream1.write_all(resp1.as_bytes()).unwrap();
            let _ = stream1.flush();
            streams.push(stream1);

            // Second chunk request: fails with 500 Internal Server Error
            let (mut stream2, _) = listener.accept().unwrap();
            let _ = read_mock_request(&mut stream2);
            let body2 = r#"{"error": {"message": "Service unavailable"}}"#;
            let resp2 = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body2.len(),
                body2
            );
            stream2.write_all(resp2.as_bytes()).unwrap();
            let _ = stream2.flush();
            streams.push(stream2);

            // Retry for second chunk request: fails again with 500
            let (mut stream3, _) = listener.accept().unwrap();
            let _ = read_mock_request(&mut stream3);
            let resp3 = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body2.len(),
                body2
            );
            stream3.write_all(resp3.as_bytes()).unwrap();
            let _ = stream3.flush();
            streams.push(stream3);

            // Unified cleanup request for salvaged chunk 1
            let (mut stream4, _) = listener.accept().unwrap();
            let req4 = read_mock_request(&mut stream4);
            assert!(req4.contains("First chunk succeeded"));
            let body4 = r#"{"text": "First chunk succeeded."}"#;
            let resp4 = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body4.len(),
                body4
            );
            stream4.write_all(resp4.as_bytes()).unwrap();
            let _ = stream4.flush();
            streams.push(stream4);

            std::thread::sleep(std::time::Duration::from_millis(50));
        });

        let client = GroqVoiceClient::with_endpoint(
            Some("test-key".to_string()),
            format!("http://127.0.0.1:{port}/voice"),
            "whisper-large-v3".to_string(),
        );

        // 2 chunks separated by silence valley
        let mut samples = vec![0.05f32; 10000];
        for sample in &mut samples[4000..4600] {
            *sample = 0.0001;
        }

        let result = client
            .transcribe_samples_bounded(&samples, 16000, None, None, None, 6000, 3000)
            .await;

        // Must salvage the successfully transcribed first chunk instead of failing!
        assert!(
            result.is_ok(),
            "Expected salvaged partial result, got {result:?}"
        );
        assert_eq!(result.unwrap(), "First chunk succeeded.");
        server_thread.join().unwrap();
    }
}
