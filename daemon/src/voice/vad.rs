use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};
use std::path::Path;

/// Voice Activity Detection (VAD) gate powered by Silero VAD v6 with energy-based fallback.
pub struct VadGate {
    detector: Option<VoiceActivityDetector>,
    energy_threshold: f32,
    last_frame_detected: bool,
}

impl VadGate {
    /// Initialize with an optional path to silero_vad.onnx model.
    /// If model_path is Some and the file exists on disk, creates a sherpa-onnx VoiceActivityDetector.
    /// Otherwise, falls back to an energy-based RMS speech detector.
    pub fn new(model_path: Option<&Path>) -> Self {
        let detector = if let Some(path) = model_path
            && path.exists()
        {
            let config = VadModelConfig {
                silero_vad: SileroVadModelConfig {
                    model: Some(path.to_string_lossy().to_string()),
                    threshold: 0.5,
                    min_silence_duration: 0.5,
                    min_speech_duration: 0.25,
                    window_size: 512,
                    max_speech_duration: 30.0,
                },
                sample_rate: 16000,
                num_threads: 1,
                provider: None,
                debug: false,
                ..Default::default()
            };
            VoiceActivityDetector::create(&config, 30.0)
        } else {
            None
        };

        Self {
            detector,
            energy_threshold: 0.015,
            last_frame_detected: false,
        }
    }

    /// Feed audio waveform samples (16kHz mono).
    pub fn accept_waveform(&mut self, samples: &[f32]) {
        if let Some(ref detector) = self.detector {
            detector.accept_waveform(samples);
            self.last_frame_detected = detector.detected();
        } else {
            // RMS energy fallback
            if samples.is_empty() {
                self.last_frame_detected = false;
                return;
            }
            let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
            let rms = (sum_sq / samples.len() as f32).sqrt();
            self.last_frame_detected = rms > self.energy_threshold;
        }
    }

    /// Check if speech is currently detected in the most recent audio frame.
    pub fn is_speech_detected(&self) -> bool {
        if let Some(ref detector) = self.detector {
            detector.detected()
        } else {
            self.last_frame_detected
        }
    }

    /// Check if complete speech segments have been recognized and queued.
    pub fn has_segments(&self) -> bool {
        if let Some(ref detector) = self.detector {
            !detector.is_empty()
        } else {
            false
        }
    }

    /// Retrieve the next complete speech segment, if any.
    pub fn pop_segment(&mut self) -> Option<Vec<f32>> {
        if let Some(ref detector) = self.detector {
            if detector.is_empty() {
                None
            } else if let Some(front) = detector.front() {
                let samples = front.samples().to_vec();
                detector.pop();
                Some(samples)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Flush any remaining buffered speech into the segment queue.
    pub fn flush(&mut self) {
        if let Some(ref detector) = self.detector {
            detector.flush();
        }
    }

    /// Reset internal state and buffers.
    pub fn reset(&mut self) {
        if let Some(ref detector) = self.detector {
            detector.reset();
        }
        self.last_frame_detected = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vad_silence_rejection() {
        let mut vad = VadGate::new(None); // fallback energy mode
        let silence = vec![0.0f32; 512];

        vad.accept_waveform(&silence);
        assert!(!vad.is_speech_detected());
    }

    #[test]
    fn test_vad_low_noise_rejection() {
        let mut vad = VadGate::new(None);
        // Low amplitude noise (e.g. 0.005) below 0.015 threshold
        let noise = vec![0.005f32; 512];

        vad.accept_waveform(&noise);
        assert!(!vad.is_speech_detected());
    }

    #[test]
    fn test_vad_speech_detection() {
        let mut vad = VadGate::new(None);
        // Synthesize loud sine wave: amplitude 0.3
        let speech: Vec<f32> = (0..512)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16000.0).sin())
            .collect();

        vad.accept_waveform(&speech);
        assert!(vad.is_speech_detected());

        // Resetting clears detection
        vad.reset();
        assert!(!vad.is_speech_detected());
    }
}
