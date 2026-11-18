//! Groq Whisper Cloud Speech-to-Text inference client with Llama LLM post-processing.

use crate::error::{Error, Result};
use crate::voice::VoiceDictionary;

pub const DEFAULT_GROQ_API_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
pub const DEFAULT_GROQ_CHAT_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
pub const DEFAULT_GROQ_MODEL: &str = "whisper-large-v3";
pub const DEFAULT_CLEANUP_MODEL: &str = "llama-3.1-8b-instant";

/// Parse a specific key from a `.env` file string without external crate dependencies.
pub fn read_env_file_key(content: &str, target_key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=')
            && k.trim() == target_key
        {
            let unquoted = v.trim().trim_matches('"').trim_matches('\'').trim();
            if !unquoted.is_empty() {
                return Some(unquoted.to_string());
            }
        }
    }
    None
}

/// Discovers and loads a variable from `.env` in the current directory, parent directories, or exe dir.
pub fn load_key_from_env_file(target_key: &str) -> Option<String> {
    let mut check_dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = Some(cwd.as_path());
        let mut depth = 0;
        while let Some(d) = cur
            && depth < 4
        {
            check_dirs.push(d.to_path_buf());
            cur = d.parent();
            depth += 1;
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        check_dirs.push(parent.to_path_buf());
    }

    for dir in check_dirs {
        let path = dir.join(".env");
        if let Ok(content) = std::fs::read_to_string(path)
            && let Some(val) = read_env_file_key(&content, target_key)
        {
            return Some(val);
        }
    }
    None
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
    trigger_phrases: &[impl AsRef<str>],
) -> Option<String> {
    const MAX_ITEMS: usize = 40;
    const MAX_CHARS: usize = 500;
    let mut collected: Vec<String> = Vec::new();
    let mut total_chars = 0;
    let mut seen = std::collections::HashSet::new();

    // 1. Personal dictionary first (highest priority)
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

    // 2. Voice triggers second
    if collected.len() < MAX_ITEMS && total_chars < MAX_CHARS {
        for trigger in trigger_phrases {
            if collected.len() >= MAX_ITEMS {
                break;
            }
            let trimmed = trigger.as_ref().trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_lowercase()) {
                if total_chars + trimmed.len() + 2 > MAX_CHARS {
                    continue;
                }
                total_chars += trimmed.len() + 2;
                collected.push(trimmed.to_string());
            }
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(collected.join(", "))
    }
}

/// Cloud speech transcription client communicating with Groq's Whisper API and Llama LLM.
#[derive(Debug, Clone)]
pub struct GroqVoiceClient {
    client: reqwest::Client,
    api_key: Option<String>,
    endpoint_url: String,
    model: String,
    chat_url: String,
    cleanup_model: String,
    cleanup_enabled: bool,
}

impl Default for GroqVoiceClient {
    fn default() -> Self {
        Self::from_env_or_keyring()
    }
}

impl GroqVoiceClient {
    /// Creates a client with an explicitly supplied API key.
    pub fn new(api_key: Option<String>) -> Self {
        Self::with_endpoint(
            api_key,
            DEFAULT_GROQ_API_URL.to_string(),
            DEFAULT_GROQ_MODEL.to_string(),
        )
    }

    /// Creates a client with custom endpoint and model (for testing or alternate endpoints).
    pub fn with_endpoint(api_key: Option<String>, endpoint_url: String, model: String) -> Self {
        Self::with_full_endpoints(
            api_key,
            endpoint_url,
            model,
            DEFAULT_GROQ_CHAT_URL.to_string(),
            DEFAULT_CLEANUP_MODEL.to_string(),
        )
    }

