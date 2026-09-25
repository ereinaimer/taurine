// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::debug;

use super::models::{get_model_entry, is_model_downloaded, models_dir};
use crate::error::{Error, Result};

/// Type alias for download progress callbacks: `(downloaded_bytes, total_bytes_option, speed_bps)`.
pub type ProgressCallback<'a> = &'a mut dyn FnMut(u64, Option<u64>, f64);

/// Type alias for download status callbacks: phase description string.
pub type StatusCallback<'a> = &'a mut dyn FnMut(&str);

/// Download and setup a voice model from the catalog into the models directory.
///
/// Handles single file downloads (.bin, .onnx) and archive extractions (.tar.bz2).
/// Reports progress via optional callback throttled ~5Hz.
pub fn download_model(
    model_id: &str,
    base_dir: Option<&Path>,
    on_progress: Option<ProgressCallback>,
) -> Result<PathBuf> {
    download_model_with_status(model_id, base_dir, on_progress, None)
}

pub(crate) fn target_subfolder_for_model(canonical: &str) -> &str {
    match canonical {
        "parakeet-unified-en-0.6b" => "parakeet-unified",
        "parakeet-tdt-ctc-110m" => "parakeet-110m",
        "kws-zipformer-zh-en-3M" => "kws",
        _ => canonical,
    }
}

pub fn download_plan(model_id: &str) -> Result<(&'static str, bool)> {
    let entry = get_model_entry(model_id)
        .ok_or_else(|| Error::Config(format!("Unknown voice model identifier: '{model_id}'")))?;
    Ok((entry.url, entry.fallback_url.is_some()))
}

fn stream_to_file(
    mut reader: impl Read,
    dest_path: &Path,
    total_bytes: Option<u64>,
    on_progress: &mut Option<ProgressCallback>,
    expected_sha: Option<&str>,
) -> Result<()> {
    let mut file = File::create(dest_path).map_err(|e| {
        Error::Engine(format!(
            "Failed to create file {}: {e}",
            dest_path.display()
        ))
    })?;

    let start_time = Instant::now();
    let mut downloaded = 0u64;
    let mut buf = [0u8; 65536];
    let mut last_callback = Instant::now();
    let mut window_start = start_time;
    let mut window_bytes = 0u64;
    let mut speed_bps = 0.0;
    let mut hasher = Sha256::new();

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| Error::Engine(format!("Download error while streaming: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| Error::Engine(format!("Failed to write to file: {e}")))?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;
        window_bytes += n as u64;

        let now = Instant::now();
        let window_secs = now.duration_since(window_start).as_secs_f64();
        if window_secs >= 1.0 {
            speed_bps = window_bytes as f64 / window_secs;
            window_start = now;
            window_bytes = 0;
        } else if speed_bps == 0.0 {
            let elapsed = now.duration_since(start_time).as_secs_f64();
            if elapsed > 0.0 {
                speed_bps = downloaded as f64 / elapsed;
            }
        }

        if let Some(cb) = on_progress.as_mut()
            && now.duration_since(last_callback) >= Duration::from_millis(200)
        {
            cb(downloaded, total_bytes, speed_bps);
            last_callback = now;
        }
    }

    if let Some(cb) = on_progress.as_mut() {
        cb(downloaded, total_bytes, speed_bps);
    }

    if let Some(expected) = expected_sha {
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(Error::Engine(format!(
                "SHA-256 checksum mismatch: expected {expected}, got {actual}"
            )));
        }
    }

    Ok(())
}

fn download_and_extract_archive(
    url: &str,
    canonical: &str,
    model_id: &str,
    dir: &Path,
    sha256: Option<&str>,
    on_progress: &mut Option<ProgressCallback>,
    on_status: &mut Option<StatusCallback>,
) -> Result<()> {
    let agent = ureq::AgentBuilder::new()
        .redirects(10)
        .timeout(Duration::from_secs(600))
        .build();

    let response = agent.get(url).call().map_err(|e| {
        Error::Engine(format!(
            "Failed to download voice model archive '{url}': {e}"
        ))
    })?;

    let total_bytes = response
        .header("content-length")
        .and_then(|l| l.parse::<u64>().ok());

    let temp_archive = std::env::temp_dir().join(format!("taurine-{canonical}.tar.bz2"));
    let res = (|| -> Result<()> {
        stream_to_file(
            response.into_reader(),
            &temp_archive,
            total_bytes,
            on_progress,
            sha256,
        )?;

        if let Some(cb) = on_status.as_mut() {
            cb(&format!("Extracting voice model '{model_id}'"));
        }
        extract_model_archive(&temp_archive, canonical, dir)?;
        Ok(())
    })();

    let _ = fs::remove_file(&temp_archive);
    res
}

