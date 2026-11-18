//! Voice dictation crash recovery, orphaned session rescue, and emergency dumps.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn};

use crate::platform::ClipboardManager;

static ACTIVE_TRANSCRIPT_DUMP: Mutex<Option<String>> = Mutex::new(None);
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Returns the path to the crash recovery directory inside the Taurine data dir.
pub fn recovery_dir() -> std::io::Result<PathBuf> {
    let dir = taurine_core::paths::get_data_dir().join("recovery");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Helper to set clipboard text using the platform clipboard manager (or fake clipboard in tests).
pub fn copy_to_clipboard(text: &str) {
    if let Ok(mut clip) = crate::platform::get_clipboard_manager() {
        let _ = clip.set_text(text);
    }
}

/// Updates the in-flight transcript snapshot for emergency crash recovery.
pub fn update_active_transcript_dump(text: &str) {
    let mut lock = ACTIVE_TRANSCRIPT_DUMP
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    *lock = Some(text.to_string());
}

/// Clears the in-flight transcript snapshot once dictation successfully finishes.
pub fn clear_active_transcript_dump() {
    let mut lock = ACTIVE_TRANSCRIPT_DUMP
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    *lock = None;
}

/// Performs an emergency dump of any active transcript to disk and the system clipboard.
pub fn emergency_dump_to_disk_and_clipboard(data_dir: &Path) -> Option<String> {
    let text = {
        let lock = ACTIVE_TRANSCRIPT_DUMP
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        lock.clone()?
    };

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let recovery_dir = data_dir.join("recovery");
    let _ = std::fs::create_dir_all(&recovery_dir);
    let dump_file = recovery_dir.join("emergency_crash_dump.txt");
    let _ = std::fs::write(&dump_file, trimmed);

    copy_to_clipboard(trimmed);
    Some(trimmed.to_string())
}

/// Installs a panic hook that dumps any in-flight voice transcript before the process exits.
pub fn install_emergency_panic_handler() {
    if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let data_dir = taurine_core::paths::get_data_dir();
        emergency_dump_to_disk_and_clipboard(&data_dir);
        prev_hook(info);
    }));
}

/// Discovers any orphaned audio recording journals left over from an interrupted session,
/// transcribes them, records them in the activity log, copies the text to the clipboard,
/// and deletes the recovery files.
pub fn recover_orphaned_audio_sessions(
    cloud_client: &taurine_core::voice::GroqVoiceClient,
    dict: &taurine_core::voice::VoiceDictionary,
) -> Result<Vec<String>, String> {
    let dir = match recovery_dir() {
        Ok(d) => d,
        Err(e) => return Err(format!("Failed to access recovery directory: {e}")),
    };

    let orphaned_files = match taurine_core::voice::list_orphaned_sessions(&dir) {
        Ok(files) => files,
        Err(e) => return Err(format!("Failed to list orphaned voice sessions: {e}")),
    };

    recover_orphaned_files(cloud_client, dict, orphaned_files)
}

/// Recovers specific orphaned audio session files from disk.
pub fn recover_orphaned_files(
    cloud_client: &taurine_core::voice::GroqVoiceClient,
    dict: &taurine_core::voice::VoiceDictionary,
    orphaned_files: Vec<PathBuf>,
) -> Result<Vec<String>, String> {
    if orphaned_files.is_empty() {
        return Ok(Vec::new());
    }

    info!(
        "VoiceRecovery: processing {} orphaned voice session journal(s)",
        orphaned_files.len()
    );

    let mut recovered_texts = Vec::new();
    let terms = dict.terms().to_vec();

    for file in orphaned_files {
        let samples = match taurine_core::voice::read_spooled_samples(&file) {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    "VoiceRecovery: failed to read orphaned session {:?}: {e}",
                    file
                );
                let _ = std::fs::remove_file(&file);
                continue;
            }
        };

        if !taurine_core::voice::has_voice_activity(&samples) {
            info!(
                "VoiceRecovery: orphaned session {:?} contains no voice activity; discarding",
                file
            );
            let _ = std::fs::remove_file(&file);
            continue;
        }

        info!(
            "VoiceRecovery: recovering {} audio samples ({:.1}s) from {:?}",
            samples.len(),
            samples.len() as f64 / 16000.0,
            file
        );

        match cloud_client.transcribe_blocking(&samples, 16000, None, Some(&terms)) {
            Ok(text) => {
                let formatted = taurine_core::voice::format_transcript(&text);
                let trimmed = formatted.trim();
                if !trimmed.is_empty() {
                    info!(
                        "VoiceRecovery: successfully rescued transcript: '{}'",
                        trimmed
                    );
                    copy_to_clipboard(trimmed);

                    let word_count = trimmed.split_whitespace().count();
                    taurine_core::db::crud::stats::record_voice_dictation_usage(
                        word_count,
                        trimmed.len(),
                        None,
                        Some(trimmed.to_string()),
                    );

                    recovered_texts.push(trimmed.to_string());
                }
                let _ = std::fs::remove_file(&file);
            }
            Err(e) => {
                warn!(
                    "VoiceRecovery: transcription failed for {:?}: {e}; preserving journal for future retry",
                    file
                );
            }
        }
    }

    Ok(recovered_texts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emergency_dump_writes_file_and_clipboard() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        crate::platform::test_injector().clear();

        update_active_transcript_dump("This is an emergency rescued transcript.");
        let dumped = emergency_dump_to_disk_and_clipboard(temp_dir.path());
        assert_eq!(
            dumped,
            Some("This is an emergency rescued transcript.".to_string())
        );

        let dump_file = temp_dir
            .path()
            .join("recovery")
            .join("emergency_crash_dump.txt");
        assert!(dump_file.exists());
        let content = std::fs::read_to_string(dump_file).unwrap();
        assert_eq!(content, "This is an emergency rescued transcript.");

        let clip_text = crate::platform::read_clipboard_text().unwrap();
        assert_eq!(clip_text, "This is an emergency rescued transcript.");

        clear_active_transcript_dump();
        let dumped_after = emergency_dump_to_disk_and_clipboard(temp_dir.path());
        assert_eq!(dumped_after, None);
    }
}
