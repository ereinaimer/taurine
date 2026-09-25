use crate::error::Result;

/// Output of a speech transcription engine.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcription {
    /// Transcribed text.
    pub text: String,
    /// Confidence score from 0.0 to 1.0.
    pub confidence: f32,
    /// Duration of the audio segment in seconds.
    pub duration_secs: f32,
}

impl Transcription {
    /// Create a new transcription result with clamped confidence.
    pub fn new(text: impl Into<String>, confidence: f32, duration_secs: f32) -> Self {
        Self {
            text: text.into(),
            confidence: confidence.clamp(0.0, 1.0),
            duration_secs,
        }
    }

    /// Check if the transcribed text is blank.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Abstract interface implemented by all Taurine speech-to-text inference engines.
pub trait Transcriber: Send + Sync {
    /// Identifier name for the transcriber engine.
    fn name(&self) -> &str;

    /// Set optional hotwords payload to bias the upcoming transcription.
    fn set_hotwords(&mut self, _hotwords: Option<String>) {}

    /// Transcribe 16kHz mono audio samples into text.
    fn transcribe(&mut self, audio: &[f32], sample_rate: u32) -> Result<Transcription>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTranscriber {
        hotwords: Option<String>,
    }

    impl Transcriber for MockTranscriber {
        fn name(&self) -> &str {
            "mock"
        }

        fn set_hotwords(&mut self, hotwords: Option<String>) {
            self.hotwords = hotwords;
        }

        fn transcribe(&mut self, _audio: &[f32], _sample_rate: u32) -> Result<Transcription> {
            let text = if let Some(ref hw) = self.hotwords {
                format!("hello {hw}")
            } else {
                "hello world".to_string()
            };
            Ok(Transcription::new(text, 0.95, 1.2))
        }
    }

    #[test]
    fn test_transcription_creation_and_clamping() {
        let t = Transcription::new("  testing  ", 1.5, 0.5);
        assert_eq!(t.text, "  testing  ");
        assert_eq!(t.confidence, 1.0);
        assert!(!t.is_empty());

        let empty = Transcription::new("   ", -0.2, 0.0);
        assert!(empty.is_empty());
        assert_eq!(empty.confidence, 0.0);
    }

    #[test]
    fn test_mock_transcriber() {
        let mut mock = MockTranscriber { hotwords: None };
        assert_eq!(mock.name(), "mock");
        let result = mock.transcribe(&[0.0; 1600], 16000).expect("transcription");
        assert_eq!(result.text, "hello world");
        assert_eq!(result.confidence, 0.95);

        mock.set_hotwords(Some("taurine".to_string()));
        let res_hw = mock
            .transcribe(&[0.0; 1600], 16000)
            .expect("transcription with hw");
        assert_eq!(res_hw.text, "hello taurine");
    }
}
