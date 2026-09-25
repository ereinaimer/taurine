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
    /// Fallback download URL if primary source fails.
    pub fallback_url: Option<&'static str>,
    /// Individual remote filenames for multi-file models.
    pub remote_files: Option<&'static [&'static str]>,
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
        id: "parakeet-unified-en-0.6b",
        name: "Parakeet Unified English",
        size_display: "631 MB",
        size_bytes: 501_350_460,
        url: "https://huggingface.co/csukuangfj2/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming/resolve/main/",
        fallback_url: Some(
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming.tar.bz2",
        ),
        remote_files: Some(&[
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ]),
        sha256: None,
        is_archive: false,
        description: "NVIDIA FastConformer RNN-T; English-only dictation with native punctuation",
    },
    ModelCatalogEntry {
        id: "parakeet-tdt-ctc-110m",
        name: "Parakeet Light English",
        size_display: "135 MB",
        size_bytes: 104_337_827,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet_tdt_ctc_110m-en-36000-int8.tar.bz2",
        fallback_url: None,
        remote_files: None,
        sha256: None,
        is_archive: true,
        description: "NVIDIA hybrid TDT-CTC; fast English dictation for constrained machines",
    },
    ModelCatalogEntry {
        id: "kws-zipformer-zh-en-3M",
        name: "Zipformer KWS Spotter 3M",
        size_display: "33 MB",
        size_bytes: 32_885_699,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20.tar.bz2",
        fallback_url: None,
        remote_files: None,
        sha256: None,
        is_archive: true,
        description: "Ultra-low power resident keyword spotter; <0.3% CPU usage",
    },
    ModelCatalogEntry {
        id: "silero_vad_v6",
        name: "Silero VAD v6 ONNX",
        size_display: "644 KB",
        size_bytes: 643_854,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
        fallback_url: None,
        remote_files: None,
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
    let trimmed = identifier.trim().to_ascii_lowercase();
    let canonical = match trimmed.as_str() {
        "unified" | "parakeet-unified" | "best" | "quality" | "parakeet-unified-en-0.6b" => {
            "parakeet-unified-en-0.6b"
        }
        "110m" | "parakeet-110m" | "light" | "fast" | "tdt-ctc" | "parakeet-tdt-ctc-110m" => {
            "parakeet-tdt-ctc-110m"
        }
        "kws" | "zipformer" | "kws-zipformer-zh-en-3M" => "kws-zipformer-zh-en-3M",
        "vad" | "silero" | "silero_vad_v6" => "silero_vad_v6",
        "auto" => {
            if get_system_ram_gb() >= 8 {
                "parakeet-unified-en-0.6b"
            } else {
                "parakeet-tdt-ctc-110m"
            }
        }
        _ => {
            for entry in MODEL_CATALOG {
                if entry.id.eq_ignore_ascii_case(&trimmed) {
                    return Some(entry);
                }
            }
            return None;
        }
    };
    MODEL_CATALOG.iter().find(|entry| entry.id == canonical)
}

/// Return all available catalog entries.
pub fn list_models() -> &'static [ModelCatalogEntry] {
    MODEL_CATALOG
}

/// Returns total system physical memory in gigabytes, categorized by physical RAM tier.
pub fn get_system_ram_gb() -> u64 {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    let bytes = sys.total_memory();

    let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    if gb >= 28.0 {
        32
    } else if gb >= 13.0 {
        16
    } else if gb >= 6.5 {
        8
    } else if gb >= 3.0 {
        4
    } else {
        gb.round() as u64
    }
}

/// Minimum free system memory required to load the quality engine alongside the light engine.
pub const UNIFIED_MIN_FREE_BYTES: u64 = 1_073_741_824;

/// Free (available, not total) system memory in bytes. 0 on error (fail-closed: light engine only).
pub fn available_memory_bytes() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.free_memory()
}

/// True when the quality engine may be loaded in parallel with the light engine.
pub fn quality_engine_allowed() -> bool {
    available_memory_bytes() >= UNIFIED_MIN_FREE_BYTES
}

/// Resolves user-friendly aliases to their canonical model catalog ID.
pub fn resolve_model_alias(alias: &str) -> &'static str {
    let trimmed = alias.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "unified" | "parakeet-unified" | "best" | "quality" => "parakeet-unified-en-0.6b",
        "110m" | "parakeet-110m" | "light" | "fast" | "tdt-ctc" => "parakeet-tdt-ctc-110m",
        "kws" | "zipformer" => "kws-zipformer-zh-en-3M",
        "vad" | "silero" => "silero_vad_v6",
        "auto" => {
            if get_system_ram_gb() >= 8 {
                "parakeet-unified-en-0.6b"
            } else {
                "parakeet-tdt-ctc-110m"
            }
        }
        _ => {
            for entry in MODEL_CATALOG {
                if entry.id.eq_ignore_ascii_case(&trimmed) {
                    return entry.id;
                }
            }
            if get_system_ram_gb() >= 8 {
                "parakeet-unified-en-0.6b"
            } else {
                "parakeet-tdt-ctc-110m"
            }
        }
    }
}

