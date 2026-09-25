use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, error, info, warn};

/// Thread-safe audio frame buffer storing 16kHz mono f32 samples.
#[derive(Debug, Default)]
pub struct AudioFrameBuffer {
    samples: Mutex<Vec<f32>>,
}

impl AudioFrameBuffer {
    /// Create a new empty audio frame buffer.
    pub fn new() -> Self {
        Self {
            samples: Mutex::new(Vec::with_capacity(32000)), // 2 seconds initial capacity
        }
    }

    /// Append 16kHz mono samples into the buffer.
    pub fn push_samples(&self, incoming: &[f32]) {
        let mut lock = self.samples.lock().unwrap_or_else(|p| p.into_inner());
        lock.extend_from_slice(incoming);
    }

    /// Pop a fixed-size frame of samples (e.g. 512 samples for 32ms VAD), if available.
    pub fn pop_frame(&self, frame_size: usize) -> Option<Vec<f32>> {
        let mut lock = self.samples.lock().unwrap_or_else(|p| p.into_inner());
        if lock.len() >= frame_size {
            let frame: Vec<f32> = lock.drain(..frame_size).collect();
            Some(frame)
        } else {
            None
        }
    }

    /// Drain and return all stored samples.
    pub fn drain(&self) -> Vec<f32> {
        let mut lock = self.samples.lock().unwrap_or_else(|p| p.into_inner());
        std::mem::take(&mut *lock)
    }

    /// Clear all stored samples.
    pub fn clear(&self) {
        let mut lock = self.samples.lock().unwrap_or_else(|p| p.into_inner());
        lock.clear();
    }

    /// Return the number of buffered samples.
    pub fn len(&self) -> usize {
        self.samples.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// Return true if the buffer has no samples.
    pub fn is_empty(&self) -> bool {
        self.samples
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
    }
}

/// Helper to downmix multi-channel audio to mono and resample to 16kHz.
#[derive(Debug)]
pub struct Resampler16k {
    input_sample_rate: u32,
    channels: u16,
}

impl Resampler16k {
    pub fn new(input_sample_rate: u32, channels: u16) -> Self {
        Self {
            input_sample_rate,
            channels: channels.max(1),
        }
    }

    /// Convert interleaved multi-channel input samples at input_sample_rate to 16kHz mono.
    pub fn process(&self, interleaved_samples: &[f32]) -> Vec<f32> {
        if interleaved_samples.is_empty() {
            return Vec::new();
        }

        let ch = self.channels as usize;
        // Step 1: Downmix to mono by averaging channels
        let mono: Vec<f32> = if ch == 1 {
            interleaved_samples.to_vec()
        } else {
            interleaved_samples
                .chunks(ch)
                .map(|frame| frame.iter().sum::<f32>() / ch as f32)
                .collect()
        };

        // Step 2: Resample to 16kHz if needed
        if self.input_sample_rate == 16000 {
            mono
        } else {
            resample_linear(&mono, self.input_sample_rate, 16000)
        }
    }
}

/// Linear interpolation resampler from source_rate to target_rate.
pub fn resample_linear(input: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if input.is_empty() || source_rate == 0 || target_rate == 0 {
        return Vec::new();
    }
    if source_rate == target_rate {
        return input.to_vec();
    }

    let ratio = source_rate as f64 / target_rate as f64;
    let target_len = ((input.len() as f64) / ratio).round() as usize;
    let mut output = Vec::with_capacity(target_len);

    for i in 0..target_len {
        let src_idx = (i as f64) * ratio;
        let idx0 = src_idx.floor() as usize;
        let idx1 = (idx0 + 1).min(input.len() - 1);
        let frac = (src_idx - idx0 as f64) as f32;

        let sample = input[idx0] * (1.0 - frac) + input[idx1] * frac;
        output.push(sample);
    }

    output
}

/// Dynamic microphone capture engine with automatic device reconnection.
pub struct AudioCapture {
    buffer: Arc<AudioFrameBuffer>,
    is_running: Arc<AtomicBool>,
    device_disconnected: Arc<AtomicBool>,
    _stream: Mutex<Option<cpal::Stream>>,
}

// SAFETY: `_stream` is protected behind a `Mutex` and cpal audio stream handles can safely be
// transferred across thread boundaries within Taurine daemon's audio capture runtime.
unsafe impl Send for AudioCapture {}

// SAFETY: All fields of `AudioCapture` are protected by internal synchronization (`Arc`, `AtomicBool`, `Mutex`),
// making concurrent shared references safe across threads.
unsafe impl Sync for AudioCapture {}

impl AudioCapture {
    /// Create a new AudioCapture instance backed by the given frame buffer.
    pub fn new(buffer: Arc<AudioFrameBuffer>) -> Self {
        Self {
            buffer,
            is_running: Arc::new(AtomicBool::new(false)),
            device_disconnected: Arc::new(AtomicBool::new(false)),
            _stream: Mutex::new(None),
        }
    }

    /// Return reference to the underlying frame buffer.
    pub fn buffer(&self) -> &Arc<AudioFrameBuffer> {
        &self.buffer
    }

