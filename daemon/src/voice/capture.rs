use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, error, info, warn};

/// Maximum buffered samples (5 minutes of 16kHz mono audio = 4,800,000 samples).
pub const MAX_BUFFER_SAMPLES: usize = 16_000 * 60 * 5;

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

    /// Append 16kHz mono samples into the buffer, discarding oldest samples if capacity exceeds `MAX_BUFFER_SAMPLES`.
    pub fn push_samples(&self, incoming: &[f32]) {
        let mut lock = self.samples.lock().unwrap_or_else(|p| p.into_inner());
        let total = lock.len() + incoming.len();
        if total > MAX_BUFFER_SAMPLES {
            let overflow = total - MAX_BUFFER_SAMPLES;
            let drain_count = overflow.min(lock.len());
            lock.drain(..drain_count);
        }
        lock.extend_from_slice(incoming);
    }

    /// Prepend 16kHz mono samples to the front of the buffer (e.g. to restore boundary audio context).
    pub fn prepend_samples(&self, incoming: &[f32]) {
        if incoming.is_empty() {
            return;
        }
        let mut lock = self.samples.lock().unwrap_or_else(|p| p.into_inner());
        let mut new_samples = Vec::with_capacity(lock.len() + incoming.len());
        new_samples.extend_from_slice(incoming);
        new_samples.extend_from_slice(&lock);
        if new_samples.len() > MAX_BUFFER_SAMPLES {
            new_samples.truncate(MAX_BUFFER_SAMPLES);
        }
        *lock = new_samples;
    }

    /// Peek at the most recent `n` samples in the buffer without draining them.
    pub fn peek_tail(&self, n: usize) -> Vec<f32> {
        let lock = self.samples.lock().unwrap_or_else(|p| p.into_inner());
        if lock.is_empty() || n == 0 {
            Vec::new()
        } else {
            let start = lock.len().saturating_sub(n);
            lock[start..].to_vec()
        }
    }

    /// Pop a fixed-size frame of samples (e.g. 512 samples for 32ms), if available.
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