fn is_non_empty_file(path: &Path) -> bool {
    path.is_file()
        && std::fs::metadata(path)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
}

fn has_matching_file(dir: &Path, prefix: &str, suffix: &str) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.starts_with(prefix)
                && name.ends_with(suffix)
                && std::fs::metadata(&path)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

/// Check if a model's required files are present and non-empty on disk.
pub fn is_model_downloaded(model_id: &str, base_dir: Option<&Path>) -> bool {
    let canonical = resolve_model_alias(model_id);
    let default_dir = models_dir();
    let dir = base_dir.unwrap_or(&default_dir);

    match canonical {
        "parakeet-unified-en-0.6b" => {
            let model_path = dir.join("parakeet-unified");
            model_path.is_dir()
                && (is_non_empty_file(&model_path.join("encoder.int8.onnx"))
                    || has_matching_file(&model_path, "encoder", ".onnx"))
                && (is_non_empty_file(&model_path.join("decoder.int8.onnx"))
                    || has_matching_file(&model_path, "decoder", ".onnx"))
                && (is_non_empty_file(&model_path.join("joiner.int8.onnx"))
                    || has_matching_file(&model_path, "joiner", ".onnx"))
                && (is_non_empty_file(&model_path.join("tokens.txt"))
                    || has_matching_file(&model_path, "tokens", ".txt"))
        }
        "parakeet-tdt-ctc-110m" => {
            let model_path = dir.join("parakeet-110m");
            model_path.is_dir()
                && (is_non_empty_file(&model_path.join("model.int8.onnx"))
                    || has_matching_file(&model_path, "model", ".onnx"))
                && (is_non_empty_file(&model_path.join("tokens.txt"))
                    || has_matching_file(&model_path, "tokens", ".txt"))
        }
        "kws-zipformer-zh-en-3M" => {
            let model_path = dir.join("kws");
            model_path.is_dir()
                && (is_non_empty_file(&model_path.join("encoder.int8.onnx"))
                    || has_matching_file(&model_path, "encoder", ".onnx"))
                && (is_non_empty_file(&model_path.join("decoder.int8.onnx"))
                    || has_matching_file(&model_path, "decoder", ".onnx"))
                && (is_non_empty_file(&model_path.join("joiner.int8.onnx"))
                    || has_matching_file(&model_path, "joiner", ".onnx"))
                && (is_non_empty_file(&model_path.join("tokens.txt"))
                    || has_matching_file(&model_path, "tokens", ".txt"))
        }
        "silero_vad_v6" => is_non_empty_file(&dir.join("silero_vad.onnx")),
        _ => false,
    }
}

