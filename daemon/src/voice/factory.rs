use super::parakeet::ParakeetTranscriber;
use std::path::Path;
use taurine_core::voice::Transcriber;

/// Create a boxed Transcriber for exactly one configured voice model.
///
/// Only the resolved model is instantiated; the other model is never touched,
/// keeping idle voice RAM at zero until a PTT/Hands-Free session loads it.
pub fn create_transcriber(model_name: &str, models_dir: Option<&Path>) -> Box<dyn Transcriber> {
    let canonical = taurine_core::voice::resolve_configured_model(model_name);
    match canonical {
        "parakeet-unified-en-0.6b" => {
            let path = models_dir.map(|d| d.join("parakeet-unified"));
            Box::new(ParakeetTranscriber::new(canonical, path.as_deref()))
        }
        "parakeet-tdt-ctc-110m" => {
            let path = models_dir.map(|d| d.join("parakeet-110m"));
            Box::new(ParakeetTranscriber::new(canonical, path.as_deref()))
        }
        _ => {
            // `resolve_configured_model` only returns the two canonical IDs;
            // fail closed to the light model rather than loading both.
            let path = models_dir.map(|d| d.join("parakeet-110m"));
            Box::new(ParakeetTranscriber::new(
                "parakeet-tdt-ctc-110m",
                path.as_deref(),
            ))
        }
    }
}

#[cfg(test)]
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
    }
}
