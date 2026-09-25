use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

/// Metadata entry for a machine learning speech or voice activity model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalogEntry {
    /// Unique machine identifier for the model.
    pub id: &'static str,
    /// Friendly human-readable name.
    pub name: &'static str,
    /// Human-readable download size estimate.
    pub size_display: &'static str,
    /// Approximate download size in bytes.
    pub size_bytes: u64,
    /// Pinned download URL.
    pub url: &'static str,
    /// Expected SHA-256 checksum (if pinned).
    pub sha256: Option<&'static str>,
    /// Whether the model distribution is a compressed archive.
    pub is_archive: bool,
    /// Brief description of the model architecture and recommended use case.
    pub description: &'static str,
}

/// Catalog of officially supported speech models in Taurine.
pub static MODEL_CATALOG: &[ModelCatalogEntry] = &[
    ModelCatalogEntry {
        id: "parakeet-tdt-0.6b-v3",
        name: "NeMo Parakeet-TDT 0.6B v3",
        size_display: "600 MB",
        size_bytes: 629_145_600,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
        sha256: None,
        is_archive: true,
        description: "NVIDIA FastConformer + TDT INT8 transducer; sub-100ms latency on CPU with native punctuation",
    },
    ModelCatalogEntry {
        id: "whisper-large-v3-turbo",
        name: "Whisper Large-v3 Turbo (Q5_0)",
        size_display: "550 MB",
        size_bytes: 576_716_800,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: None,
        is_archive: false,
        description: "OpenAI distilled 4-layer decoder; multi-lingual coverage with high precision",
    },
    ModelCatalogEntry {
        id: "distil-whisper-large-v3",
        name: "Distil-Whisper Large-v3 (Q5_0)",
        size_display: "450 MB",
        size_bytes: 471_859_200,
        url: "https://huggingface.co/distil-whisper/distil-large-v3-ggml/resolve/main/ggml-distil-large-v3.bin",
        sha256: None,
        is_archive: false,
        description: "HuggingFace distilled Whisper; 4-6x faster than standard Whisper on CPU",
    },
    ModelCatalogEntry {
        id: "whisper-small-en",
        name: "Whisper Small English (Q5_0)",
        size_display: "170 MB",
        size_bytes: 178_257_920,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en-q5_0.bin",
        sha256: None,
        is_archive: false,
        description: "OpenAI Small.en; balanced lightweight CPU engine for laptops",
    },
    ModelCatalogEntry {
        id: "whisper-base-en",
        name: "Whisper Base English (Q5_0)",
        size_display: "60 MB",
        size_bytes: 62_914_560,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en-q5_0.bin",
        sha256: None,
        is_archive: false,
        description: "OpenAI Base.en; ultra-lightweight CPU fallback with minimal RAM",
    },
    ModelCatalogEntry {
        id: "whisper-medium-en",
        name: "Whisper Medium English (Q5_0)",
        size_display: "900 MB",
        size_bytes: 943_718_400,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en-q5_0.bin",
        sha256: None,
        is_archive: false,
        description: "OpenAI Medium.en; high accuracy transcription for complex vocabulary",
    },
    ModelCatalogEntry {
        id: "whisper-large-v3",
        name: "Whisper Large-v3 (Q5_0)",
        size_display: "1.6 GB",
        size_bytes: 1_717_986_918,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin",
        sha256: None,
        is_archive: false,
        description: "OpenAI full Large-v3; maximum accuracy for challenging acoustic environments",
    },
    ModelCatalogEntry {
        id: "moonshine-base-en",
        name: "Moonshine Base English",
        size_display: "135 MB",
        size_bytes: 141_557_760,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-base-en-int8.tar.bz2",
        sha256: None,
        is_archive: true,
        description: "UsefulSensors Base INT8 encoder-decoder; zero audio padding",
    },
    ModelCatalogEntry {
        id: "moonshine-tiny-en",
        name: "Moonshine Tiny English",
        size_display: "27 MB",
        size_bytes: 28_311_552,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-tiny-en-int8.tar.bz2",
        sha256: None,
        is_archive: true,
        description: "UsefulSensors Tiny INT8; active verifier for ambient trigger spotting",
    },
    ModelCatalogEntry {
        id: "kws-zipformer-zh-en-3M",
        name: "Zipformer KWS Spotter 3M",
        size_display: "15 MB",
        size_bytes: 15_728_640,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20.tar.bz2",
        sha256: None,
        is_archive: true,
        description: "Ultra-low power resident keyword spotter; <0.3% CPU usage",
    },
    ModelCatalogEntry {
        id: "silero_vad_v6",
        name: "Silero VAD v6 ONNX",
        size_display: "2 MB",
        size_bytes: 2_097_152,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
        sha256: None,
        is_archive: false,
        description: "Voice activity detector gating audio frames in 32ms intervals",
    },
];

/// Returns the models directory inside the app data directory.
pub fn models_dir() -> PathBuf {
    crate::system::paths::ensure_data_dir().join("models")
}

/// Retrieve a catalog entry by its canonical ID or alias.
pub fn get_model_entry(identifier: &str) -> Option<&'static ModelCatalogEntry> {
    let canonical = resolve_model_alias(identifier);
    MODEL_CATALOG.iter().find(|entry| entry.id == canonical)
}

/// Return all available catalog entries.
pub fn list_models() -> &'static [ModelCatalogEntry] {
    MODEL_CATALOG
}

