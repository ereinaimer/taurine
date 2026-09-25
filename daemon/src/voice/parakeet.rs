use sherpa_onnx::{
    OfflineNemoEncDecCtcModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineTransducerModelConfig,
};
use std::path::{Path, PathBuf};
use taurine_core::error::Result;
use taurine_core::voice::{Transcriber, Transcription};
use tracing::debug;

/// Returns adaptive thread count for sherpa recognizers scaled with available cores (1..=4).
pub(crate) fn recognizer_threads() -> i32 {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 4) as i32;
    debug!("taurine-voice-parakeet: configuring recognizer with {threads} threads");
    threads
}

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

fn load_ctc_labels(path: &Path) -> Vec<String> {
    if let Ok(content) = std::fs::read_to_string(path) {
        let mut labels = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let tok = if let Some((tok, _)) = trimmed.rsplit_once(' ') {
                tok
            } else {
                trimmed
            };
            if tok == "<blk>" || tok == "<blank>" {
                labels.push(String::new());
            } else {
                labels.push(tok.to_string());
            }
        }
        labels
    } else {
        Vec::new()
    }
}

/// Decode CTC log-probabilities using `shenava-ctc-beam` with optional slash-separated hotwords.
pub fn decode_ctc_beam(
    decoder: &shenava_ctc_beam::CtcBeamDecoder,
    log_probs: &[Vec<f32>],
    hotwords: Option<&str>,
) -> String {
    let hw_list: Vec<String> = hotwords
        .map(|hw| {
            hw.split('/')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let hw = shenava_ctc_beam::Hotwords::new(hw_list, 10.0);
    decoder.decode(log_probs, &hw, 100, -5.0, -10.0)
}

/// Fallback resolution helper: if primary decoding yields empty text, use fallback transcript.
pub(crate) fn resolve_transcription_text(
    primary_text: &str,
    fallback_text: impl FnOnce() -> String,
    has_fallback: bool,
) -> String {
    if primary_text.trim().is_empty() && has_fallback {
        fallback_text()
    } else {
        primary_text.trim().to_string()
    }
}

/// Speech-to-text transcriber using NeMo Parakeet models across Fast, Balanced, and Best tiers.
pub struct ParakeetTranscriber {
    model_name: String,
    recognizer: Option<OfflineRecognizer>,
    fallback_recognizer: Option<OfflineRecognizer>,
    ctc_decoder: Option<shenava_ctc_beam::CtcBeamDecoder>,
    hotwords: Option<String>,
}

impl ParakeetTranscriber {
    /// Initialize Parakeet recognizer with model directory.
    pub fn new(model_name: impl Into<String>, model_dir: Option<&Path>) -> Self {
        let raw = model_name.into();
        let canonical = taurine_core::voice::resolve_configured_model(&raw);
        // Store the canonical ID so `name()` reports the model actually loaded.
        let model_name = canonical.to_string();

        let mut recognizer = None;
        let mut fallback_recognizer = None;
        let mut ctc_decoder = None;

        if let Some(dir) = model_dir
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
                    config.model_config.num_threads = recognizer_threads();
                    recognizer = OfflineRecognizer::create(&config);

                    let ctc_labels = load_ctc_labels(&tokens_path);
                    if !ctc_labels.is_empty() {
                        ctc_decoder = Some(shenava_ctc_beam::CtcBeamDecoder::new(ctc_labels));
                    }
                }
            } else if canonical == "parakeet-unified-en-0.6b" {
                // Best tier: ensure bpe.vocab exists or derive from tokens.txt
                let _ = taurine_core::voice::ensure_unified_hotwords_asset(Some(dir));
                let encoder = find_file_in_dir(dir, "encoder.int8.onnx", "encoder", ".onnx");
                let decoder = find_file_in_dir(dir, "decoder.int8.onnx", "decoder", ".onnx");
                let joiner = find_file_in_dir(dir, "joiner.int8.onnx", "joiner", ".onnx");
                let tokens = find_file_in_dir(dir, "tokens.txt", "tokens", ".txt");
                let bpe_vocab = find_file_in_dir(dir, "bpe.vocab", "bpe", ".vocab");

                if let (Some(enc), Some(dec), Some(joi), Some(tok)) =
                    (encoder, decoder, joiner, tokens)
                {
                    // Primary: modified_beam_search with provisioned bpe_vocab and hotwords_score = 2.0
                    let mut config = OfflineRecognizerConfig::default();
                    config.model_config.transducer = OfflineTransducerModelConfig {
                        encoder: Some(enc.to_string_lossy().to_string()),
                        decoder: Some(dec.to_string_lossy().to_string()),
                        joiner: Some(joi.to_string_lossy().to_string()),
                    };
                    config.model_config.tokens = Some(tok.to_string_lossy().to_string());
                    config.model_config.model_type = Some("nemo_transducer".into());
                    config.model_config.num_threads = recognizer_threads();
                    config.decoding_method = Some("modified_beam_search".into());
                    config.max_active_paths = 4;
                    config.model_config.modeling_unit = Some("bpe".into());
                    if let Some(ref bpe) = bpe_vocab {
                        config.model_config.bpe_vocab = Some(bpe.to_string_lossy().to_string());
                    }
                    config.hotwords_score = 2.0;
                    recognizer = OfflineRecognizer::create(&config);

                    // Fallback: greedy search with zero hotwords score (Fix 3)
                    let mut fallback_config = OfflineRecognizerConfig::default();
                    fallback_config.model_config.transducer = OfflineTransducerModelConfig {
                        encoder: Some(enc.to_string_lossy().to_string()),
                        decoder: Some(dec.to_string_lossy().to_string()),
                        joiner: Some(joi.to_string_lossy().to_string()),
                    };
                    fallback_config.model_config.tokens = Some(tok.to_string_lossy().to_string());
                    fallback_config.model_config.model_type = Some("nemo_transducer".into());
                    fallback_config.model_config.num_threads = recognizer_threads();
                    fallback_config.decoding_method = Some("greedy_search".into());
                    fallback_config.hotwords_score = 0.0;
                    fallback_recognizer = OfflineRecognizer::create(&fallback_config);
                }
            } else {
                // Balanced tier: parakeet-tdt-0.6b-v2 or other transducer models
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
                    config.model_config.num_threads = recognizer_threads();
                    recognizer = OfflineRecognizer::create(&config);

                    let ctc_labels = load_ctc_labels(&tok);
                    if !ctc_labels.is_empty() {
                        ctc_decoder = Some(shenava_ctc_beam::CtcBeamDecoder::new(ctc_labels));
                    }
                }
            }
        }

        Self {
            model_name,
            recognizer,
            fallback_recognizer,
            ctc_decoder,
            hotwords: None,
        }
    }

    /// Decode raw CTC log-probabilities using the model's CTC beam decoder.
    pub fn decode_ctc_log_probs(
        &self,
        log_probs: &[Vec<f32>],
        hotwords: Option<&str>,
    ) -> Option<String> {
        let decoder = self.ctc_decoder.as_ref()?;
        Some(decode_ctc_beam(decoder, log_probs, hotwords))
    }
}

