use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
use std::path::Path;
use taurine_core::error::Result;
use taurine_core::voice::{Transcriber, Transcription};

/// Speech-to-text transcriber using NeMo Parakeet-TDT (FastConformer transducer).
pub struct ParakeetTranscriber {
    recognizer: Option<OfflineRecognizer>,
}

impl ParakeetTranscriber {
    /// Initialize Parakeet recognizer with model directory containing:
    /// - encoder.int8.onnx
    /// - decoder.int8.onnx
    /// - joiner.int8.onnx
    /// - tokens.txt
    pub fn new(model_dir: Option<&Path>) -> Self {
        let recognizer = if let Some(dir) = model_dir
            && dir.exists()
        {
            let encoder = dir.join("encoder.int8.onnx");
            let decoder = dir.join("decoder.int8.onnx");
            let joiner = dir.join("joiner.int8.onnx");
            let tokens = dir.join("tokens.txt");

            if encoder.exists() && decoder.exists() && joiner.exists() && tokens.exists() {
                let mut config = OfflineRecognizerConfig::default();
                config.model_config.transducer = OfflineTransducerModelConfig {
                    encoder: Some(encoder.to_string_lossy().to_string()),
                    decoder: Some(decoder.to_string_lossy().to_string()),
                    joiner: Some(joiner.to_string_lossy().to_string()),
                };
                config.model_config.tokens = Some(tokens.to_string_lossy().to_string());
                config.model_config.model_type = Some("nemo_transducer".into());
                config.model_config.num_threads = 2;
                OfflineRecognizer::create(&config)
            } else {
                None
            }
        } else {
            None
        };

        Self { recognizer }
    }
}

impl Transcriber for ParakeetTranscriber {
    fn name(&self) -> &str {
        "parakeet-tdt-0.6b-v3"
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
            // Fallback when offline model weights are absent
            Ok(Transcription::new("", 0.0, duration_secs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parakeet_transcriber_fallback_when_uninitialized() {
        let mut transcriber = ParakeetTranscriber::new(None);
        assert_eq!(transcriber.name(), "parakeet-tdt-0.6b-v3");

        let silent_audio = vec![0.0f32; 16000];
        let res = transcriber
            .transcribe(&silent_audio, 16000)
            .expect("transcribe should succeed");
        assert!(res.is_empty());
        assert_eq!(res.duration_secs, 1.0);
    }
}