/// Best-effort removal of deprecated model dirs/files. Returns count removed. Never errors.
pub fn prune_deprecated_voice_models(dir: &Path) -> usize {
    let mut count = 0;
    if !dir.is_dir() {
        return 0;
    }

    // Deprecated directories
    for legacy_dir in ["parakeet", "moonshine", "moonshine-tiny"] {
        let p = dir.join(legacy_dir);
        if p.is_dir() && std::fs::remove_dir_all(&p).is_ok() {
            count += 1;
        }
    }

    // Deprecated files (ggml-*.bin, whisper-*.bin)
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
                && ((name.starts_with("ggml-") && name.ends_with(".bin"))
                    || (name.starts_with("whisper-") && name.ends_with(".bin")))
                && std::fs::remove_file(&path).is_ok()
            {
                count += 1;
            }
        }
    }

    count
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
        let expected_auto = if get_system_ram_gb() >= 8 {
            "parakeet-unified-en-0.6b"
        } else {
            "parakeet-tdt-ctc-110m"
        };
        assert_eq!(resolve_model_alias("auto"), expected_auto);
        assert_eq!(resolve_model_alias("unified"), "parakeet-unified-en-0.6b");
        assert_eq!(resolve_model_alias("110m"), "parakeet-tdt-ctc-110m");
        assert_eq!(resolve_model_alias("kws"), "kws-zipformer-zh-en-3M");
        assert_eq!(resolve_model_alias("vad"), "silero_vad_v6");
    }

    #[test]
    fn test_get_model_entry() {
        let entry =
            get_model_entry("parakeet-unified-en-0.6b").expect("unified must exist in catalog");
        assert_eq!(entry.id, "parakeet-unified-en-0.6b");
        assert!(entry.size_bytes > 400_000_000);
        assert!(!entry.is_archive);

        let light = get_model_entry("110m").expect("alias must resolve in get_model_entry");
        assert_eq!(light.id, "parakeet-tdt-ctc-110m");
        assert!(light.is_archive);

        assert_eq!(get_model_entry("moonshine-tiny-en"), None);
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
        assert!(!is_model_downloaded(
            "parakeet-unified-en-0.6b",
            Some(temp_dir.path())
        ));
        assert!(!is_model_downloaded("110m", Some(temp_dir.path())));
    }

    #[test]
    fn test_prune_deprecated_voice_models() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();
        std::fs::create_dir_all(path.join("parakeet")).unwrap();
        std::fs::create_dir_all(path.join("moonshine")).unwrap();
        std::fs::create_dir_all(path.join("moonshine-tiny")).unwrap();
        std::fs::create_dir_all(path.join("parakeet-unified")).unwrap();
        std::fs::write(path.join("ggml-small.en.bin"), b"test").unwrap();
        std::fs::write(path.join("whisper-base.bin"), b"test").unwrap();
        std::fs::write(path.join("silero_vad.onnx"), b"test").unwrap();

        let removed = prune_deprecated_voice_models(path);
        assert_eq!(removed, 5);
        assert!(!path.join("parakeet").exists());
        assert!(!path.join("moonshine").exists());
        assert!(!path.join("moonshine-tiny").exists());
        assert!(!path.join("ggml-small.en.bin").exists());
        assert!(!path.join("whisper-base.bin").exists());
        assert!(path.join("parakeet-unified").exists());
        assert!(path.join("silero_vad.onnx").exists());
    }

    #[test]
    fn test_system_ram_detection() {
        let ram = get_system_ram_gb();
        println!("Detected RAM tier: {ram} GB");
        assert!(ram >= 4);
    }

    #[test]
    fn test_available_memory_sane() {
        let free = available_memory_bytes();
        assert!(free > 0, "must report some free memory on any dev machine");
        assert!(
            free < 4 * 1024 * 1024 * 1024 * 1024,
            "sanity upper bound 4 TiB"
        );
        // Gate agrees with itself:
        assert_eq!(quality_engine_allowed(), free >= UNIFIED_MIN_FREE_BYTES);
    }

    #[test]
    fn test_is_model_downloaded_kws_epoch_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let kws_dir = temp_dir.path().join("kws");
        std::fs::create_dir_all(&kws_dir).unwrap();

        assert!(!is_model_downloaded(
            "kws-zipformer-zh-en-3M",
            Some(temp_dir.path())
        ));

        // Create epoch-suffixed files like sherpa-onnx archive
        std::fs::write(
            kws_dir.join("encoder-epoch-13-avg-2-chunk-16-left-64.int8.onnx"),
            b"model data",
        )
        .unwrap();
        std::fs::write(
            kws_dir.join("decoder-epoch-13-avg-2-chunk-16-left-64.onnx"),
            b"model data",
        )
        .unwrap();
        std::fs::write(
            kws_dir.join("joiner-epoch-13-avg-2-chunk-16-left-64.int8.onnx"),
            b"model data",
        )
        .unwrap();
        std::fs::write(kws_dir.join("tokens.txt"), b"model data").unwrap();

        assert!(is_model_downloaded(
            "kws-zipformer-zh-en-3M",
            Some(temp_dir.path())
        ));
    }

    #[test]
    fn test_is_model_downloaded_rejects_empty_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let kws_dir = temp_dir.path().join("kws");
        std::fs::create_dir_all(&kws_dir).unwrap();

        // Write 0-byte files
        std::fs::write(
            kws_dir.join("encoder-epoch-13-avg-2-chunk-16-left-64.int8.onnx"),
            b"",
        )
        .unwrap();
        std::fs::write(
            kws_dir.join("decoder-epoch-13-avg-2-chunk-16-left-64.onnx"),
            b"",
        )
        .unwrap();
        std::fs::write(
            kws_dir.join("joiner-epoch-13-avg-2-chunk-16-left-64.int8.onnx"),
            b"",
        )
        .unwrap();
        std::fs::write(kws_dir.join("tokens.txt"), b"").unwrap();

        assert!(!is_model_downloaded(
            "kws-zipformer-zh-en-3M",
            Some(temp_dir.path())
        ));
    }
}