    /// Creates a client with customized speech and cleanup LLM endpoints.
    pub fn with_full_endpoints(
        api_key: Option<String>,
        endpoint_url: String,
        model: String,
        chat_url: String,
        cleanup_model: String,
    ) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            api_key,
            endpoint_url,
            model,
            chat_url,
            cleanup_model,
            cleanup_enabled: true,
        }
    }

    /// Enable or disable the secondary Llama LLM cleanup pass.
    pub fn with_cleanup_enabled(mut self, enabled: bool) -> Self {
        self.cleanup_enabled = enabled;
        self
    }

    /// Resolves API key from environment (`GROQ_API_KEY`) or workspace `.env` file.
    pub fn from_env_or_keyring() -> Self {
        Self::new(None)
    }

    /// Resolves the effective API key from instance, environment, or `.env` file.
    pub fn resolve_api_key(&self) -> Option<String> {
        if let Some(key) = &self.api_key {
            let trimmed = key.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        if let Ok(key) = std::env::var("GROQ_API_KEY") {
            let trimmed = key.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        load_key_from_env_file("GROQ_API_KEY")
    }

    /// Run the raw transcript through Groq's Llama-3.1-8b cleanup agent.
    pub async fn clean_transcript(
        &self,
        raw_transcript: &str,
        custom_terms: Option<&[String]>,
    ) -> Result<String> {
        let trimmed_raw = raw_transcript.trim();
        if trimmed_raw.is_empty() {
            return Ok(String::new());
        }

        let api_key = match self.resolve_api_key() {
            Some(k) => k,
            None => return Ok(trimmed_raw.to_string()),
        };

        let mut system_prompt = "You are an expert voice dictation cleanup engine. Transform the raw transcribed speech into clean, well-formatted, professional text.\nRules:\n- Correct punctuation, capitalization, and minor grammatical slips.\n- Remove conversational filler words (e.g. um, uh, ah, like, you know) and accidental stutter repetitions.\n- Format numbers, currencies, dates, email addresses, and technical terms appropriately.\n- Preserve the speaker's original intent, vocabulary, and meaning exactly.\n- If the speech contains only conversational filler words, stuttering, or ambient noise with no substantive content, output an empty string.\n- Output ONLY the final cleaned text with no surrounding quotes, explanations, markdown code blocks, or notes.".to_string();

        if let Some(terms) = custom_terms
            && !terms.is_empty()
        {
            const MAX_CUSTOM_TERMS: usize = 50;
            const MAX_CUSTOM_CHARS: usize = 1000;
            let mut bounded_terms = Vec::new();
            let mut total_chars = 0;
            for term in terms.iter() {
                if bounded_terms.len() >= MAX_CUSTOM_TERMS {
                    break;
                }
                let trimmed = term.trim();
                if !trimmed.is_empty() {
                    if total_chars + trimmed.len() + 2 > MAX_CUSTOM_CHARS {
                        continue;
                    }
                    total_chars += trimmed.len() + 2;
                    bounded_terms.push(trimmed);
                }
            }
            if !bounded_terms.is_empty() {
                let terms_list = bounded_terms.join(", ");
                system_prompt.push_str(&format!(
                    "\n- Domain vocabulary to preserve and format verbatim if phonetically spoken: {terms_list}."
                ));
            }
        }

        let payload = serde_json::json!({
            "model": self.cleanup_model,
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": trimmed_raw
                }
            ],
            "temperature": 0.1,
            "max_tokens": 1024
        });

        let response = match self
            .client
            .post(&self.chat_url)
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&payload)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    "Llama cleanup request failed ({e}); falling back to raw transcript"
                );
                return Ok(trimmed_raw.to_string());
            }
        };

        if !response.status().is_success() {
            tracing::warn!(
                "Llama cleanup returned status {}; falling back to raw transcript",
                response.status()
            );
            return Ok(trimmed_raw.to_string());
        }

        #[derive(serde::Deserialize)]
        struct ChatChoice {
            message: ChatMessage,
        }

        #[derive(serde::Deserialize)]
        struct ChatMessage {
            content: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct ChatResponse {
            choices: Vec<ChatChoice>,
        }

        let parsed: ChatResponse = match response.json().await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to parse Llama cleanup response ({e}); falling back");
                return Ok(trimmed_raw.to_string());
            }
        };

        if let Some(first) = parsed.choices.into_iter().next()
            && let Some(content) = first.message.content
        {
            let sanitized = sanitize_cleaned_text(trimmed_raw, &content);
            return Ok(sanitized);
        }

        Ok(trimmed_raw.to_string())
    }

    /// Transcribe 16-bit PCM WAV audio bytes.
    pub async fn transcribe_wav(
        &self,
        wav_bytes: &[u8],
        prompt: Option<&str>,
        custom_terms: Option<&[String]>,
    ) -> Result<String> {
        let api_key = self.resolve_api_key().ok_or_else(|| {
            let diag = crate::diagnostic::Diagnostic::problem(
                "Groq API key not configured for voice dictation",
            )
            .help("Set GROQ_API_KEY in your .env file or environment variables")
            .example("GROQ_API_KEY=gsk_...");
            Error::Config(diag.render())
        })?;

        let mut form = reqwest::multipart::Form::new()
            .text("model", self.model.clone())
            .text("response_format", "json")
            .text("temperature", "0");

        if let Some(p) = prompt
            && !p.trim().is_empty()
        {
            form = form.text("prompt", p.trim().to_string());
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

        let response = self
            .client
            .post(&self.endpoint_url)
            .header("Authorization", format!("Bearer {api_key}"))
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                Error::Service(format!("Failed to send transcription request to Groq: {e}"))
            })?;

        let status = response.status();
        if status.as_u16() == 401 {
            return Err(Error::Config(
                "Groq API authentication failed: invalid or unauthorized API key".to_string(),
            ));
        }

        if status.as_u16() == 429 {
            return Err(Error::Service(
                "Groq rate limit exceeded: please wait a moment before dictating again".to_string(),
            ));
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Service(format!(
                "Groq transcription request failed with HTTP {status}: {body}"
            )));
        }

        #[derive(serde::Deserialize)]
        struct WhisperResponse {
            text: String,
        }

        let parsed: WhisperResponse = response.json().await.map_err(|e| {
            Error::Service(format!("Failed to parse Groq Whisper JSON response: {e}"))
        })?;

        let raw_transcript = parsed.text.trim();
        if self.cleanup_enabled && !raw_transcript.is_empty() {
            self.clean_transcript(raw_transcript, custom_terms).await
        } else {
            Ok(raw_transcript.to_string())
        }
    }

    /// Transcribe raw 16kHz mono audio samples asynchronously.
    pub async fn transcribe_samples(
        &self,
        samples: &[f32],
        sample_rate: u32,
        prompt: Option<&str>,
        custom_terms: Option<&[String]>,
    ) -> Result<String> {
        let wav = crate::voice::encode_pcm16_wav(samples, sample_rate, 1);
        self.transcribe_wav(&wav, prompt, custom_terms).await
    }

    /// Synchronous transcription entry point for session manager threads.
    pub fn transcribe_blocking(
        &self,
        samples: &[f32],
        sample_rate: u32,
        prompt: Option<&str>,
        custom_terms: Option<&[String]>,
    ) -> Result<String> {
        let wav = crate::voice::encode_pcm16_wav(samples, sample_rate, 1);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
                tokio::task::block_in_place(|| {
                    handle.block_on(self.transcribe_wav(&wav, prompt, custom_terms))
                })
            } else {
                let this = self.clone();
                let prompt = prompt.map(str::to_string);
                let custom_terms = custom_terms.map(|t| t.to_vec());
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| {
                            Error::Service(format!("Failed to create async runtime: {e}"))
                        })?;
                    rt.block_on(this.transcribe_wav(
                        &wav,
                        prompt.as_deref(),
                        custom_terms.as_deref(),
                    ))
                })
                .join()
                .map_err(|_| {
                    Error::Service("Voice transcription worker thread panicked".to_string())
                })?
            }
        } else {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| Error::Service(format!("Failed to create async runtime: {e}")))?;
            rt.block_on(self.transcribe_wav(&wav, prompt, custom_terms))
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
        let dict = VoiceDictionary::from_csv("Taurine, PostgreSQL, API");
        let triggers = vec!["open browser", "send email", "taurine", "POSTGRESQL"];

        let prompt = build_whisper_prompt(&dict, &triggers);
        assert!(prompt.is_some(), "Prompt must not be empty");
        let prompt_str = prompt.unwrap();

        // Must be comma-separated
        assert!(prompt_str.contains("Taurine, PostgreSQL, API"));
        assert!(prompt_str.contains("open browser"));
        assert!(prompt_str.contains("send email"));

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
    async fn test_missing_api_key_returns_config_error() {
        let client = GroqVoiceClient::new(None);
        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let result = client.transcribe_wav(&wav, None, None).await;

        if std::env::var("GROQ_API_KEY").is_err() {
            assert!(result.is_err());
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains("Groq API key not configured"),
                "Error was: {err}"
            );
        }
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
        let transcript = client.transcribe_wav(&wav, None, None).await.unwrap();

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
            .transcribe_wav(&wav, Some("Taurine, Whisper, Rust"), None)
            .await
            .unwrap();

        assert_eq!(transcript, "Taurine Whisper Rust test.");
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
        let result = client.transcribe_wav(&wav, None, None).await;

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
        let result = client.transcribe_wav(&wav, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("rate limit"));
        server_thread.join().unwrap();
    }

    #[test]
    fn test_read_env_file_key_parsing() {
        let env_content = r#"
# Comment line
GROQ_API_KEY="gsk_quoted_value_123"
OTHER_KEY='single_quoted'
UNQUOTED_KEY=unquoted_value_456
EMPTY_KEY=
        SPACED_KEY = spaced_value
"#;
        assert_eq!(
            read_env_file_key(env_content, "GROQ_API_KEY").as_deref(),
            Some("gsk_quoted_value_123")
        );
        assert_eq!(
            read_env_file_key(env_content, "OTHER_KEY").as_deref(),
            Some("single_quoted")
        );
        assert_eq!(
            read_env_file_key(env_content, "UNQUOTED_KEY").as_deref(),
            Some("unquoted_value_456")
        );
        assert_eq!(
            read_env_file_key(env_content, "SPACED_KEY").as_deref(),
            Some("spaced_value")
        );
        assert_eq!(read_env_file_key(env_content, "NON_EXISTENT"), None);
        assert_eq!(read_env_file_key(env_content, "EMPTY_KEY"), None);
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
    async fn test_clean_transcript_calls_llama_mock_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req = read_mock_request(&mut stream);

            assert!(req.starts_with("POST /chat/completions HTTP/1.1"));
            assert!(
                req.to_lowercase()
                    .contains("authorization: bearer test-llama-key")
            );
            assert!(req.contains("llama-3.1-8b-instant"));
            assert!(req.contains("um hello like this is a test"));
            assert!(req.contains("Taurine, Rust"));

            let response_body = r#"{
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "\"Hello, this is a test.\""
                    }
                }]
            }"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_full_endpoints(
            Some("test-llama-key".to_string()),
            "http://127.0.0.1:0/transcribe".to_string(),
            "whisper-large-v3".to_string(),
            format!("http://127.0.0.1:{port}/chat/completions"),
            "llama-3.1-8b-instant".to_string(),
        );

        let custom_terms = vec!["Taurine".to_string(), "Rust".to_string()];
        let cleaned = client
            .clean_transcript("um hello like this is a test", Some(&custom_terms))
            .await
            .unwrap();
        // Wrapper quotes stripped by sanitize_cleaned_text
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

        let client = GroqVoiceClient::with_full_endpoints(
            Some("test-llama-key".to_string()),
            "http://127.0.0.1:0/transcribe".to_string(),
            "whisper-large-v3".to_string(),
            format!("http://127.0.0.1:{port}/chat/completions"),
            "llama-3.1-8b-instant".to_string(),
        );

        let result = client
            .clean_transcript("raw text fallback", None)
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

            let response_body = r#"{
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": ""
                    }
                }]
            }"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let client = GroqVoiceClient::with_full_endpoints(
            Some("test-llama-key".to_string()),
            "http://127.0.0.1:0/transcribe".to_string(),
            "whisper-large-v3".to_string(),
            format!("http://127.0.0.1:{port}/chat/completions"),
            "llama-3.1-8b-instant".to_string(),
        );

        let cleaned = client.clean_transcript("um uh", None).await.unwrap();
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
            rt.block_on(async { client.transcribe_blocking(&[0.0; 160], 16000, None, None) });

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
            rt.block_on(async { client.transcribe_blocking(&[0.0; 160], 16000, None, None) });

        assert_eq!(result.unwrap(), "multithread blocking test");
        server_thread.join().unwrap();
    }
}
