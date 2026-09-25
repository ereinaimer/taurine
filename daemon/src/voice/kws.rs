use sherpa_onnx::{KeywordSpotter, KeywordSpotterConfig, OnlineStream};
use std::path::Path;

/// Streaming keyword spotter powered by Zipformer KWS.
pub struct Spotter {
    spotter: Option<KeywordSpotter>,
    stream: Option<OnlineStream>,
}

fn find_file_in_dir(
    dir: &Path,
    exact: &str,
    prefix: &str,
    ext: &str,
) -> Option<std::path::PathBuf> {
    let exact_path = dir.join(exact);
    if exact_path.is_file() {
        return Some(exact_path);
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str())
                && file_name.starts_with(prefix)
                && file_name.ends_with(ext)
            {
                return Some(path);
            }
        }
    }
    None
}

fn load_lexicon(lexicon_path: &Path) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string(lexicon_path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((word, phones)) = trimmed.split_once(' ') {
                let word_upper = word.trim().to_uppercase();
                let phones_clean = phones.trim().to_string();
                map.entry(word_upper).or_insert(phones_clean);
            }
        }
    }
    map
}

fn phrase_to_phonemes(
    phrase: &str,
    lexicon: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let mut phone_parts = Vec::new();
    for word in phrase.split_whitespace() {
        let clean_word = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_uppercase();
        if clean_word.is_empty() {
            continue;
        }
        let phones = lexicon.get(&clean_word)?;
        phone_parts.push(phones.as_str());
    }
    if phone_parts.is_empty() {
        None
    } else {
        Some(phone_parts.join(" "))
    }
}

fn load_token_set(tokens_path: &Path) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    if let Ok(content) = std::fs::read_to_string(tokens_path) {
        for line in content.lines() {
            if let Some(token) = line.split_whitespace().next() {
                set.insert(token.to_string());
            }
        }
    }
    set
}

impl Spotter {
    /// Initialize Keyword Spotter with model directory and in-memory keywords string.
    pub fn new(model_dir: Option<&Path>, keywords: Option<&str>) -> Self {
        let spotter = if let Some(dir) = model_dir
            && dir.exists()
        {
            let encoder = find_file_in_dir(dir, "encoder.int8.onnx", "encoder", ".onnx");
            let decoder = find_file_in_dir(dir, "decoder.int8.onnx", "decoder", ".onnx");
            let joiner = find_file_in_dir(dir, "joiner.int8.onnx", "joiner", ".onnx");
            let tokens = find_file_in_dir(dir, "tokens.txt", "tokens", ".txt");

            if let (Some(encoder), Some(decoder), Some(joiner), Some(tokens)) =
                (encoder, decoder, joiner, tokens)
            {
                let mut config = KeywordSpotterConfig::default();
                config.model_config.transducer.encoder =
                    Some(encoder.to_string_lossy().to_string());
                config.model_config.transducer.decoder =
                    Some(decoder.to_string_lossy().to_string());
                config.model_config.transducer.joiner = Some(joiner.to_string_lossy().to_string());
                config.model_config.tokens = Some(tokens.to_string_lossy().to_string());
                config.model_config.num_threads = 1;

                if let Some(kw) = keywords {
                    let phone_file = dir.join("en.phone");
                    let formatted_keywords = if phone_file.is_file() {
                        let lexicon = load_lexicon(&phone_file);
                        let token_set = load_token_set(&tokens);
                        let mut lines = Vec::new();
                        for line in kw.lines() {
                            let phrase = line.trim();
                            if phrase.is_empty() {
                                continue;
                            }
                            if phrase.contains('@') {
                                lines.push(phrase.to_string());
                            } else if let Some(phones) = phrase_to_phonemes(phrase, &lexicon) {
                                if phones.split_whitespace().all(|p| token_set.contains(p)) {
                                    let label = phrase.replace(' ', "_");
                                    lines.push(format!("{phones} @{label}"));
                                } else {
                                    tracing::warn!(
                                        "Ambient keyword '{}' contains phonemes not in tokens.txt; omitting",
                                        phrase
                                    );
                                }
                            } else {
                                tracing::warn!(
                                    "Ambient keyword '{}' contains words not in pronunciation lexicon; omitting",
                                    phrase
                                );
                            }
                        }
                        lines.join("\n")
                    } else {
                        kw.to_string()
                    };

                    if !formatted_keywords.trim().is_empty() {
                        config.keywords_buf = Some(formatted_keywords);
                    }
                }

                if config.keywords_buf.is_some() {
                    KeywordSpotter::create(&config)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let stream = spotter.as_ref().map(|s| s.create_stream());
        Self { spotter, stream }
    }

    /// Check if local spotter model is loaded and ready.
    pub fn is_ready(&self) -> bool {
        self.spotter.is_some()
    }

    /// Process 16kHz audio samples and return detected keyword if spotted.
    pub fn detect(&mut self, samples: &[f32], sample_rate: u32) -> Option<String> {
        if let Some(ref spotter) = self.spotter
            && let Some(ref mut stream) = self.stream
        {
            stream.accept_waveform(sample_rate as i32, samples);
            while spotter.is_ready(stream) {
                spotter.decode(stream);
            }
            let result = spotter.get_result(stream);
            if let Some(r) = result
                && !r.keyword.trim().is_empty()
            {
                // Reset persistent stream on keyword hit so trailing audio carries over
                spotter.reset(stream);
                let clean_kw = r
                    .keyword
                    .trim()
                    .trim_start_matches('@')
                    .replace('_', " ")
                    .trim()
                    .to_string();
                return Some(clean_kw);
            }
        }
        None
    }
}

// SAFETY: KeywordSpotter is protected by internal C++ reference counts and can safely be
// transferred across thread boundaries within Taurine's daemon voice pipeline.
unsafe impl Send for Spotter {}

// SAFETY: All calls to `Spotter` methods operate through synchronized Mutex wrappers,
// ensuring thread-safe shared references.
unsafe impl Sync for Spotter {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spotter_fallback_when_uninitialized() {
        let spotter = Spotter::new(None, Some("hello\nworld\n"));
        assert!(!spotter.is_ready());
    }

    #[test]
    fn test_spotter_stream_persists_across_frames() {
        // Regression test for the 32ms stream recreation bug: feeding sequential
        // 512-sample frames must reuse the persistent stream without panicking,
        // even when no model is loaded.
        let mut spotter = Spotter::new(None, Some("hello\nworld\n"));
        for _ in 0..4 {
            assert_eq!(spotter.detect(&[0.1f32; 512], 16000), None);
        }
        assert!(!spotter.is_ready());
    }

    #[test]
    fn test_phrase_to_phonemes() {
        let mut lexicon = std::collections::HashMap::new();
        lexicon.insert("TYPE".to_string(), "T AY1 P".to_string());
        lexicon.insert("THIS".to_string(), "DH IH1 S".to_string());

        let phones = phrase_to_phonemes("type this", &lexicon);
        assert_eq!(phones.as_deref(), Some("T AY1 P DH IH1 S"));

        let unknown = phrase_to_phonemes("type unknown_word", &lexicon);
        assert_eq!(unknown, None);
    }
}
