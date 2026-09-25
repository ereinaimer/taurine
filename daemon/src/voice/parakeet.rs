use sherpa_onnx::{
    OfflineNemoEncDecCtcModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineTransducerModelConfig,
};
use std::path::{Path, PathBuf};
use taurine_core::error::Result;
use taurine_core::voice::{Transcriber, Transcription};

fn find_file_in_dir(dir: &Path, exact: &str, prefix: &str, ext: &str) -> Option<PathBuf> {
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

/// Speech-to-text transcriber using NeMo Parakeet models (Unified transducer or TDT-CTC 110M).
pub struct ParakeetTranscriber {
    model_name: String,
    recognizer: Option<OfflineRecognizer>,
}

impl ParakeetTranscriber {
    /// Initialize Parakeet recognizer with model directory.
    pub fn new(model_name: impl Into<String>, model_dir: Option<&Path>) -> Self {
        let raw = model_name.into();
        let canonical = taurine_core::voice::resolve_configured_model(&raw);
        // Store the canonical ID so `name()` reports the model actually loaded.
        let model_name = canonical.to_string();

        let recognizer = if let Some(dir) = model_dir
            && dir.exists()
        {
            if canonical == "parakeet-tdt-ctc-110m" {
                let model = find_file_in_dir(dir, "model.int8.onnx", "model", ".onnx");
                let tokens = find_file_in_dir(dir, "tokens.txt", "tokens", ".txt");

                if let (Some(model_path), Some(tokens_path)) = (model, tokens) {
                    let mut config = OfflineRecognizerConfig::default();
                    config.model_config.nemo_ctc = OfflineNemoEncDecCtcModelConfig {
                        model: Some(model_path.to_string_lossy().to_string()),
                    };
                    config.model_config.tokens = Some(tokens_path.to_string_lossy().to_string());
                    config.model_config.num_threads = 2;
                    OfflineRecognizer::create(&config)
                } else {
                    None
                }
            } else {
                let encoder = find_file_in_dir(dir, "encoder.int8.onnx", "encoder", ".onnx");
                let decoder = find_file_in_dir(dir, "decoder.int8.onnx", "decoder", ".onnx");
                let joiner = find_file_in_dir(dir, "joiner.int8.onnx", "joiner", ".onnx");
                let tokens = find_file_in_dir(dir, "tokens.txt", "tokens", ".txt");

                if let (Some(enc), Some(dec), Some(joi), Some(tok)) =
                    (encoder, decoder, joiner, tokens)
                {
                    let mut config = OfflineRecognizerConfig::default();
                    config.model_config.transducer = OfflineTransducerModelConfig {
                        encoder: Some(enc.to_string_lossy().to_string()),
                        decoder: Some(dec.to_string_lossy().to_string()),
                        joiner: Some(joi.to_string_lossy().to_string()),
                    };
                    config.model_config.tokens = Some(tok.to_string_lossy().to_string());
                    config.model_config.model_type = Some("nemo_transducer".into());
                    config.model_config.num_threads = 2;
                    OfflineRecognizer::create(&config)
                } else {
                    None
                }
            }
        } else {
            None
        };

        Self {
            model_name,
            recognizer,
        }
    }
}

impl Transcriber for ParakeetTranscriber {
    fn name(&self) -> &str {
        &self.model_name
    }

    fn transcribe(&mut self, audio: &[f32], sample_rate: u32) -> Result<Transcription> {
        let duration_secs = if sample_rate > 0 {
            audio.len() as f32 / sample_rate as f32
        } else {
            0.0
        };

        if let Some(ref recognizer) = self.recognizer {
            let stream = recognizer.create_stream();
            stream.accept_waveform(sample_rate as i32, audio);
            recognizer.decode(&stream);
            let result = stream.get_result();
            let text = result
                .as_ref()
                .map(|r| r.text.trim().to_string())
                .unwrap_or_default();
            let confidence = if text.is_empty() { 0.0 } else { 0.95 };
            Ok(Transcription::new(text, confidence, duration_secs))
        } else {
            Ok(Transcription::new("", 0.0, duration_secs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parakeet_transcriber_fallback_when_uninitialized() {
        let mut transcriber = ParakeetTranscriber::new("parakeet-unified-en-0.6b", None);
        assert_eq!(transcriber.name(), "parakeet-unified-en-0.6b");

        let silent_audio = vec![0.0f32; 16000];
        let res = transcriber
            .transcribe(&silent_audio, 16000)
            .expect("transcribe should succeed");
        assert!(res.is_empty());
        assert_eq!(res.duration_secs, 1.0);

        let transcriber_110m = ParakeetTranscriber::new("parakeet-tdt-ctc-110m", None);
        assert_eq!(transcriber_110m.name(), "parakeet-tdt-ctc-110m");
    }
}