impl Transcriber for ParakeetTranscriber {
    fn name(&self) -> &str {
        &self.model_name
    }

    fn set_hotwords(&mut self, hotwords: Option<String>) {
        self.hotwords = hotwords;
    }

    fn transcribe(&mut self, audio: &[f32], sample_rate: u32) -> Result<Transcription> {
        let duration_secs = if sample_rate > 0 {
            audio.len() as f32 / sample_rate as f32
        } else {
            0.0
        };

        if let Some(ref recognizer) = self.recognizer {
            let stream = if let Some(ref hw) = self.hotwords
                && !hw.trim().is_empty()
                && self.model_name == "parakeet-unified-en-0.6b"
            {
                recognizer.create_stream_with_hotwords(hw.trim())
            } else {
                recognizer.create_stream()
            };
            stream.accept_waveform(sample_rate as i32, audio);
            recognizer.decode(&stream);
            let result = stream.get_result();
            let text = result
                .as_ref()
                .map(|r| r.text.trim().to_string())
                .unwrap_or_default();

            // Fix 3: Dual-pass fallback for Unified model.
            // If modified_beam_search yields an empty transcript, immediately
            // fall back to greedy decode so the user never gets dropped output.
            let text = if let Some(ref fallback) = self.fallback_recognizer
                && !audio.is_empty()
            {
                resolve_transcription_text(
                    &text,
                    || {
                        debug!(
                            "taurine-voice-parakeet: beam search returned empty text, trying fallback"
                        );
                        let fallback_stream = fallback.create_stream();
                        fallback_stream.accept_waveform(sample_rate as i32, audio);
                        fallback.decode(&fallback_stream);
                        fallback_stream
                            .get_result()
                            .as_ref()
                            .map(|r| r.text.trim().to_string())
                            .unwrap_or_default()
                    },
                    true,
                )
            } else {
                text
            };

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

        // Aliases resolve to the canonical loaded model, never the raw input.
        let transcriber_auto = ParakeetTranscriber::new("auto", None);
        assert_eq!(
            transcriber_auto.name(),
            taurine_core::voice::resolve_auto_model()
        );
    }

    #[test]
    fn test_parakeet_transcriber_hotwords_setter() {
        let mut transcriber = ParakeetTranscriber::new("parakeet-unified-en-0.6b", None);
        assert!(transcriber.hotwords.is_none());
        transcriber.set_hotwords(Some("movies folder/Taurine".to_string()));
        assert_eq!(
            transcriber.hotwords.as_deref(),
            Some("movies folder/Taurine")
        );
        transcriber.set_hotwords(None);
        assert!(transcriber.hotwords.is_none());
    }

    #[test]
    fn test_recognizer_threads_scaling() {
        let threads = recognizer_threads();
        assert!((1..=4).contains(&threads));
        if let Ok(p) = std::thread::available_parallelism()
            && p.get() >= 2
        {
            assert!(threads >= 2);
        }
    }

    #[test]
    fn test_shenava_ctc_beam_boosts_ambiguous_phrase() {
        let labels = vec![
            "▁movies".to_string(),
            "▁move".to_string(),
            "▁is".to_string(),
            "▁folder".to_string(),
            "".to_string(),
        ];
        let decoder = shenava_ctc_beam::CtcBeamDecoder::new(labels);
        // Frame 0: "move" slightly favored over "movies" (-0.2 vs -0.8)
        // Frame 1: blank
        // Frame 2: "folder" (-0.2)
        let log_probs = vec![
            vec![-0.8, -0.2, -20.0, -20.0, -20.0],
            vec![-20.0, -20.0, -20.0, -20.0, -0.1],
            vec![-20.0, -20.0, -20.0, -0.2, -20.0],
        ];
        let unboosted = decode_ctc_beam(&decoder, &log_probs, None);
        assert_eq!(unboosted, "move folder");

        let boosted = decode_ctc_beam(&decoder, &log_probs, Some("movies folder/Taurine"));
        assert_eq!(boosted, "movies folder");
    }

    #[test]
    fn test_dual_pass_fallback_resolution() {
        let resolved_empty = resolve_transcription_text("", || "fallback text".to_string(), true);
        assert_eq!(resolved_empty, "fallback text");

        let resolved_present =
            resolve_transcription_text("primary text", || panic!("should not be called"), true);
        assert_eq!(resolved_present, "primary text");

        let resolved_no_fallback = resolve_transcription_text("", || panic!("no fallback"), false);
        assert_eq!(resolved_no_fallback, "");
    }
}