/// Download model archive or standalone binary with both byte progress and phase status callbacks.
pub fn download_model_with_status(
    model_id: &str,
    base_dir: Option<&Path>,
    mut on_progress: Option<ProgressCallback>,
    mut on_status: Option<StatusCallback>,
) -> Result<PathBuf> {
    let entry = get_model_entry(model_id)
        .ok_or_else(|| Error::Config(format!("Unknown voice model identifier: '{model_id}'")))?;
    let canonical = entry.id;

    let default_dir = models_dir();
    let dir = base_dir.unwrap_or(&default_dir);

    // If already downloaded and verified on disk, return immediately
    if is_model_downloaded(canonical, Some(dir)) {
        debug!(
            "Voice model '{canonical}' is already cached at {}",
            dir.display()
        );
        return Ok(dir.to_path_buf());
    }

    fs::create_dir_all(dir).map_err(|e| {
        Error::Engine(format!(
            "Failed to create voice models directory {}: {e}",
            dir.display()
        ))
    })?;

    debug!("Downloading voice model '{}'", entry.id);

    if let Some(files) = entry.remote_files {
        let subfolder = target_subfolder_for_model(canonical);
        let target_dir = dir.join(subfolder);
        fs::create_dir_all(&target_dir)
            .map_err(|e| Error::Engine(format!("Failed to create model target dir: {e}")))?;

        let mut download_multifile = || -> Result<()> {
            let agent = ureq::AgentBuilder::new()
                .redirects(10)
                .timeout(Duration::from_secs(600))
                .build();

            for file in files {
                let file_url = format!("{}{file}", entry.url);
                let response = agent
                    .get(&file_url)
                    .call()
                    .map_err(|e| Error::Engine(format!("Failed to download {file_url}: {e}")))?;
                let total_bytes = response
                    .header("content-length")
                    .and_then(|l| l.parse::<u64>().ok());
                let target_file = target_dir.join(file);
                let part_file = target_dir.join(format!("{file}.part"));

                let res = stream_to_file(
                    response.into_reader(),
                    &part_file,
                    total_bytes,
                    &mut on_progress,
                    None,
                );
                if let Err(e) = res {
                    let _ = fs::remove_file(&part_file);
                    return Err(e);
                }
                fs::rename(&part_file, &target_file)
                    .map_err(|e| Error::Engine(format!("Failed to rename part file: {e}")))?;
            }
            standardize_model_files(&target_dir);
            Ok(())
        };

        if let Err(err) = download_multifile() {
            if let Some(fallback_url) = entry.fallback_url {
                debug!(
                    "Multi-file download failed ({err}); falling back to archive {fallback_url}"
                );
                download_and_extract_archive(
                    fallback_url,
                    canonical,
                    entry.id,
                    dir,
                    entry.sha256,
                    &mut on_progress,
                    &mut on_status,
                )?;
            } else {
                return Err(err);
            }
        }
    } else if entry.is_archive {
        download_and_extract_archive(
            entry.url,
            canonical,
            entry.id,
            dir,
            entry.sha256,
            &mut on_progress,
            &mut on_status,
        )?;
    } else {
        // Single file download (Silero VAD .onnx)
        let filename = if canonical == "silero_vad_v6" {
            "silero_vad.onnx".to_string()
        } else {
            format!("{canonical}.bin")
        };

        let target_file = dir.join(&filename);
        let part_file = dir.join(format!("{filename}.part"));

        let agent = ureq::AgentBuilder::new()
            .redirects(10)
            .timeout(Duration::from_secs(600))
            .build();

        let response = agent.get(entry.url).call().map_err(|e| {
            Error::Engine(format!(
                "Failed to download voice model '{}': {e}",
                entry.id
            ))
        })?;
        let total_bytes = response
            .header("content-length")
            .and_then(|l| l.parse::<u64>().ok());

        let res = stream_to_file(
            response.into_reader(),
            &part_file,
            total_bytes,
            &mut on_progress,
            entry.sha256,
        );
        if let Err(e) = res {
            let _ = fs::remove_file(&part_file);
            return Err(e);
        }
        fs::rename(&part_file, &target_file).map_err(|e| {
            Error::Engine(format!(
                "Failed to rename part file to {}: {e}",
                target_file.display()
            ))
        })?;
    }

    debug!("Voice model '{canonical}' ready at {}", dir.display());
    Ok(dir.to_path_buf())
}