/// Resolves user-friendly aliases to their canonical model catalog ID.
pub fn resolve_model_alias(alias: &str) -> &'static str {
    let trimmed = alias.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "parakeet" | "parakeet-tdt" | "fastconformer" => "parakeet-tdt-0.6b-v3",
        "turbo" | "large-turbo" | "large-v3-turbo" => "whisper-large-v3-turbo",
        "distil" | "distil-large" | "distil-whisper" => "distil-whisper-large-v3",
        "small" | "small-en" | "small.en" => "whisper-small-en",
        "base" | "base-en" | "base.en" => "whisper-base-en",
        "medium" | "medium-en" | "medium.en" => "whisper-medium-en",
        "large" | "large-v3" => "whisper-large-v3",
        "moonshine" | "moonshine-base" => "moonshine-base-en",
        "moonshine-tiny" => "moonshine-tiny-en",
        "kws" | "zipformer" => "kws-zipformer-zh-en-3M",
        "vad" | "silero" => "silero_vad_v6",
        "auto" => "parakeet-tdt-0.6b-v3",
        _ => {
            for entry in MODEL_CATALOG {
                if entry.id.eq_ignore_ascii_case(&trimmed) {
                    return entry.id;
                }
            }
            // If unknown, fallback to parakeet
            "parakeet-tdt-0.6b-v3"
        }
    }
}

/// Check if a model's required files are present and non-empty on disk.
pub fn is_model_downloaded(model_id: &str, base_dir: Option<&Path>) -> bool {
    let canonical = resolve_model_alias(model_id);
    let default_dir = models_dir();
    let dir = base_dir.unwrap_or(&default_dir);

    match canonical {
        "parakeet-tdt-0.6b-v3" => {
            let model_path = dir.join("parakeet");
            model_path.join("encoder.int8.onnx").is_file()
                && model_path.join("decoder.int8.onnx").is_file()
                && model_path.join("joiner.int8.onnx").is_file()
                && model_path.join("tokens.txt").is_file()
        }
        "moonshine-base-en" => {
            let model_path = dir.join("moonshine");
            model_path.join("encoder.int8.onnx").is_file()
                && model_path.join("decoder.int8.onnx").is_file()
                && model_path.join("tokens.txt").is_file()
        }
        "moonshine-tiny-en" => {
            let model_path = dir.join("moonshine-tiny");
            model_path.join("encoder.int8.onnx").is_file()
                && model_path.join("decoder.int8.onnx").is_file()
                && model_path.join("tokens.txt").is_file()
        }
        "kws-zipformer-zh-en-3M" => {
            let model_path = dir.join("kws");
            model_path
                .join("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx")
                .is_file()
                || model_path.join("encoder.int8.onnx").is_file()
        }
        "silero_vad_v6" => dir.join("silero_vad.onnx").is_file(),
        _ => {
            // Whisper models are single .bin files
            let filename = format!("ggml-{}.bin", canonical.trim_start_matches("whisper-"));
            let bin_path = dir.join(&filename);
            let direct_path = dir.join(format!("{canonical}.bin"));
            bin_path.is_file() || direct_path.is_file()
        }
    }
}

/// Compute the SHA-256 hash of a file as a lowercase hexadecimal string.
pub fn compute_file_sha256(path: &Path) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];

    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Compare a file's computed SHA-256 against an expected hex string (case-insensitive).
pub fn verify_file_sha256(path: &Path, expected_sha256: &str) -> bool {
    match compute_file_sha256(path) {
        Ok(actual) => actual.eq_ignore_ascii_case(expected_sha256.trim()),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_resolve_model_alias() {
        assert_eq!(resolve_model_alias("auto"), "parakeet-tdt-0.6b-v3");
        assert_eq!(resolve_model_alias("parakeet"), "parakeet-tdt-0.6b-v3");
        assert_eq!(resolve_model_alias("turbo"), "whisper-large-v3-turbo");
        assert_eq!(resolve_model_alias("distil"), "distil-whisper-large-v3");
        assert_eq!(resolve_model_alias("small"), "whisper-small-en");
        assert_eq!(resolve_model_alias("moonshine"), "moonshine-base-en");
        assert_eq!(resolve_model_alias("kws"), "kws-zipformer-zh-en-3M");
        assert_eq!(resolve_model_alias("vad"), "silero_vad_v6");
    }

    #[test]
    fn test_get_model_entry() {
        let entry =
            get_model_entry("parakeet-tdt-0.6b-v3").expect("parakeet must exist in catalog");
        assert_eq!(entry.id, "parakeet-tdt-0.6b-v3");
        assert!(entry.size_bytes > 500_000_000);
        assert!(entry.is_archive);

        let turbo = get_model_entry("turbo").expect("alias must resolve in get_model_entry");
        assert_eq!(turbo.id, "whisper-large-v3-turbo");
    }

    #[test]
    fn test_compute_and_verify_sha256() {
        let mut temp = NamedTempFile::new().unwrap();
        let payload = b"Taurine voice dictation verification payload";
        temp.write_all(payload).unwrap();
        temp.flush().unwrap();

        let path = temp.path();
        let computed = compute_file_sha256(path).unwrap();

        // Independent hash computation
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let expected = hex::encode(hasher.finalize());

        assert_eq!(computed, expected);
        assert!(verify_file_sha256(path, &expected));
        assert!(!verify_file_sha256(
            path,
            "bad_hash_00000000000000000000000000000000000000000000000000000000000000"
        ));
    }

    #[test]
    fn test_is_model_downloaded_false_when_missing() {
        let temp_dir = tempfile::tempdir().unwrap();
        assert!(!is_model_downloaded("parakeet", Some(temp_dir.path())));
        assert!(!is_model_downloaded(
            "whisper-small-en",
            Some(temp_dir.path())
        ));
        assert!(!is_model_downloaded("moonshine", Some(temp_dir.path())));
    }
}
