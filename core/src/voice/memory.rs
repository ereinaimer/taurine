// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

//! Voice engine memory telemetry and disk metrics.
//!
//! Provides resident set size (RSS) tracking using sysinfo and on-disk model
//! size calculation to observe memory consumption during model load, step-up,
//! and eviction.

use std::path::Path;
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Returns the current process resident set size (RSS) in megabytes.
pub fn process_rss_mb() -> Option<u64> {
    let pid = Pid::from_u32(std::process::id());
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.memory() / (1024 * 1024))
}

/// Helper to format an optional megabyte value, rendering `None` as `"unknown"`.
pub fn format_mb(mb: Option<u64>) -> String {
    mb.map(|n| n.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Best-effort on-disk size of model files in megabytes.
pub fn model_disk_size_mb(model_id: &str, base_dir: Option<&Path>) -> Option<u64> {
    let canonical = super::models::resolve_model_alias(model_id);
    let default_dir = super::models::models_dir();
    let dir = base_dir.unwrap_or(&default_dir);

    let file_path = match canonical {
        "parakeet-unified-en-0.6b" => {
            let model_path = dir.join("parakeet-unified");
            let encoder = model_path.join("encoder.int8.onnx");
            if encoder.is_file() {
                encoder
            } else {
                return None;
            }
        }
        "parakeet-tdt-ctc-110m" => {
            let model_path = dir.join("parakeet-110m");
            let model = model_path.join("model.int8.onnx");
            if model.is_file() {
                model
            } else {
                return None;
            }
        }
        _ => return None,
    };

    std::fs::metadata(file_path)
        .ok()
        .map(|m| m.len() / (1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_rss_mb_is_positive() {
        let rss = process_rss_mb();
        assert!(rss.is_some(), "process RSS must be readable on host");
        let mb = rss.unwrap();
        assert!(mb > 0, "process RSS must be strictly positive, got {mb} MB");
    }

    #[test]
    fn test_format_mb() {
        assert_eq!(format_mb(Some(42)), "42");
        assert_eq!(format_mb(None), "unknown");
    }

    #[test]
    fn test_model_disk_size_mb_when_missing() {
        let dummy = Path::new("non_existent_dir_taurine_test");
        assert_eq!(
            model_disk_size_mb("parakeet-tdt-ctc-110m", Some(dummy)),
            None
        );
        assert_eq!(
            model_disk_size_mb("parakeet-unified-en-0.6b", Some(dummy)),
            None
        );
    }
}