/// Extract downloaded archive into model subfolder.
fn extract_model_archive(archive_path: &Path, canonical: &str, dest_root: &Path) -> Result<()> {
    let subfolder = target_subfolder_for_model(canonical);

    let target_dir = dest_root.join(subfolder);
    fs::create_dir_all(&target_dir).map_err(|e| {
        Error::Engine(format!(
            "Failed to create model folder {}: {e}",
            target_dir.display()
        ))
    })?;

    // Create a temporary extraction directory to unbundle the archive contents
    let temp_extract =
        std::env::temp_dir().join(format!("taurine-extract-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_extract)
        .map_err(|e| Error::Engine(format!("Failed to create temp extract dir: {e}")))?;

    let tar_result = std::process::Command::new("tar")
        .arg("-xf")
        .arg(archive_path)
        .arg("-C")
        .arg(&temp_extract)
        .status();

    match tar_result {
        Ok(status) if status.success() => {
            // Relocate extracted model files into target_dir
            relocate_model_files(&temp_extract, &target_dir)?;
            standardize_model_files(&target_dir);
            let _ = fs::remove_dir_all(&temp_extract);
            Ok(())
        }
        Ok(status) => {
            let _ = fs::remove_dir_all(&temp_extract);
            Err(Error::Engine(format!(
                "Archive extraction command 'tar' exited with status: {status}"
            )))
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&temp_extract);
            Err(Error::Engine(format!(
                "Failed to execute system 'tar' command to extract archive: {e}"
            )))
        }
    }
}

/// Recursively search for model files in extracted root and copy them into target directory.
fn relocate_model_files(src: &Path, dst: &Path) -> Result<()> {
    let entries = fs::read_dir(src).map_err(|e| {
        Error::Engine(format!(
            "Failed to read extracted directory {}: {e}",
            src.display()
        ))
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // If it's a subdirectory, recursively inspect it
            relocate_model_files(&path, dst)?;
        } else if path.is_file() {
            let file_name = path.file_name().unwrap_or_default();
            let dest_file = dst.join(file_name);
            fs::copy(&path, &dest_file).map_err(|e| {
                Error::Engine(format!(
                    "Failed to copy extracted file {} to {}: {e}",
                    path.display(),
                    dest_file.display()
                ))
            })?;
        }
    }
    Ok(())
}

fn standardize_model_files(dst: &Path) {
    let aliases = [
        ("encoder.int8.onnx", "encoder", ".int8.onnx"),
        ("decoder.int8.onnx", "decoder", ".int8.onnx"),
        ("decoder.onnx", "decoder", ".onnx"),
        ("joiner.int8.onnx", "joiner", ".int8.onnx"),
        ("joiner.onnx", "joiner", ".onnx"),
        ("model.int8.onnx", "model", ".int8.onnx"),
    ];

    for (target_name, prefix, suffix) in aliases {
        let target_path = dst.join(target_name);
        if !target_path.exists()
            && let Ok(entries) = fs::read_dir(dst)
        {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file()
                    && let Some(name) = p.file_name().and_then(|n| n.to_str())
                    && name != target_name
                    && name.starts_with(prefix)
                    && name.ends_with(suffix)
                {
                    let _ = fs::copy(&p, &target_path);
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_plan_selection() {
        let (u_url, u_fallback) = download_plan("parakeet-unified-en-0.6b").unwrap();
        assert!(u_url.contains("huggingface.co"));
        assert!(u_fallback);

        let (l_url, l_fallback) = download_plan("parakeet-tdt-ctc-110m").unwrap();
        assert!(l_url.contains("github.com"));
        assert!(!l_fallback);

        let err = download_plan("non-existent-model").unwrap_err();
        assert!(err.to_string().contains("non-existent-model"));
    }
}
