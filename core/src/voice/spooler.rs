//! Continuous audio spooler and crash recovery journal for voice dictation.
//!
//! Streams incoming audio samples directly to disk while recording so that
//! unexpected process termination or system crashes never result in lost speech.

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// Extension for active crash recovery audio journal files.
pub const RECOVERY_EXTENSION: &str = "pcm";

/// Manages an append-only rolling audio journal on disk for an active recording session.
#[derive(Debug)]
pub struct AudioSpooler {
    path: PathBuf,
    writer: BufWriter<File>,
    samples_written: usize,
}

impl AudioSpooler {
    /// Creates a new spooler writing to `recovery_dir/active_session_{session_id}.pcm`.
    pub fn new(recovery_dir: impl AsRef<Path>, session_id: &str) -> std::io::Result<Self> {
        let dir = recovery_dir.as_ref();
        fs::create_dir_all(dir)?;

        let sanitized_id = session_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let path = dir.join(format!(
            "active_session_{sanitized_id}.{RECOVERY_EXTENSION}"
        ));

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;

        Ok(Self {
            path,
            writer: BufWriter::new(file),
            samples_written: 0,
        })
    }

    /// Appends audio samples to the recovery journal file.
    pub fn append_samples(&mut self, samples: &[f32]) -> std::io::Result<()> {
        if samples.is_empty() {
            return Ok(());
        }

        // One buffer-marshal per chunk instead of a write call per sample.
        // BufWriter already batches OS writes; this removes the per-sample
        // call overhead. flush stays: it bounds the hard-kill tail-loss window.
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for &sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.writer.write_all(&bytes)?;
        self.writer.flush()?;
        self.samples_written += samples.len();
        Ok(())
    }

    /// Returns the number of samples written to disk so far.
    pub fn samples_written(&self) -> usize {
        self.samples_written
    }

    /// Returns the file path of this recovery journal.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Finalizes the journal, flushes all buffers, and returns the file path.
    pub fn finish(mut self) -> std::io::Result<PathBuf> {
        self.writer.flush()?;
        Ok(self.path)
    }

    /// Discards the journal and securely removes the file from disk.
    pub fn discard(self) -> std::io::Result<()> {
        let path = self.path;
        drop(self.writer);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// Reads spooled raw f32 samples from an orphaned recovery journal file.
pub fn read_spooled_samples(path: impl AsRef<Path>) -> std::io::Result<Vec<f32>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;

    // Each f32 sample is 4 bytes. Drop incomplete trailing byte fragments.
    let valid_bytes = bytes.len() - (bytes.len() % 4);
    let mut samples = Vec::with_capacity(valid_bytes / 4);

    for chunk in bytes[..valid_bytes].as_chunks::<4>().0 {
        samples.push(f32::from_le_bytes(*chunk));
    }

    Ok(samples)
}

/// Discovers all orphaned recovery journals in the given recovery directory.
pub fn list_orphaned_sessions(recovery_dir: impl AsRef<Path>) -> std::io::Result<Vec<PathBuf>> {
    let dir = recovery_dir.as_ref();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut orphaned = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|ext| ext.to_str()) == Some(RECOVERY_EXTENSION)
        {
            orphaned.push(path);
        }
    }

    orphaned.sort();
    Ok(orphaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_spooler_create_append_read_back() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut spooler = AudioSpooler::new(temp_dir.path(), "test_req_1").unwrap();

        let samples_1 = vec![0.1f32, 0.2, 0.3, 0.4];
        let samples_2 = vec![0.5f32, 0.6, 0.7];

        spooler.append_samples(&samples_1).unwrap();
        spooler.append_samples(&samples_2).unwrap();
        assert_eq!(spooler.samples_written(), 7);

        let path = spooler.finish().unwrap();
        assert!(path.exists());

        let read_back = read_spooled_samples(&path).unwrap();
        assert_eq!(read_back, vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7]);
    }

    #[test]
    fn test_audio_spooler_discard_deletes_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut spooler = AudioSpooler::new(temp_dir.path(), "test_req_discard").unwrap();

        spooler.append_samples(&[0.1, 0.2]).unwrap();
        let path = spooler.path().to_path_buf();
        assert!(path.exists());

        spooler.discard().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_read_spooled_samples_handles_incomplete_trailing_bytes() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("corrupt.pcm");

        // Write 1 valid sample (4 bytes) + 2 corrupt trailing bytes
        let mut raw = Vec::new();
        raw.extend_from_slice(&0.5f32.to_le_bytes());
        raw.push(0xFF);
        raw.push(0xEE);
        fs::write(&file_path, &raw).unwrap();

        let recovered = read_spooled_samples(&file_path).unwrap();
        assert_eq!(recovered.len(), 1);
        assert!((recovered[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_list_orphaned_sessions_discovers_journals() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_1 = temp_dir.path().join("active_session_1.pcm");
        let file_2 = temp_dir.path().join("active_session_2.pcm");
        let file_ignore = temp_dir.path().join("other.txt");

        fs::write(&file_1, b"test").unwrap();
        fs::write(&file_2, b"test").unwrap();
        fs::write(&file_ignore, b"test").unwrap();

        let found = list_orphaned_sessions(temp_dir.path()).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.contains(&file_1));
        assert!(found.contains(&file_2));
        assert!(!found.contains(&file_ignore));
    }
}
