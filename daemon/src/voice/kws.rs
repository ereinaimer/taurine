use sherpa_onnx::{KeywordSpotter, KeywordSpotterConfig};
use std::path::Path;

/// Streaming keyword spotter powered by Zipformer KWS.
pub struct Spotter {
    spotter: Option<KeywordSpotter>,
}

impl Spotter {
    /// Initialize Keyword Spotter with model directory and in-memory keywords string.
    pub fn new(model_dir: Option<&Path>, keywords: Option<&str>) -> Self {
        let spotter = if let Some(dir) = model_dir
            && dir.exists()
        {
            let encoder = dir.join("encoder.int8.onnx");
            let decoder = dir.join("decoder.int8.onnx");
            let joiner = dir.join("joiner.int8.onnx");
            let tokens = dir.join("tokens.txt");

            if encoder.exists() && decoder.exists() && joiner.exists() && tokens.exists() {
                let mut config = KeywordSpotterConfig::default();
                config.model_config.transducer.encoder =
                    Some(encoder.to_string_lossy().to_string());
                config.model_config.transducer.decoder =
                    Some(decoder.to_string_lossy().to_string());
                config.model_config.transducer.joiner = Some(joiner.to_string_lossy().to_string());
                config.model_config.tokens = Some(tokens.to_string_lossy().to_string());
                config.model_config.num_threads = 1;

                if let Some(kw) = keywords {
                    config.keywords_buf = Some(kw.to_string());
                }

                KeywordSpotter::create(&config)
            } else {
                None
            }
        } else {
            None
        };

        Self { spotter }
    }

    /// Check if local spotter model is loaded and ready.
    pub fn is_ready(&self) -> bool {
        self.spotter.is_some()
    }

    /// Process 16kHz audio samples and return detected keyword if spotted.
    pub fn detect(&self, samples: &[f32], sample_rate: u32) -> Option<String> {
        if let Some(ref spotter) = self.spotter {
            let stream = spotter.create_stream();
            stream.accept_waveform(sample_rate as i32, samples);
            while spotter.is_ready(&stream) {
                spotter.decode(&stream);
            }
            let result = spotter.get_result(&stream);
            if let Some(r) = result
                && !r.keyword.trim().is_empty()
            {
                return Some(r.keyword.trim().to_string());
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
}
