use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineWhisperModelConfig};
use std::path::Path;
use taurine_core::error::Result;
use taurine_core::voice::{Transcriber, Transcription};

/// Speech-to-text transcriber using Whisper variants (Large-v3-Turbo, Distil-Whisper, Base, Small, Medium).
pub struct WhisperTranscriber {
    model_name: String,
    recognizer: Option<OfflineRecognizer>,
}

impl WhisperTranscriber {
    /// Initialize Whisper recognizer with model directory containing:
    /// - encoder.int8.onnx (or tiny.en-encoder.int8.onnx, etc.)
    /// - decoder.int8.onnx (or tiny.en-decoder.int8.onnx, etc.)
    /// - tokens.txt
    pub fn new(model_name: impl Into<String>, model_dir: Option<&Path>) -> Self {
        let model_name = model_name.into();
        let recognizer = if let Some(dir) = model_dir
            && dir.exists()
        {
            let enc_candidates = [
                "encoder.int8.onnx",
                "encoder.onnx",
                "tiny.en-encoder.int8.onnx",
                "base.en-encoder.int8.onnx",
                "small.en-encoder.int8.onnx",
                "medium.en-encoder.int8.onnx",
            ];
            let dec_candidates = [
                "decoder.int8.onnx",
                "decoder.onnx",
                "tiny.en-decoder.int8.onnx",
                "base.en-decoder.int8.onnx",
                "small.en-decoder.int8.onnx",
                "medium.en-decoder.int8.onnx",
            ];

            let enc_path = enc_candidates
                .iter()
                .map(|f| dir.join(f))
                .find(|p| p.exists());
            let dec_path = dec_candidates
                .iter()
                .map(|f| dir.join(f))
                .find(|p| p.exists());
            let tokens_path = ["tokens.txt"]
                .iter()
                .map(|f| dir.join(f))
                .find(|p| p.exists());

            if let (Some(enc), Some(dec), Some(tok)) = (enc_path, dec_path, tokens_path) {
                let mut config = OfflineRecognizerConfig::default();
                config.model_config.whisper = OfflineWhisperModelConfig {
                    encoder: Some(enc.to_string_lossy().to_string()),
                    decoder: Some(dec.to_string_lossy().to_string()),
                    language: Some("en".into()),
                    task: Some("transcribe".into()),
                    tail_paddings: 0,
                    enable_token_timestamps: true,
                    enable_segment_timestamps: true,
                };
                config.model_config.tokens = Some(tok.to_string_lossy().to_string());
                config.model_config.model_type = Some("whisper".into());
                config.model_config.num_threads = 2;
                OfflineRecognizer::create(&config)
            } else {
                None
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

impl Transcriber for WhisperTranscriber {
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
            let confidence = if text.is_empty() { 0.0 } else { 0.92 };
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
    fn test_whisper_transcriber_fallback_when_uninitialized() {
        let mut transcriber = WhisperTranscriber::new("whisper-large-v3-turbo", None);
        assert_eq!(transcriber.name(), "whisper-large-v3-turbo");

        let silent_audio = vec![0.0f32; 16000];
        let res = transcriber
            .transcribe(&silent_audio, 16000)
            .expect("transcribe should succeed");
        assert!(res.is_empty());
        assert_eq!(res.duration_secs, 1.0);
    }
}
