use sherpa_onnx::{OfflineMoonshineModelConfig, OfflineRecognizer, OfflineRecognizerConfig};
use std::path::Path;
use taurine_core::error::Result;
use taurine_core::voice::{Transcriber, Transcription};

/// Speech-to-text transcriber using Moonshine.
pub struct MoonshineTranscriber {
    recognizer: Option<OfflineRecognizer>,
}

impl MoonshineTranscriber {
    /// Initialize Moonshine recognizer with model directory containing:
    /// - encoder.int8.onnx (or encoder.onnx)
    /// - decoder.int8.onnx (or merged_decoder.int8.onnx)
    /// - tokens.txt
    pub fn new(model_dir: Option<&Path>) -> Self {
        let recognizer = if let Some(dir) = model_dir
            && dir.exists()
        {
            let enc_candidates = ["encoder.int8.onnx", "encoder.onnx"];
            let dec_candidates = [
                "decoder.int8.onnx",
                "decoder.onnx",
                "merged_decoder.int8.onnx",
                "merged_decoder.onnx",
            ];

            let encoder = enc_candidates
                .iter()
                .map(|f| dir.join(f))
                .find(|p| p.exists());
            let decoder = dec_candidates
                .iter()
                .map(|f| dir.join(f))
                .find(|p| p.exists());
            let tokens = dir.join("tokens.txt");

            if let (Some(enc), Some(dec)) = (encoder, decoder)
                && tokens.exists()
            {
                let mut config = OfflineRecognizerConfig::default();
                config.model_config.moonshine = OfflineMoonshineModelConfig {
                    encoder: Some(enc.to_string_lossy().to_string()),
                    merged_decoder: Some(dec.to_string_lossy().to_string()),
                    ..Default::default()
                };
                config.model_config.tokens = Some(tokens.to_string_lossy().to_string());
                config.model_config.model_type = Some("moonshine".into());
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

impl Transcriber for MoonshineTranscriber {
    fn name(&self) -> &str {
        "moonshine-base-en"
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
            let confidence = if text.is_empty() { 0.0 } else { 0.90 };
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
    fn test_moonshine_transcriber_fallback_when_uninitialized() {
        let mut transcriber = MoonshineTranscriber::new(None);
        assert_eq!(transcriber.name(), "moonshine-base-en");

        let silent_audio = vec![0.0f32; 8000];
        let res = transcriber
            .transcribe(&silent_audio, 16000)
            .expect("transcribe should succeed");
        assert!(res.is_empty());
        assert_eq!(res.duration_secs, 0.5);
    }
}
