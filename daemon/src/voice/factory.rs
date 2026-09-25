use super::moonshine::MoonshineTranscriber;
use super::parakeet::ParakeetTranscriber;
use super::whisper::WhisperTranscriber;
use std::path::Path;
use taurine_core::voice::Transcriber;

/// Create a boxed Transcriber implementation based on configured voice model identifier.
pub fn create_transcriber(model_name: &str, models_dir: Option<&Path>) -> Box<dyn Transcriber> {
    let lower = model_name.trim().to_ascii_lowercase();

    match lower.as_str() {
        "parakeet" | "parakeet-tdt" | "parakeet-tdt-0.6b-v3" | "auto" => {
            let path = models_dir.map(|d| d.join("parakeet"));
            Box::new(ParakeetTranscriber::new(path.as_deref()))
        }
        "moonshine" | "moonshine-base" | "moonshine-base-en" => {
            let path = models_dir.map(|d| d.join("moonshine"));
            Box::new(MoonshineTranscriber::new(path.as_deref()))
        }
        _ => {
            let path = models_dir.map(|d| d.join(&lower));
            Box::new(WhisperTranscriber::new(lower, path.as_deref()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_creates_expected_transcribers() {
        let t1 = create_transcriber("auto", None);
        assert_eq!(t1.name(), "parakeet-tdt-0.6b-v3");

        let t2 = create_transcriber("moonshine", None);
        assert_eq!(t2.name(), "moonshine-base-en");

        let t3 = create_transcriber("whisper-small-en", None);
        assert_eq!(t3.name(), "whisper-small-en");
    }
}