/// 80 Hz 2nd-order IIR Butterworth high-pass filter (16 kHz).
///
/// Attenuates DC offset, desk bumps, AC mains hum, and HVAC rumble below
/// human vocal fundamentals (85-255 Hz). Coefficients follow the RBJ cookbook
/// high-pass with Q = 1/sqrt(2) (Butterworth), computed in `new`.
#[derive(Debug, Clone)]
pub struct HighPassFilter80Hz {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl HighPassFilter80Hz {
    /// Create a new filter with Direct Form I state zeroed.
    pub fn new() -> Self {
        let sample_rate = 16_000.0f64;
        let cutoff = 80.0f64;
        // Q = 1/sqrt(2) for a Butterworth response.
        let q = 1.0f64 / std::f64::consts::SQRT_2;
        let w0 = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);
        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        Self {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (a1 / a0) as f32,
            a2: (a2 / a0) as f32,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Filter samples in place, preserving state across calls for stream continuity.
    pub fn process_in_place(&mut self, samples: &mut [f32]) {
        for s in samples.iter_mut() {
            let x0 = *s;
            let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = x0;
            self.y2 = self.y1;
            self.y1 = y0;
            *s = y0;
        }
    }

    /// Reset delay state (e.g. after device reconnect).
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

impl Default for HighPassFilter80Hz {
    fn default() -> Self {
        Self::new()
    }
}

/// Soft-knee saturation limiter.
///
/// For |x| <= 0.75 (-2.5 dBFS), the response is exactly linear: y = x.
/// Above 0.75, a smooth tanh curve compresses peaks smoothly up to a maximum
/// theoretical ceiling of 0.98, maintaining C1 continuity and eliminating hard-clipping harmonics:
/// y = sign(x) * (0.75 + 0.23 * tanh((|x| - 0.75) / 0.23))
#[inline]
pub fn soft_knee_limit(sample: f32) -> f32 {
    let abs_s = sample.abs();
    if abs_s <= 0.75 {
        sample
    } else {
        let sign = sample.signum();
        sign * (0.75 + 0.23 * ((abs_s - 0.75) / 0.23).tanh())
    }
}

/// Normalize a speech snippet to -20 dBFS RMS with a +15 dB maximum gain cap
/// and soft-knee peak limiting below 0.98.
///
/// Near-silence (RMS below 1e-6) is returned unchanged so idle noise is never
/// pumped. Returns a new vector; input is untouched.
pub fn normalize_snippet_rms(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    const TARGET_DBFS: f32 = -20.0;
    const MAX_GAIN_DB: f32 = 15.0;
    const SILENCE_FLOOR: f32 = 1e-6;
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    let rms = (sum_sq / samples.len() as f32).sqrt();
    if !rms.is_finite() || rms < SILENCE_FLOOR {
        return samples.to_vec();
    }
    let target_rms = 10.0f32.powf(TARGET_DBFS / 20.0);
    let max_gain = 10.0f32.powf(MAX_GAIN_DB / 20.0);
    let gain = (target_rms / rms).min(max_gain);
    samples.iter().map(|s| soft_knee_limit(s * gain)).collect()
}

/// Slices an utterance buffer preserving a post-roll cushion of trailing consonant decay.
///
/// If an utterance ends due to silence (>8 frames = >256ms), we retain exactly 8 frames
/// (~256ms / 4,096 samples at 16kHz) of trailing decay so trailing consonants ("-der", "t", "k")
/// are never clipped, while trimming excess dead silence beyond 8 frames.
pub fn slice_utterance_with_postroll(buffer: &[f32], silence_frames: usize) -> Vec<f32> {
    const POST_ROLL_FRAMES: usize = 8;
    const FRAME_SAMPLES: usize = 512;
    let excess_samples = silence_frames.saturating_sub(POST_ROLL_FRAMES) * FRAME_SAMPLES;
    if excess_samples > 0 && buffer.len() > excess_samples {
        buffer[..buffer.len() - excess_samples].to_vec()
    } else {
        buffer.to_vec()
    }
}

/// Counts trailing 512-sample frames whose RMS energy falls below speech level.
///
/// Used by Push-to-Talk and Hands-Free completion to measure trailing dead air
/// before applying [`slice_utterance_with_postroll`].
pub fn trailing_silence_frames(samples: &[f32]) -> usize {
    const FRAME_SAMPLES: usize = 512;
    const SILENCE_RMS: f32 = 1e-3;
    if samples.is_empty() {
        return 0;
    }
    let mut silence_frames = 0;
    let mut idx = samples.len();
    while idx >= FRAME_SAMPLES {
        idx -= FRAME_SAMPLES;
        let frame = &samples[idx..idx + FRAME_SAMPLES];
        let mean_sq = frame.iter().map(|s| s * s).sum::<f32>() / FRAME_SAMPLES as f32;
        if mean_sq.sqrt() < SILENCE_RMS {
            silence_frames += 1;
        } else {
            break;
        }
    }
    silence_frames
}

struct RubatoResamplerState {
    resampler: rubato::FftFixedIn<f32>,
    fifo: Vec<f32>,
}

/// Helper to downmix multi-channel audio to mono and resample to 16kHz with anti-aliasing.
pub struct Resampler16k {
    input_sample_rate: u32,
    channels: u16,
    rubato: Option<Mutex<RubatoResamplerState>>,
    high_pass: Mutex<HighPassFilter80Hz>,
}

impl std::fmt::Debug for Resampler16k {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resampler16k")
            .field("input_sample_rate", &self.input_sample_rate)
            .field("channels", &self.channels)
            .field("has_rubato", &self.rubato.is_some())
            .finish()
    }
}

impl Resampler16k {
    pub fn new(input_sample_rate: u32, channels: u16) -> Self {
        let rubato = if input_sample_rate != 16000 && input_sample_rate > 0 {
            match rubato::FftFixedIn::<f32>::new(input_sample_rate as usize, 16000, 1024, 2, 1) {
                Ok(r) => Some(Mutex::new(RubatoResamplerState {
                    resampler: r,
                    fifo: Vec::with_capacity(4096),
                })),
                Err(e) => {
                    warn!(
                        "Failed to initialize Rubato band-limited resampler for {input_sample_rate}Hz: {e}; falling back to linear"
                    );
                    None
                }
            }
        } else {
            None
        };

        Self {
            input_sample_rate,
            channels: channels.max(1),
            rubato,
            high_pass: Mutex::new(HighPassFilter80Hz::new()),
        }
    }

