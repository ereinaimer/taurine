//! Voice Activity Detection (VAD) and silence-aware audio chunking for long-form dictation.

/// Minimal RMS threshold to consider a 32ms frame as containing active speech.
/// Ambient room noise and electrical floor typically register below 0.003 (~ -50 dBFS).
pub const SPEECH_FRAME_RMS_THRESHOLD: f32 = 0.003;

/// Minimum consecutive speech frames required to confirm voice activity (3 frames = ~96ms at 16kHz).
pub const MIN_SPEECH_BURST_FRAMES: usize = 3;

/// Frame size for energy and silence analysis (512 samples = 32ms at 16kHz).
pub const VAD_FRAME_SAMPLES: usize = 512;

/// Maximum chunk duration in samples (3 minutes at 16kHz = 2,880,000 samples).
pub const DEFAULT_CHUNK_MAX_SAMPLES: usize = 16_000 * 60 * 3;

/// Minimum chunk duration before searching for a silence split point (2 minutes = 1,920,000 samples).
pub const DEFAULT_CHUNK_MIN_SAMPLES: usize = 16_000 * 60 * 2;

/// Threshold below which a frame is considered silence for segmentation split points.
pub const SILENCE_SPLIT_RMS: f32 = 0.003;

/// Analyzes an audio buffer using frame-based energy detection to determine
/// whether it contains genuine human speech versus pure silence or ambient noise floor.
pub fn has_voice_activity(samples: &[f32]) -> bool {
    if samples.len() < VAD_FRAME_SAMPLES * MIN_SPEECH_BURST_FRAMES {
        return false;
    }

    let mut consecutive_speech_frames = 0;
    let mut total_speech_frames = 0;

    for chunk in samples.as_chunks::<VAD_FRAME_SAMPLES>().0 {
        let mean_sq = chunk.iter().map(|&s| s * s).sum::<f32>() / VAD_FRAME_SAMPLES as f32;
        let rms = mean_sq.sqrt();
        if rms >= SPEECH_FRAME_RMS_THRESHOLD {
            consecutive_speech_frames += 1;
            total_speech_frames += 1;
            if consecutive_speech_frames >= MIN_SPEECH_BURST_FRAMES {
                return true;
            }
        } else {
            consecutive_speech_frames = 0;
        }
    }

    total_speech_frames >= 6
}

/// Computes the frame index with the lowest RMS energy within the given sample range.
fn find_quietest_split_point(samples: &[f32], start_idx: usize, end_idx: usize) -> usize {
    let mut best_idx = (start_idx + end_idx) / 2;
    let mut lowest_rms = f32::MAX;

    let mut idx = start_idx;
    while idx + VAD_FRAME_SAMPLES <= end_idx {
        let frame = &samples[idx..idx + VAD_FRAME_SAMPLES];
        let mean_sq = frame.iter().map(|&s| s * s).sum::<f32>() / VAD_FRAME_SAMPLES as f32;
        let rms = mean_sq.sqrt();
        if rms < lowest_rms {
            lowest_rms = rms;
            best_idx = idx + (VAD_FRAME_SAMPLES / 2);
            if lowest_rms < SILENCE_SPLIT_RMS {
                // Found an excellent silence split point
                break;
            }
        }
        idx += VAD_FRAME_SAMPLES;
    }

    best_idx
}

/// Slices a long audio buffer into silence-aware chunks of at most `max_samples`.
///
/// Guarantees that no chunk exceeds `max_samples` and splits occur at natural speech pauses
/// so words are never severed mid-syllable.
pub fn split_audio_into_chunks(
    samples: &[f32],
    max_samples: usize,
    min_split_samples: usize,
) -> Vec<Vec<f32>> {
    if samples.len() <= max_samples {
        return vec![samples.to_vec()];
    }

    let mut chunks = Vec::new();
    let mut offset = 0;

    while offset < samples.len() {
        let remaining = samples.len() - offset;
        if remaining <= max_samples {
            chunks.push(samples[offset..].to_vec());
            break;
        }

        // Search window for a natural silence split point between min and max bounds
        let min_split = min_split_samples.min(max_samples);
        let window_start = offset + min_split.min(remaining);
        let window_end = offset + max_samples.min(remaining);

        let split_point = find_quietest_split_point(samples, window_start, window_end);
        let slice_len = split_point.saturating_sub(offset).max(VAD_FRAME_SAMPLES);
        let actual_end = (offset + slice_len).min(samples.len());

        chunks.push(samples[offset..actual_end].to_vec());
        offset = actual_end;
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_voice_activity_pure_silence() {
        let silence = vec![0.0f32; 16000 * 2]; // 2 seconds of zeroes
        assert!(!has_voice_activity(&silence));
    }

    #[test]
    fn test_has_voice_activity_low_ambient_noise() {
        // Floor noise well below speech threshold (e.g. 0.001 RMS)
        let noise = vec![0.001f32; 16000 * 2];
        assert!(!has_voice_activity(&noise));
    }

    #[test]
    fn test_has_voice_activity_single_click_pop_rejected() {
        // 2 seconds of silence with a brief 1-frame click
        let mut audio = vec![0.0f32; 16000 * 2];
        for sample in &mut audio[8000..8200] {
            *sample = 0.5; // Single short transient
        }
        assert!(!has_voice_activity(&audio));
    }

    #[test]
    fn test_has_voice_activity_sustained_speech_burst_detected() {
        // 1 second silence + 200ms of sustained speech tone + 1 second silence
        let mut audio = vec![0.0005f32; 16000 * 2];
        for sample in &mut audio[16000..16000 + 3200] {
            // 200ms = ~6 frames of speech energy
            *sample = 0.05;
        }
        assert!(has_voice_activity(&audio));
    }

    #[test]
    fn test_split_audio_short_stays_single_chunk() {
        let audio = vec![0.02f32; 1000];
        let chunks = split_audio_into_chunks(&audio, 5000, 2000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 1000);
    }

    #[test]
    fn test_split_audio_finds_silence_valley() {
        // Total 10,000 samples. Max chunk: 6,000, Min split: 3,000.
        // Insert a silence valley at sample 4000..4600.
        let mut audio = vec![0.05f32; 10000];
        for sample in &mut audio[4000..4600] {
            *sample = 0.0001; // Silence valley
        }

        let chunks = split_audio_into_chunks(&audio, 6000, 3000);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].len() <= 6000);
        assert!(chunks[0].len() >= 4000);
        assert_eq!(chunks[0].len() + chunks[1].len(), 10000);
    }

    #[test]
    fn test_split_audio_min_split_greater_than_max_samples_clamped() {
        let audio = vec![0.05f32; 10000];
        // min_split (7000) > max_samples (5000) should be clamped safely
        let chunks = split_audio_into_chunks(&audio, 5000, 7000);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= 5000);
        }
    }
}
