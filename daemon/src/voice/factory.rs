use std::path::Path;
use taurine_core::voice::Transcriber;

#[cfg(feature = "voice")]
use super::parakeet::ParakeetTranscriber;

/// Create a boxed Transcriber for exactly one configured voice model.
///
/// Only the resolved model is instantiated; the other model is never touched,
/// keeping idle voice RAM at zero until a PTT/Hands-Free session loads it.
///
/// Without the `voice` feature this returns a disabled stub that fails closed
/// on transcribe. Default builds enable `voice`, so shipped behavior is unchanged.
pub fn create_transcriber(model_name: &str, models_dir: Option<&Path>) -> Box<dyn Transcriber> {
    #[cfg(feature = "voice")]
    {
        let canonical = taurine_core::voice::resolve_configured_model(model_name);
        match canonical {
            "parakeet-unified-en-0.6b" => {
                let path = models_dir.map(|d| d.join("parakeet-unified"));
                Box::new(ParakeetTranscriber::new(canonical, path.as_deref()))
            }
            "parakeet-tdt-0.6b-v2" => {
                let path = models_dir.map(|d| d.join("parakeet-0.6b-v2"));
                Box::new(ParakeetTranscriber::new(canonical, path.as_deref()))
            }
            "parakeet-tdt-ctc-110m" => {
                let path = models_dir.map(|d| d.join("parakeet-110m"));
                Box::new(ParakeetTranscriber::new(canonical, path.as_deref()))
            }
            _ => {
                // `resolve_configured_model` only returns canonical catalog IDs;
                // fail closed to the light model rather than loading both.
                let path = models_dir.map(|d| d.join("parakeet-110m"));
                Box::new(ParakeetTranscriber::new(
                    "parakeet-tdt-ctc-110m",
                    path.as_deref(),
                ))
            }
        }
    }
    #[cfg(not(feature = "voice"))]
    {
        let _ = (model_name, models_dir);
        Box::new(DisabledTranscriber)
    }
}

/// Stub used when the `voice` ML stack is compiled out. Fails closed so no
/// caller can mistake a no-voice dev build for working transcription.
#[cfg(not(feature = "voice"))]
struct DisabledTranscriber;

#[cfg(not(feature = "voice"))]
impl Transcriber for DisabledTranscriber {
    fn name(&self) -> &str {
        "disabled-no-voice-feature"
    }

    fn transcribe(
        &mut self,
        _audio: &[f32],
        _sample_rate: u32,
    ) -> taurine_core::error::Result<taurine_core::voice::Transcription> {
        Err(taurine_core::error::Error::Service(
            "voice model disabled in this build (rebuild with default features)".to_string(),
        ))
    }
}

#[cfg(all(test, feature = "voice"))]
mod tests {
    use super::*;

    #[test]
    fn test_factory_creates_expected_transcribers() {
        let t1 = create_transcriber("auto", None);
        assert!(t1.name() == "parakeet-unified-en-0.6b" || t1.name() == "parakeet-tdt-ctc-110m");
        // Case-insensitive auto resolves through the same single path.
        let t2 = create_transcriber("AUTO", None);
        assert_eq!(t1.name(), t2.name());
        let t3 = create_transcriber("parakeet-unified-en-0.6b", None);
        assert_eq!(t3.name(), "parakeet-unified-en-0.6b");
        let t4 = create_transcriber("parakeet-tdt-ctc-110m", None);
        assert_eq!(t4.name(), "parakeet-tdt-ctc-110m");
        // Tier aliases resolve to their canonical models.
        assert_eq!(
            create_transcriber("best", None).name(),
            "parakeet-unified-en-0.6b"
        );
        assert_eq!(
            create_transcriber("balanced", None).name(),
            "parakeet-tdt-0.6b-v2"
        );
        assert_eq!(
            create_transcriber("fast", None).name(),
            "parakeet-tdt-ctc-110m"
        );
        let t5 = create_transcriber("parakeet-tdt-0.6b-v2", None);
        assert_eq!(t5.name(), "parakeet-tdt-0.6b-v2");
    }
}

#[cfg(all(test, not(feature = "voice")))]
mod tests {
    use super::*;

    #[test]
    fn test_factory_stub_fails_closed_without_voice() {
        let mut t = create_transcriber("auto", None);
        assert_eq!(t.name(), "disabled-no-voice-feature");
        assert!(t.transcribe(&[0.0; 1600], 16000).is_err());
    }
}
