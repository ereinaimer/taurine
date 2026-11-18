//! Groq Whisper Cloud Speech-to-Text inference client.

use crate::error::{Error, Result};
use crate::voice::VoiceDictionary;

pub const DEFAULT_GROQ_API_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
pub const DEFAULT_GROQ_MODEL: &str = "whisper-large-v3";

/// Compiles personal dictionary terms and active voice trigger phrases into a
/// comma-separated prompt string for Whisper acoustic and language biasing.
pub fn build_whisper_prompt(
    dict: &VoiceDictionary,
    trigger_phrases: &[impl AsRef<str>],
) -> Option<String> {
    const PROMPT_CAP: usize = 100;
    let mut collected: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Personal dictionary first (highest priority)
    for term in dict.terms() {
        let trimmed = term.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_lowercase()) {
            collected.push(trimmed.to_string());
            if collected.len() >= PROMPT_CAP {
                break;
            }
        }
    }

    // 2. Voice triggers second
    if collected.len() < PROMPT_CAP {
        for trigger in trigger_phrases {
            let trimmed = trigger.as_ref().trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_lowercase()) {
                collected.push(trimmed.to_string());
                if collected.len() >= PROMPT_CAP {
                    break;
                }
            }
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(collected.join(", "))
    }
}

/// Cloud speech transcription client communicating with Groq's Whisper API.
#[derive(Debug, Clone)]
pub struct GroqVoiceClient {
    client: reqwest::Client,
    api_key: Option<String>,
    endpoint_url: String,
    model: String,
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
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            api_key,
            endpoint_url,
            model,
        }
    }

    /// Resolves API key from environment (`GROQ_API_KEY`) or OS Keyring (`AiProvider::Groq`).
    pub fn from_env_or_keyring() -> Self {
        Self::new(None)
    }

    /// Resolves the effective API key from instance, environment, or OS keyring.
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

        let store = crate::ai::OsKeyringStore;
        if let Ok(Some(secret)) =
            crate::ai::CredentialStore::get_secret(&store, crate::ai::AiProvider::Groq)
        {
            let trimmed = secret.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        None
    }

    /// Transcribe 16-bit PCM WAV audio bytes.
    pub async fn transcribe_wav(&self, wav_bytes: &[u8], prompt: Option<&str>) -> Result<String> {
        let api_key = self.resolve_api_key().ok_or_else(|| {
            let diag = crate::diagnostic::Diagnostic::problem(
                "Groq API key not configured for voice dictation",
            )
            .help("Set the GROQ_API_KEY environment variable or run 'taurine ai --provider groq' to configure your API key")
            .example("taurine ai --provider groq --key gsk_...");
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

        Ok(parsed.text.trim().to_string())
    }

    /// Transcribe raw 16kHz mono audio samples asynchronously.
    pub async fn transcribe_samples(
        &self,
        samples: &[f32],
        sample_rate: u32,
        prompt: Option<&str>,
    ) -> Result<String> {
        let wav = crate::voice::encode_pcm16_wav(samples, sample_rate, 1);
        self.transcribe_wav(&wav, prompt).await
    }

    /// Synchronous transcription entry point for session manager threads.
    pub fn transcribe_blocking(
        &self,
        samples: &[f32],
        sample_rate: u32,
        prompt: Option<&str>,
    ) -> Result<String> {
        let wav = crate::voice::encode_pcm16_wav(samples, sample_rate, 1);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| handle.block_on(self.transcribe_wav(&wav, prompt)))
        } else {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt.block_on(self.transcribe_wav(&wav, prompt)),
                Err(e) => Err(Error::Service(format!(
                    "Failed to create async runtime: {e}"
                ))),
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
        let result = client.transcribe_wav(&wav, None).await;

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

        let client = GroqVoiceClient::with_endpoint(
            Some("test-groq-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        );

        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let transcript = client.transcribe_wav(&wav, None).await.unwrap();

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
        );

        let wav = crate::voice::encode_pcm16_wav(&[0.0; 160], 16000, 1);
        let transcript = client
            .transcribe_wav(&wav, Some("Taurine, Whisper, Rust"))
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
        let result = client.transcribe_wav(&wav, None).await;

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
        let result = client.transcribe_wav(&wav, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("rate limit"));
        server_thread.join().unwrap();
    }
}