    /// Check if audio capture stream is actively running.
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    /// Check if the capture device reported disconnection.
    pub fn is_device_disconnected(&self) -> bool {
        self.device_disconnected.load(Ordering::Relaxed)
    }

    /// Start capturing audio from the default input device.
    pub fn start(&self) -> Result<(), String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        if self.is_running() {
            return Ok(());
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No audio input device found on the system".to_string())?;

        let device_name = device.name().unwrap_or_else(|_| "Default Device".into());
        debug!("Initializing voice capture device: {device_name}");

        let config = device
            .default_input_config()
            .map_err(|e| format!("Failed to query default input audio config: {e}"))?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let resampler = Arc::new(Resampler16k::new(sample_rate, channels));

        let buffer_clone = self.buffer.clone();
        let is_running_clone = self.is_running.clone();
        let disconnected_clone = self.device_disconnected.clone();

        let err_fn = move |err: cpal::StreamError| {
            error!("Audio stream error: {err}");
            if matches!(err, cpal::StreamError::DeviceNotAvailable) {
                warn!("Audio input device became unavailable; flagging disconnection");
                disconnected_clone.store(true, Ordering::SeqCst);
                is_running_clone.store(false, Ordering::SeqCst);
            }
        };

        let sample_format = config.sample_format();
        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let resampler_clone = resampler.clone();
                let buf_clone = buffer_clone.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let resampled = resampler_clone.process(data);
                        buf_clone.push_samples(&resampled);
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let resampler_clone = resampler.clone();
                let buf_clone = buffer_clone.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let f32_samples: Vec<f32> =
                            data.iter().map(|&s| s as f32 / 32768.0).collect();
                        let resampled = resampler_clone.process(&f32_samples);
                        buf_clone.push_samples(&resampled);
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let resampler_clone = resampler.clone();
                let buf_clone = buffer_clone.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        let f32_samples: Vec<f32> = data
                            .iter()
                            .map(|&s| (s as f32 - 32768.0) / 32768.0)
                            .collect();
                        let resampled = resampler_clone.process(&f32_samples);
                        buf_clone.push_samples(&resampled);
                    },
                    err_fn,
                    None,
                )
            }
            _ => return Err(format!("Unsupported audio sample format: {sample_format}")),
        }
        .map_err(|e| format!("Failed to build audio input stream: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Failed to start audio input stream: {e}"))?;

        *self._stream.lock().unwrap_or_else(|p| p.into_inner()) = Some(stream);
        self.is_running.store(true, Ordering::SeqCst);
        self.device_disconnected.store(false, Ordering::SeqCst);
        info!("Audio capture started at {sample_rate}Hz ({channels} ch) -> 16kHz mono");

        Ok(())
    }

    /// Attempt to recover from a disconnected device by querying the new default device.
    pub fn try_recover_device(&self) -> Result<bool, String> {
        if !self.is_device_disconnected() {
            return Ok(false);
        }

        info!("Attempting automatic audio input device recovery...");
        self.stop();

        match self.start() {
            Ok(()) => {
                info!("Successfully recovered audio input device");
                Ok(true)
            }
            Err(e) => {
                debug!("Device recovery attempt failed (no device ready): {e}");
                Ok(false)
            }
        }
    }

    /// Stop capturing audio.
    pub fn stop(&self) {
        if let Some(stream) = self
            ._stream
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            drop(stream);
        }
        self.is_running.store(false, Ordering::SeqCst);
        debug!("Audio capture stopped");
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_frame_buffer_push_pop_drain() {
        let buffer = AudioFrameBuffer::new();
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);

        let input_samples = vec![0.1f32; 1024];
        buffer.push_samples(&input_samples);
        assert_eq!(buffer.len(), 1024);
        assert!(!buffer.is_empty());

        let frame = buffer.pop_frame(512).expect("expected 512 frame");
        assert_eq!(frame.len(), 512);
        assert_eq!(buffer.len(), 512);

        let remaining = buffer.drain();
        assert_eq!(remaining.len(), 512);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_resampler_mono_passthrough() {
        let resampler = Resampler16k::new(16000, 1);
        let input = vec![0.5; 320];
        let output = resampler.process(&input);
        assert_eq!(output.len(), 320);
        assert_eq!(output[0], 0.5);
    }

    #[test]
    fn test_resampler_stereo_downmix() {
        let resampler = Resampler16k::new(16000, 2);
        // Interleaved stereo: [L0, R0, L1, R1]
        let input = vec![0.2, 0.4, 0.6, 0.8];
        let output = resampler.process(&input);
        assert_eq!(output.len(), 2);
        assert!((output[0] - 0.3).abs() < 1e-6);
        assert!((output[1] - 0.7).abs() < 1e-6);
    }

    #[test]
    fn test_resampler_48k_to_16k() {
        let resampler = Resampler16k::new(48000, 1);
        // 48000 samples = 1 second -> should become ~16000 samples
        let input = vec![0.1; 4800]; // 0.1 second
        let output = resampler.process(&input);
        assert_eq!(output.len(), 1600);
    }
}