    /// Convert interleaved multi-channel input samples at input_sample_rate to 16kHz mono.
    ///
    /// Downmixes, band-limited resamples (anti-aliasing filter), then applies the 80 Hz
    /// high-pass filter so downstream dictation (PTT, Hands-Free) receives clean audio.
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

        // Step 2: Resample to 16kHz with band-limited sinc/FFT anti-aliasing
        let mut out = if self.input_sample_rate == 16000 {
            mono
        } else if let Some(ref state_mutex) = self.rubato {
            if let Ok(mut state) = state_mutex.lock() {
                use rubato::Resampler;
                state.fifo.extend_from_slice(&mono);
                let needed = state.resampler.input_frames_next();
                let mut resampled_chunks = Vec::new();
                while state.fifo.len() >= needed {
                    let chunk: Vec<f32> = state.fifo.drain(..needed).collect();
                    match state.resampler.process(&[&chunk], None) {
                        Ok(mut output_channels) => {
                            if let Some(chan) = output_channels.get_mut(0) {
                                resampled_chunks.append(chan);
                            }
                        }
                        Err(e) => {
                            warn!("Rubato resampling error: {e}");
                            break;
                        }
                    }
                }
                resampled_chunks
            } else {
                resample_linear(&mono, self.input_sample_rate, 16000)
            }
        } else {
            resample_linear(&mono, self.input_sample_rate, 16000)
        };

        // Step 3: 80 Hz high-pass rumble/DC removal at 16 kHz.
        if !out.is_empty()
            && let Ok(mut hp) = self.high_pass.lock()
        {
            hp.process_in_place(&mut out);
        }
        out
    }

    /// Reset high-pass and resampler state (e.g. after device reconnect).
    pub fn reset_filter(&self) {
        if let Ok(mut hp) = self.high_pass.lock() {
            hp.reset();
        }
        if let Some(ref state_mutex) = self.rubato
            && let Ok(mut state) = state_mutex.lock()
        {
            use rubato::Resampler;
            state.fifo.clear();
            state.resampler.reset();
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

#[cfg(windows)]
#[allow(non_snake_case, clippy::upper_case_acronyms)]
mod win_com {
    use std::ffi::c_void;

    pub const CLSID_MMDEVICE_ENUMERATOR: windows_sys::core::GUID = windows_sys::core::GUID {
        data1: 0xBCDE0395,
        data2: 0xE52F,
        data3: 0x467C,
        data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
    };

    pub const IID_IMMDEVICE_ENUMERATOR: windows_sys::core::GUID = windows_sys::core::GUID {
        data1: 0xA95664D2,
        data2: 0x9614,
        data3: 0x4F35,
        data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
    };

    #[repr(C)]
    pub struct PROPERTYKEY {
        pub fmtid: windows_sys::core::GUID,
        pub pid: u32,
    }

    // PKEY_Device_FriendlyName: {a45c254e-df1c-4efd-8020-67d146a850e0}, 14
    pub const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows_sys::core::GUID {
            data1: 0xa45c254e,
            data2: 0xdf1c,
            data3: 0x4efd,
            data4: [0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0],
        },
        pid: 14,
    };

    #[repr(C)]
    pub struct IUnknownVtbl {
        pub QueryInterface: unsafe extern "system" fn(
            *mut c_void,
            *const windows_sys::core::GUID,
            *mut *mut c_void,
        ) -> windows_sys::core::HRESULT,
        pub AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
        pub Release: unsafe extern "system" fn(*mut c_void) -> u32,
    }

    #[repr(C)]
    pub struct IMMDeviceEnumeratorVtbl {
        pub base: IUnknownVtbl,
        pub EnumAudioEndpoints: unsafe extern "system" fn(
            *mut c_void,
            i32,
            u32,
            *mut *mut c_void,
        ) -> windows_sys::core::HRESULT,
        pub GetDefaultAudioEndpoint: unsafe extern "system" fn(
            *mut c_void,
            i32,
            i32,
            *mut *mut c_void,
        )
            -> windows_sys::core::HRESULT,
        pub GetDevice: unsafe extern "system" fn(
            *mut c_void,
            *const u16,
            *mut *mut c_void,
        ) -> windows_sys::core::HRESULT,
        pub RegisterEndpointNotificationCallback:
            unsafe extern "system" fn(*mut c_void, *mut c_void) -> windows_sys::core::HRESULT,
        pub UnregisterEndpointNotificationCallback:
            unsafe extern "system" fn(*mut c_void, *mut c_void) -> windows_sys::core::HRESULT,
    }

    #[repr(C)]
    pub struct IMMDeviceVtbl {
        pub base: IUnknownVtbl,
        pub Activate: unsafe extern "system" fn(
            *mut c_void,
            *const windows_sys::core::GUID,
            u32,
            *mut c_void,
            *mut *mut c_void,
        ) -> windows_sys::core::HRESULT,
        pub OpenPropertyStore: unsafe extern "system" fn(
            *mut c_void,
            u32,
            *mut *mut c_void,
        ) -> windows_sys::core::HRESULT,
        pub GetId:
            unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> windows_sys::core::HRESULT,
        pub GetState:
            unsafe extern "system" fn(*mut c_void, *mut u32) -> windows_sys::core::HRESULT,
    }

    #[repr(C)]
    pub struct IPropertyStoreVtbl {
        pub base: IUnknownVtbl,
        pub GetCount:
            unsafe extern "system" fn(*mut c_void, *mut u32) -> windows_sys::core::HRESULT,
        pub GetAt:
            unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> windows_sys::core::HRESULT,
        pub GetValue: unsafe extern "system" fn(
            *mut c_void,
            *const PROPERTYKEY,
            *mut PROPVARIANT,
        ) -> windows_sys::core::HRESULT,
        pub SetValue: unsafe extern "system" fn(
            *mut c_void,
            *const PROPERTYKEY,
            *const PROPVARIANT,
        ) -> windows_sys::core::HRESULT,
        pub Commit: unsafe extern "system" fn(*mut c_void) -> windows_sys::core::HRESULT,
    }

    #[repr(C)]
    pub struct PROPVARIANT {
        pub vt: u16,
        pub w_reserved1: u16,
        pub w_reserved2: u16,
        pub w_reserved3: u16,
        pub ptr_or_val: *const u16,
        pub _pad: [u8; 8],
    }
}

/// Query the system default communications audio capture device name on Windows.
///
/// On Windows, querying the `eCommunications` role endpoint allows applications to
/// route microphone input through hardware acoustic echo cancellation (AEC) and
/// multi-mic directional beamforming supported by modern sound hardware drivers.
#[cfg(windows)]
pub fn query_default_communications_device_name() -> Option<String> {
    use std::ffi::c_void;
    use win_com::*;
    use windows_sys::Win32::System::Com::*;

    const E_CAPTURE: i32 = 1;
    const E_COMMUNICATIONS: i32 = 2;
    const STGM_READ: u32 = 0;
    const VT_LPWSTR: u16 = 31;

    // SAFETY: We initialize COM on this thread (multithreaded), instantiate the system
    // MMDeviceEnumerator class via CoCreateInstance, query the eCommunications default
    // audio capture endpoint via COM vtable dispatch, read the friendly name property from
    // the property store, convert the UTF-16 slice to an owned String, and release all
    // allocated COM interface pointers and memory.
    unsafe {
        let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED as u32);

        let mut enumerator_raw: *mut c_void = std::ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_MMDEVICE_ENUMERATOR,
            std::ptr::null_mut(),
            CLSCTX_ALL,
            &IID_IMMDEVICE_ENUMERATOR,
            &mut enumerator_raw,
        );
        if hr < 0 || enumerator_raw.is_null() {
            return None;
        }

        let enumerator_vtbl = *(enumerator_raw as *mut *const IMMDeviceEnumeratorVtbl);
        let mut device_raw: *mut c_void = std::ptr::null_mut();
        let hr = ((*enumerator_vtbl).GetDefaultAudioEndpoint)(
            enumerator_raw,
            E_CAPTURE,
            E_COMMUNICATIONS,
            &mut device_raw,
        );
        ((*enumerator_vtbl).base.Release)(enumerator_raw);
        if hr < 0 || device_raw.is_null() {
            return None;
        }

        let device_vtbl = *(device_raw as *mut *const IMMDeviceVtbl);
        let mut store_raw: *mut c_void = std::ptr::null_mut();
        let hr = ((*device_vtbl).OpenPropertyStore)(device_raw, STGM_READ, &mut store_raw);
        ((*device_vtbl).base.Release)(device_raw);
        if hr < 0 || store_raw.is_null() {
            return None;
        }

        let store_vtbl = *(store_raw as *mut *const IPropertyStoreVtbl);
        let mut pv: PROPVARIANT = std::mem::zeroed();
        let hr = ((*store_vtbl).GetValue)(store_raw, &PKEY_DEVICE_FRIENDLY_NAME, &mut pv);
        ((*store_vtbl).base.Release)(store_raw);
        if hr < 0 {
            return None;
        }

        if pv.vt == VT_LPWSTR && !pv.ptr_or_val.is_null() {
            let ptr = pv.ptr_or_val;
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let s = String::from_utf16(slice).ok();
            CoTaskMemFree(ptr as *const c_void);
            s
        } else {
            None
        }
    }
}

/// Query the system default communications audio capture device name on non-Windows platforms (stub).
#[cfg(not(windows))]
pub fn query_default_communications_device_name() -> Option<String> {
    None
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

    #[cfg(test)]
    pub fn set_running_for_test(&self, running: bool) {
        self.is_running.store(running, Ordering::Relaxed);
    }

    /// Check if the capture device reported disconnection.
    pub fn is_device_disconnected(&self) -> bool {
        self.device_disconnected.load(Ordering::Relaxed)
    }

    /// Enumerate all unique audio input device names currently available on the system.
    pub fn list_input_devices() -> Vec<String> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let mut names = Vec::new();
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                if let Ok(name) = device.name()
                    && !names.contains(&name)
                {
                    names.push(name);
                }
            }
        }
        names
    }

    /// Resolve the appropriate audio input device: checks configured `voice_input_device`
    /// against available input devices (case-insensitive substring match).
    /// Falls back to system default input device if unset or not found.
    pub fn resolve_input_device(host: &cpal::Host) -> Result<cpal::Device, String> {
        use cpal::traits::{DeviceTrait, HostTrait};

        if let Some(configured_name) = taurine_core::settings::get_cached_voice_input_device()
            && !configured_name.trim().is_empty()
        {
            let configured_lower = configured_name.trim().to_lowercase();
            if let Ok(devices) = host.input_devices() {
                for d in devices {
                    if let Ok(name) = d.name() {
                        let name_lower = name.to_lowercase();
                        if name_lower == configured_lower || name_lower.contains(&configured_lower)
                        {
                            return Ok(d);
                        }
                    }
                }
            }
            warn!(
                "Configured voice input device '{configured_name}' not found; falling back to default device"
            );
        }

        // On Windows, if no explicit device is configured, try querying the system default
        // communications endpoint to engage hardware AEC and beamforming.
        if let Some(comm_name) = query_default_communications_device_name() {
            debug!("Attempting to match Windows Communications device: '{comm_name}'");
            let comm_lower = comm_name.trim().to_lowercase();
            if let Ok(devices) = host.input_devices() {
                for d in devices {
                    if let Ok(name) = d.name() {
                        let name_lower = name.to_lowercase();
                        if name_lower == comm_lower
                            || name_lower.contains(&comm_lower)
                            || comm_lower.contains(&name_lower)
                        {
                            info!(
                                "Using Windows Communications audio device: '{name}' (Hardware AEC & Beamforming enabled)"
                            );
                            return Ok(d);
                        }
                    }
                }
            }
        }

        host.default_input_device()
            .ok_or_else(|| "No audio input device found on the system".to_string())
    }

    /// Restart the audio stream if it is currently running (e.g. after changing input device).
    pub fn restart(&self) -> Result<(), String> {
        let was_running = self.is_running();
        self.stop();
        if was_running {
            self.start()?;
        }
        Ok(())
    }

    /// Start capturing audio from the configured or default input device.
    pub fn start(&self) -> Result<(), String> {
        // Hermetic tests never open the host microphone.
        if cfg!(test) {
            self.is_running.store(true, Ordering::SeqCst);
            return Ok(());
        }
        use cpal::traits::{DeviceTrait, StreamTrait};

        let mut stream_guard = self._stream.lock().unwrap_or_else(|p| p.into_inner());
        if stream_guard.is_some() || self.is_running() {
            return Ok(());
        }

        let host = cpal::default_host();
        let device = Self::resolve_input_device(&host)?;

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

        *stream_guard = Some(stream);
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
    fn test_audio_frame_buffer_peek_and_prepend() {
        let buffer = AudioFrameBuffer::new();
        buffer.push_samples(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(buffer.peek_tail(2), vec![3.0, 4.0]);
        assert_eq!(buffer.peek_tail(10), vec![1.0, 2.0, 3.0, 4.0]);

        // Prepend samples
        buffer.prepend_samples(&[0.5, 0.75]);
        assert_eq!(buffer.len(), 6);
        let drained = buffer.drain();
        assert_eq!(drained, vec![0.5, 0.75, 1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_audio_frame_buffer_overflow_cap() {
        let buffer = AudioFrameBuffer::new();
        // Push MAX_BUFFER_SAMPLES
        let chunk = vec![0.5f32; 100_000];
        for _ in 0..48 {
            buffer.push_samples(&chunk);
        }
        assert_eq!(buffer.len(), 4_800_000);

        // Push another 10,000 samples — total length must still be capped at MAX_BUFFER_SAMPLES
        let extra = vec![1.0f32; 10_000];
        buffer.push_samples(&extra);
        assert_eq!(buffer.len(), MAX_BUFFER_SAMPLES);
    }

    #[test]
    fn test_resampler_mono_passthrough() {
        // In-band 1 kHz tone passes the high-pass filter ~unity; length preserved.
        let resampler = Resampler16k::new(16000, 1);
        let input = sine_wave(1000.0, 0.5, 320);
        let output = resampler.process(&input);
        assert_eq!(output.len(), 320);
        assert!(rms(&output) / rms(&input) > 0.9);
    }

    #[test]
    fn test_resampler_stereo_downmix() {
        // Same-phase tones average arithmetically before high-pass filtering.
        // Steady state only: skip the first 200 samples of filter warmup.
        let resampler = Resampler16k::new(16000, 2);
        let left = sine_wave(1000.0, 0.2, 1600);
        let right = sine_wave(1000.0, 0.4, 1600);
        let mut input = Vec::with_capacity(3200);
        for (l, r) in left.iter().zip(right.iter()) {
            input.push(*l);
            input.push(*r);
        }
        let output = resampler.process(&input);
        assert_eq!(output.len(), 1600);
        // The high-pass filter shifts phase slightly, so compare energy levels,
        // not samples: averaged 0.2/0.4 tones must carry 0.3-tone energy.
        let expected = sine_wave(1000.0, 0.3, 1600);
        let ratio = rms(&output[200..]) / rms(&expected[200..]);
        assert!(
            (ratio - 1.0).abs() < 0.05,
            "downmix energy ratio was {ratio}"
        );
    }

    #[test]
    fn test_resampler_48k_to_16k() {
        let resampler = Resampler16k::new(48000, 1);
        // 48000 samples = 1 second -> should produce ~16000 samples
        let input = vec![0.1; 48000];
        let output = resampler.process(&input);
        assert!(
            output.len() >= 15000 && output.len() <= 16500,
            "len was {}",
            output.len()
        );
    }

    fn sine_wave(freq_hz: f32, amplitude: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * freq_hz * i as f32 / 16000.0).sin())
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn test_high_pass_filter_attenuates_low_frequencies() {
        // 50 Hz rumble/hum must be attenuated; 1 kHz speech band must pass.
        // Steady state only: skip the first 4000 samples of filter warmup.
        let low = sine_wave(50.0, 0.5, 16000);
        let mut hp = HighPassFilter80Hz::new();
        let mut low_out = low.clone();
        hp.process_in_place(&mut low_out);
        let low_ratio = rms(&low_out[4000..]) / rms(&low[4000..]);
        assert!(low_ratio < 0.5, "50 Hz ratio was {low_ratio}");

        let speech = sine_wave(1000.0, 0.5, 16000);
        let mut hp = HighPassFilter80Hz::new();
        let mut speech_out = speech.clone();
        hp.process_in_place(&mut speech_out);
        let speech_ratio = rms(&speech_out[4000..]) / rms(&speech[4000..]);
        assert!(speech_ratio > 0.9, "1 kHz ratio was {speech_ratio}");
    }

    #[test]
    fn test_snippet_rms_normalization() {
        // Quiet speech (-29 dBFS) is brought up to -20 dBFS.
        let quiet = sine_wave(440.0, 0.05, 16000);
        let normed = normalize_snippet_rms(&quiet);
        assert_eq!(normed.len(), quiet.len());
        let normed_rms = rms(&normed);
        assert!(
            (normed_rms - 0.1).abs() < 0.005,
            "normalized RMS was {normed_rms}"
        );

        // Transient peaks never exceed soft-knee clipping ceiling (<= 0.98).
        let mut sparse = vec![0.0f32; 1600];
        sparse[1599] = 0.5;
        let limited = normalize_snippet_rms(&sparse);
        let peak = limited.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.75 && peak <= 0.98, "peak was {peak}");
    }

    #[test]
    fn test_slice_utterance_with_postroll_preserves_cushion() {
        // Speech + 14 frames of silence (7168 samples)
        let speech_len = 5120; // 10 frames of speech
        let silence_frames = 14;
        let silence_len = silence_frames * 512; // 7168 samples
        let total_len = speech_len + silence_len;
        let buffer = vec![0.5f32; total_len];

        let sliced = slice_utterance_with_postroll(&buffer, silence_frames);
        // Post-roll cushion must preserve exactly 8 frames (4096 samples / ~256ms) of trailing decay,
        // trimming the excess 6 frames (3072 samples) of dead silence.
        let expected_len = speech_len + (8 * 512); // 5120 + 4096 = 9216
        assert_eq!(sliced.len(), expected_len);

        // If silence frames <= 8, no trimming is performed (entire cushion is preserved)
        let short_silence_frames = 5;
        let short_total = speech_len + (short_silence_frames * 512);
        let short_buffer = vec![0.5f32; short_total];
        let short_sliced = slice_utterance_with_postroll(&short_buffer, short_silence_frames);
        assert_eq!(short_sliced.len(), short_total);
    }

    #[test]
    fn test_trailing_silence_frames_counts_quiet_tail() {
        let mut samples = vec![0.5f32; 5120];
        samples.extend(vec![0.0f32; 512 * 10]);
        assert_eq!(trailing_silence_frames(&samples), 10);

        let speech_only = vec![0.5f32; 5120];
        assert_eq!(trailing_silence_frames(&speech_only), 0);
        assert_eq!(trailing_silence_frames(&[]), 0);
    }

    #[test]
    fn test_soft_knee_limiter_smoothness_and_ceiling() {
        // 1. Below 0.75 threshold: exact linear response
        assert_eq!(soft_knee_limit(0.0), 0.0);
        assert_eq!(soft_knee_limit(0.5), 0.5);
        assert_eq!(soft_knee_limit(-0.5), -0.5);
        assert_eq!(soft_knee_limit(0.75), 0.75);
        assert_eq!(soft_knee_limit(-0.75), -0.75);

        // 2. Above threshold: smooth compression strictly capped at or below 0.98
        let out_1 = soft_knee_limit(1.0);
        assert!(out_1 > 0.75 && out_1 <= 0.98, "out_1 was {out_1}");
        let out_extreme = soft_knee_limit(100.0);
        assert!(
            out_extreme <= 0.98 && out_extreme > 0.97,
            "out_extreme was {out_extreme}"
        );
        let out_neg_extreme = soft_knee_limit(-100.0);
        assert!(
            (-0.98..-0.97).contains(&out_neg_extreme),
            "out_neg_extreme was {out_neg_extreme}"
        );

        // 3. Monotonically increasing
        assert!(soft_knee_limit(0.8) < soft_knee_limit(0.9));
        assert!(soft_knee_limit(0.9) < soft_knee_limit(1.2));
    }

    #[test]
    #[cfg(windows)]
    fn test_windows_communications_device_query() {
        let result = query_default_communications_device_name();
        if let Some(ref name) = result {
            assert!(!name.is_empty(), "device name was empty");
        }
    }

    #[test]
    fn test_rubato_resampler_48k_to_16k_anti_aliasing() {
        let resampler = Resampler16k::new(48000, 1);
        // Create 48kHz signal: 1000 Hz sine (in-band) + 12000 Hz sine (out-of-band noise)
        let sample_rate = 48000.0f32;
        let num_samples = 48000; // 1 second
        let mut input = Vec::with_capacity(num_samples);
        for i in 0..num_samples {
            let t = i as f32 / sample_rate;
            let in_band = (2.0 * std::f32::consts::PI * 1000.0 * t).sin();
            let out_of_band = (2.0 * std::f32::consts::PI * 12000.0 * t).sin();
            input.push(in_band + out_of_band);
        }

        // Process in realistic CPAL chunks of 480 samples (10ms each)
        let mut output = Vec::new();
        for chunk in input.chunks(480) {
            output.extend(resampler.process(chunk));
        }

        // Total output length should be approximately 16000 samples (+/- a buffer frame)
        assert!(
            output.len() >= 15000 && output.len() <= 17000,
            "len was {}",
            output.len()
        );

        // Measure energy of the 12 kHz alias in the 16 kHz output (aliases to 4 kHz).
        let steady = &output[2000..14000];
        let mut corr_4k = 0.0f32;
        let mut corr_1k = 0.0f32;
        for (idx, &s) in steady.iter().enumerate() {
            let t = idx as f32 / 16000.0;
            corr_4k += s * (2.0 * std::f32::consts::PI * 4000.0 * t).sin();
            corr_1k += s * (2.0 * std::f32::consts::PI * 1000.0 * t).sin();
        }
        let ratio = corr_4k.abs() / corr_1k.abs();
        // With rubato anti-aliasing filter, the 12kHz tone aliasing into 4kHz is suppressed below 5% of 1kHz signal.
        assert!(ratio < 0.05, "alias ratio was {ratio}");
    }
}
