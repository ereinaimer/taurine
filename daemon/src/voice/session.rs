use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use taurine_core::voice::{Transcriber, VoiceDictionary, format_transcript};

use super::capture::AudioCapture;
use super::factory::create_transcriber;
use super::modes::VoiceMode;

/// Minimal energy threshold to consider audio frames as containing speech.
const MIN_SPEECH_RMS_ENERGY: f32 = 1e-7;

/// Minimum audio samples required for transcription (100ms at 16kHz = 1600 samples).
const MIN_SAMPLES_COUNT: usize = 1600;

/// Default inactivity timeout before warm transcriber is dropped from RAM.
const DEFAULT_INACTIVITY_TTL: Duration = Duration::from_secs(20);

struct EngineSlot {
    transcriber: Box<dyn Transcriber>,
    last_used: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineMode {
    Auto,
    Pinned(String),
}

/// Manages the lifecycle of voice recording, dictation sessions, and text injection.
///
/// Keeps the speech recognition model warm in RAM for sub-100ms response during active dictation,
/// and unloads it after 20 seconds of inactivity to preserve system RAM when idle.
pub struct VoiceSessionManager {
    mode: Mutex<VoiceMode>,
    capture: Arc<AudioCapture>,
    paused: Arc<AtomicBool>,
    model_name: Mutex<String>,
    models_dir: Option<PathBuf>,
    dictionary: Mutex<VoiceDictionary>,
    /// When true the shared AudioCapture stream is restarted after each PTT/HandsFree
    /// session ends so that the AlwaysOnVoiceListener ambient loop can continue feeding.
    always_on_enabled: Arc<AtomicBool>,
    light: Arc<Mutex<Option<EngineSlot>>>,
    quality: Arc<Mutex<Option<EngineSlot>>>,
    generation: Arc<AtomicU64>,
    engine_mode: Mutex<EngineMode>,
    ttl: Duration,
}

impl VoiceSessionManager {
    /// Create a new session manager with an audio capture interface and pause flag.
    pub fn new(capture: Arc<AudioCapture>, paused: Arc<AtomicBool>) -> Self {
        let models_dir = Some(taurine_core::system::paths::ensure_data_dir().join("models"));
        Self {
            mode: Mutex::new(VoiceMode::Idle),
            capture,
            paused,
            model_name: Mutex::new("auto".to_string()),
            models_dir,
            dictionary: Mutex::new(VoiceDictionary::default()),
            always_on_enabled: Arc::new(AtomicBool::new(false)),
            light: Arc::new(Mutex::new(None)),
            quality: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
            engine_mode: Mutex::new(EngineMode::Auto),
            ttl: DEFAULT_INACTIVITY_TTL,
        }
    }

    /// Mark this session manager as backing an always-on ambient listener.
    ///
    /// When enabled, the AudioCapture stream is automatically restarted after every
    /// PTT, HandsFree, or Escape session ends so the ambient loop can continue feeding.
    pub fn set_always_on(&self, enabled: bool) {
        self.always_on_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Builder method to specify the voice model name.
    pub fn with_model_name(self, name: impl Into<String>) -> Self {
        let name_str = name.into();
        let mode = if name_str.trim().eq_ignore_ascii_case("auto") {
            EngineMode::Auto
        } else {
            let canonical = taurine_core::voice::resolve_model_alias(&name_str);
            EngineMode::Pinned(canonical.to_string())
        };
        *self.model_name.lock().unwrap_or_else(|p| p.into_inner()) = name_str;
        *self.engine_mode.lock().unwrap_or_else(|p| p.into_inner()) = mode;
        self
    }

    /// Builder method to override the models directory.
    pub fn with_models_dir(mut self, dir: PathBuf) -> Self {
        self.models_dir = Some(dir);
        self
    }

    /// Builder method to configure custom vocabulary dictionary.
    pub fn with_dictionary(self, dict: VoiceDictionary) -> Self {
        *self.dictionary.lock().unwrap_or_else(|p| p.into_inner()) = dict;
        self
    }

    /// Builder method to customize inactivity TTL.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Get current operating mode.
    pub fn current_mode(&self) -> VoiceMode {
        *self.mode.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Returns true if a dictation session is actively capturing or processing.
    pub fn is_active(&self) -> bool {
        self.current_mode().is_active()
    }

    /// Returns true if Taurine is globally paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Update the STT model name. If the model changed, clears the warm cache.
    pub fn set_model_name(&self, name: impl Into<String>) {
        let new_name = name.into();
        let mut model = self.model_name.lock().unwrap_or_else(|p| p.into_inner());
        let mut mode = self.engine_mode.lock().unwrap_or_else(|p| p.into_inner());
        let new_mode = if new_name.trim().eq_ignore_ascii_case("auto") {
            EngineMode::Auto
        } else {
            let canonical = taurine_core::voice::resolve_model_alias(&new_name);
            EngineMode::Pinned(canonical.to_string())
        };

        if *model != new_name || *mode != new_mode {
            *model = new_name;
            *mode = new_mode;
            self.generation.fetch_add(1, Ordering::SeqCst);
            let mut quality = self.quality.lock().unwrap_or_else(|p| p.into_inner());
            let mut light = self.light.lock().unwrap_or_else(|p| p.into_inner());
            *quality = None;
            *light = None;
        }
    }

    /// Update personal dictionary rules.
    pub fn set_dictionary(&self, dict: VoiceDictionary) {
        *self.dictionary.lock().unwrap_or_else(|p| p.into_inner()) = dict;
    }

    /// Unloads the warm model from RAM if it has exceeded the inactivity TTL.
    pub fn clean_expired_transcriber(&self) {
        let mut quality = self.quality.lock().unwrap_or_else(|p| p.into_inner());
        let mut light = self.light.lock().unwrap_or_else(|p| p.into_inner());
        let mut unloaded = false;
        if let Some(ref slot) = *quality
            && slot.last_used.elapsed() > self.ttl
        {
            *quality = None;
            unloaded = true;
        }
        if let Some(ref slot) = *light
            && slot.last_used.elapsed() > self.ttl
        {
            *light = None;
            unloaded = true;
        }
        if unloaded {
            debug!("VoiceSessionManager: unloading idle voice model(s) from RAM (TTL exceeded)");
        }
    }

    /// Immediately unloads the warm model from RAM.
    pub fn unload_model(&self) {
        let mut quality = self.quality.lock().unwrap_or_else(|p| p.into_inner());
        let mut light = self.light.lock().unwrap_or_else(|p| p.into_inner());
        *quality = None;
        *light = None;
    }

    /// Trigger loading of STT engines in background.
    ///
    /// In Auto mode, loads the light engine (110M) and, if system RAM allows,
    /// also loads the quality engine (Unified 0.6B) in parallel.
    /// In Pinned mode, loads only the pinned engine into the light slot.
    pub fn trigger_load_engines(&self) {
        let cur_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let engine_mode = self
            .engine_mode
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let models_dir = self.models_dir.clone();
        let light = Arc::clone(&self.light);
        let quality = Arc::clone(&self.quality);
        let generation = Arc::clone(&self.generation);
        let ttl = self.ttl;

        let builder = std::thread::Builder::new().name("taurine-voice-loader".to_string());
        let spawn_res = builder.spawn(move || {
            match engine_mode {
                EngineMode::Pinned(canonical) => {
                    let need_load = {
                        let guard = light.lock().unwrap_or_else(|p| p.into_inner());
                        match &*guard {
                            Some(slot) => {
                                slot.last_used.elapsed() > ttl || slot.transcriber.name() != canonical
                            }
                            None => true,
                        }
                    };
                    if need_load {
                        debug!("taurine-voice-loader: loading pinned voice model '{canonical}' into light slot");
                        let transcriber = create_transcriber(&canonical, models_dir.as_deref());
                        if generation.load(Ordering::SeqCst) == cur_gen {
                            let mut guard = light.lock().unwrap_or_else(|p| p.into_inner());
                            if generation.load(Ordering::SeqCst) == cur_gen {
                                *guard = Some(EngineSlot {
                                    transcriber,
                                    last_used: Instant::now(),
                                });
                            }
                        }
                    }
                }
                EngineMode::Auto => {
                    let light_model = "parakeet-tdt-ctc-110m";
                    let need_load_light = {
                        let guard = light.lock().unwrap_or_else(|p| p.into_inner());
                        match &*guard {
                            Some(slot) => {
                                slot.last_used.elapsed() > ttl || slot.transcriber.name() != light_model
                            }
                            None => true,
                        }
                    };
                    if need_load_light {
                        debug!("taurine-voice-loader: loading light model '{light_model}'");
                        let transcriber = create_transcriber(light_model, models_dir.as_deref());
                        if generation.load(Ordering::SeqCst) == cur_gen {
                            let mut guard = light.lock().unwrap_or_else(|p| p.into_inner());
                            if generation.load(Ordering::SeqCst) == cur_gen {
                                *guard = Some(EngineSlot {
                                    transcriber,
                                    last_used: Instant::now(),
                                });
                            }
                        } else {
                            return;
                        }
                    }

                    if generation.load(Ordering::SeqCst) == cur_gen
                        && taurine_core::voice::quality_engine_allowed()
                    {
                        let quality_model = "parakeet-unified-en-0.6b";
                        let need_load_quality = {
                            let guard = quality.lock().unwrap_or_else(|p| p.into_inner());
                            match &*guard {
                                Some(slot) => {
                                    slot.last_used.elapsed() > ttl
                                        || slot.transcriber.name() != quality_model
                                }
                                None => true,
                            }
                        };
                        if need_load_quality {
                            debug!("taurine-voice-loader: loading quality model '{quality_model}'");
                            let transcriber = create_transcriber(quality_model, models_dir.as_deref());
                            if generation.load(Ordering::SeqCst) == cur_gen {
                                let mut guard = quality.lock().unwrap_or_else(|p| p.into_inner());
                                if generation.load(Ordering::SeqCst) == cur_gen {
                                    *guard = Some(EngineSlot {
                                        transcriber,
                                        last_used: Instant::now(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        });
        if let Err(e) = spawn_res {
            warn!("VoiceSessionManager: failed to spawn loader thread: {e}");
        }
    }

    /// Return underlying audio capture handle.
    pub fn capture(&self) -> &Arc<AudioCapture> {
        &self.capture
    }

    /// Start a Push-To-Talk recording session.
    ///
    /// Clears any previous buffer, starts microphone capture, and transitions to `PushToTalk`.
    pub fn start_ptt(&self) -> Result<(), String> {
        if self.is_paused() {
            debug!("VoiceSessionManager: start_ptt ignored because daemon is paused");
            return Ok(());
        }

        let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
        if *mode != VoiceMode::Idle {
            return Ok(());
        }

        self.capture.buffer().clear();
        if let Err(e) = self.capture.start() {
            warn!("Failed to start audio capture stream for PTT: {e}");
            return Err(e);
        }

        *mode = VoiceMode::PushToTalk;
        info!("VoiceSessionManager: Push-To-Talk recording started");
        crate::services::audio::play_voice_start_cue();
        self.trigger_load_engines();
        Ok(())
    }

    /// Stop Push-To-Talk recording, transcribe captured speech, inject text, and return to `Idle`.
    ///
    /// Plays the mic-close (paste) cue immediately on PTT release, before
    /// transcription/paste runs, so feedback lines up with the hotkey.
    pub fn stop_ptt(&self) -> Result<Option<String>, String> {
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::PushToTalk {
                return Ok(None);
            }
            *mode = VoiceMode::Processing;
        }

        self.capture.stop();
        crate::services::audio::play_voice_stop_cue();
        info!("VoiceSessionManager: Push-To-Talk recording stopped; processing audio");
        let result = self.process_audio_and_inject();
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            *mode = VoiceMode::Idle;
        }
        self.restart_capture_if_always_on();
        result
    }

    /// Toggle Hands-Free continuous dictation mode.
    ///
    /// If `Idle`, starts capture and transitions to `HandsFree`.
    /// If `HandsFree`, stops capture, processes audio, injects text, and transitions to `Idle`.
    pub fn toggle_handsfree(&self) -> Result<(VoiceMode, Option<String>), String> {
        if self.is_paused() {
            debug!("VoiceSessionManager: toggle_handsfree ignored because daemon is paused");
            return Ok((VoiceMode::Idle, None));
        }

        let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
        match *mode {
            VoiceMode::HandsFree => {
                *mode = VoiceMode::Processing;
                drop(mode);
                self.capture.stop();
                crate::services::audio::play_voice_stop_cue();
                info!("VoiceSessionManager: Hands-Free dictation toggled off; processing audio");
                let result = self.process_audio_and_inject()?;
                {
                    let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
                    *mode = VoiceMode::Idle;
                }
                self.restart_capture_if_always_on();
                Ok((VoiceMode::Idle, result))
            }
            VoiceMode::Idle => {
                self.capture.buffer().clear();
                if let Err(e) = self.capture.start() {
                    warn!("Failed to start audio capture stream for Hands-Free: {e}");
                    return Err(e);
                }
                *mode = VoiceMode::HandsFree;
                info!("VoiceSessionManager: Hands-Free dictation activated");
                crate::services::audio::play_voice_start_cue();
                self.trigger_load_engines();
                Ok((VoiceMode::HandsFree, None))
            }
            _ => Ok((*mode, None)),
        }
    }

    /// Universal escape stopper.
    ///
    /// Pressing Escape immediately terminates any active dictation session (Push-to-Talk or Hands-Free),
    /// processes the captured audio, injects the formatted transcript, and resets mode to `Idle`.
    /// Plays the mic-close cue immediately on Escape, before transcription runs.
    pub fn on_escape_pressed(&self) -> Result<Option<String>, String> {
        let was_recording = {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode == VoiceMode::PushToTalk || *mode == VoiceMode::HandsFree {
                *mode = VoiceMode::Processing;
                true
            } else {
                false
            }
        };

        if !was_recording {
            return Ok(None);
        }

        self.capture.stop();
        crate::services::audio::play_voice_stop_cue();
        info!("VoiceSessionManager: Escape pressed; terminating voice session and transcribing");
        let result = self.process_audio_and_inject();
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            *mode = VoiceMode::Idle;
        }
        self.restart_capture_if_always_on();
        result
    }

    /// Cancel current voice recording immediately without transcribing or injecting text.
    pub fn cancel(&self) {
        let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
        *mode = VoiceMode::Idle;
        drop(mode);
        self.capture.stop();
        self.capture.buffer().clear();
        self.restart_capture_if_always_on();
        crate::services::audio::play_voice_stop_cue();
        debug!("VoiceSessionManager: Dictation cancelled");
    }

    /// Restart the audio capture stream if always-on ambient mode is active.
    ///
    /// Called after every PTT/HandsFree/Escape session ends so the AlwaysOnVoiceListener
    /// ambient loop continues receiving microphone frames without a gap.
    fn restart_capture_if_always_on(&self) {
        if self.always_on_enabled.load(Ordering::Relaxed) {
            if let Err(e) = self.capture.start() {
                warn!("VoiceSessionManager: failed to restart ambient capture after session: {e}");
            } else {
                debug!("VoiceSessionManager: ambient capture restarted after session end");
            }
        }
    }

    /// Drain captured audio samples and execute full on-the-fly STT, formatting, and injection.
    pub fn process_audio_and_inject(&self) -> Result<Option<String>, String> {
        let samples = self.capture.buffer().drain();
        self.process_audio_samples(&samples)
    }

    /// Process a slice of 16kHz mono audio samples:
    /// 1. Verifies minimum length and energy threshold.
    /// 2. Loads the configured STT model on-the-fly.
    /// 3. Transcribes speech.
    /// 4. Drops model immediately (0 MB idle RAM footprint).
    /// 5. Applies personal dictionary corrections and spoken punctuation formatting.
    /// 6. Records usage statistics.
    /// 7. Injects text into active window.
    pub fn process_audio_samples(&self, samples: &[f32]) -> Result<Option<String>, String> {
        if samples.len() < MIN_SAMPLES_COUNT {
            debug!(
                "VoiceSessionManager: audio buffer too short ({} samples); skipping STT",
                samples.len()
            );
            return Ok(None);
        }

        // Calculate RMS energy to reject pure silence or microphone noise
        let energy: f32 = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
        if energy < MIN_SPEECH_RMS_ENERGY {
            debug!(
                "VoiceSessionManager: audio below RMS energy threshold ({:.6}); skipping STT",
                energy
            );
            return Ok(None);
        }

        let need_trigger = self
            .light
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_none()
            && self
                .quality
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_none();
        if need_trigger {
            self.trigger_load_engines();
        }

        // Wait for light up to 8s, polling every 50ms (or proceed if quality is ready)
        let wait_start = Instant::now();
        let wait_timeout = Duration::from_secs(8);
        let poll_interval = Duration::from_millis(50);
        while wait_start.elapsed() < wait_timeout {
            if self
                .light
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_some()
                || self
                    .quality
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .is_some()
            {
                break;
            }
            std::thread::sleep(poll_interval);
        }

        let mut quality_guard = self.quality.lock().unwrap_or_else(|p| p.into_inner());
        let mut light_guard = self.light.lock().unwrap_or_else(|p| p.into_inner());

        let transcription = if quality_guard.is_some() {
            *light_guard = None;
            drop(light_guard);

            let slot = quality_guard.as_mut().unwrap();
            let res = slot
                .transcriber
                .transcribe(samples, 16000)
                .map_err(|e| format!("Voice transcription error: {e}"))?;
            slot.last_used = Instant::now();
            drop(quality_guard);
            res
        } else if light_guard.is_some() {
            drop(quality_guard);

            let slot = light_guard.as_mut().unwrap();
            let mut res = slot
                .transcriber
                .transcribe(samples, 16000)
                .map_err(|e| format!("Voice transcription error: {e}"))?;
            slot.last_used = Instant::now();
            drop(light_guard);

            if res.is_empty() {
                let mut q_guard = self.quality.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(q_slot) = q_guard.as_mut() {
                    debug!(
                        "VoiceSessionManager: light engine returned empty result; escalating to quality engine"
                    );
                    *self.light.lock().unwrap_or_else(|p| p.into_inner()) = None;

                    let q_res = q_slot
                        .transcriber
                        .transcribe(samples, 16000)
                        .map_err(|e| format!("Voice transcription error: {e}"))?;
                    q_slot.last_used = Instant::now();
                    res = q_res;
                }
            } else {
                let mut q_guard = self.quality.lock().unwrap_or_else(|p| p.into_inner());
                *q_guard = None;
            }
            res
        } else {
            drop(light_guard);
            drop(quality_guard);
            return Err("Voice engine failed to load; try again".to_string());
        };

        if transcription.is_empty() {
            debug!("VoiceSessionManager: transcription returned empty text");
            return Ok(None);
        }

        // Apply personal dictionary
        let dict = self.dictionary.lock().unwrap_or_else(|p| p.into_inner());
        let corrected = dict.apply(&transcription.text);
        drop(dict);

        // Apply spoken rules and formatting
        let formatted = format_transcript(&corrected);
        let trimmed = formatted.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let active_app = crate::platform::get_active_window_info().and_then(|info| info.exec_name);

        // Check if transcribed text matches any active voice trigger
        let normalized_spoken = taurine_core::db::crud::normalize_voice_phrase(trimmed);
        if let Ok(conn) = taurine_core::db::get_conn()
            && let Ok(triggers) = taurine_core::db::crud::list_active_voice_triggers(&conn)
        {
            let current_os = taurine_core::db::get_current_os_db_string();
            for trigger in triggers {
                let norm_trigger =
                    taurine_core::db::crud::normalize_voice_phrase(&trigger.spoken_phrase);
                if norm_trigger == normalized_spoken {
                    if trigger.target_os != "all" && trigger.target_os != current_os {
                        continue;
                    }
                    if let Some(ref only) = trigger.only_apps
                        && let Some(ref app) = active_app
                        && !only.split(',').any(|a| a.trim().eq_ignore_ascii_case(app))
                    {
                        continue;
                    }
                    if let Some(ref except) = trigger.except_apps
                        && let Some(ref app) = active_app
                        && except
                            .split(',')
                            .any(|a| a.trim().eq_ignore_ascii_case(app))
                    {
                        continue;
                    }

                    info!(
                        "VoiceSessionManager: matched voice trigger '{}'; expanding output",
                        trigger.spoken_phrase
                    );

                    if trigger.action_type == "text" {
                        crate::injector::inject_expansion(
                            vec![taurine_core::engine::variables::ExpansionStep::Text(
                                trigger.output.clone(),
                            )],
                            0,
                            taurine_core::settings::SpinnerStyle::default(),
                        );
                    }

                    taurine_core::db::crud::record_voice_trigger_usage(
                        &trigger.spoken_phrase,
                        trigger.output.chars().count(),
                        active_app,
                    );
                    let _ =
                        taurine_core::db::crud::increment_voice_trigger_usage(&conn, &trigger.id);

                    return Ok(Some(trigger.output));
                }
            }
        }

        // Record voice dictation stats
        let words_count = trimmed.split_whitespace().count();
        let chars_count = trimmed.chars().count();

        taurine_core::db::crud::record_voice_dictation_usage(words_count, chars_count, active_app);

        // Inject text segment using canonical expansion pipeline with IS_INJECTING guard
        crate::injector::inject_expansion(
            vec![taurine_core::engine::variables::ExpansionStep::Text(
                trimmed.to_string(),
            )],
            0,
            taurine_core::settings::SpinnerStyle::default(),
        );

        info!(
            "VoiceSessionManager: successfully injected {} words ({} chars)",
            words_count, chars_count
        );
        Ok(Some(trimmed.to_string()))
    }

    #[cfg(test)]
    pub(crate) fn inject_test_slots(&self, light: bool, quality: bool) {
        self.inject_test_slots_with_text(
            if light { Some("ok") } else { None },
            if quality { Some("ok") } else { None },
        );
    }

    #[cfg(test)]
    pub(crate) fn inject_test_slots_with_text(
        &self,
        light_text: Option<&str>,
        quality_text: Option<&str>,
    ) {
        struct TestDummyTranscriber {
            name: String,
            text: String,
        }
        impl Transcriber for TestDummyTranscriber {
            fn name(&self) -> &str {
                &self.name
            }
            fn transcribe(
                &mut self,
                _audio: &[f32],
                _sample_rate: u32,
            ) -> taurine_core::error::Result<taurine_core::voice::Transcription> {
                Ok(taurine_core::voice::Transcription::new(
                    &self.text, 1.0, 0.1,
                ))
            }
        }

        let mut q_guard = self.quality.lock().unwrap();
        let mut l_guard = self.light.lock().unwrap();
        *q_guard = quality_text.map(|text| EngineSlot {
            transcriber: Box::new(TestDummyTranscriber {
                name: "parakeet-unified-en-0.6b".to_string(),
                text: text.to_string(),
            }),
            last_used: Instant::now(),
        });
        *l_guard = light_text.map(|text| EngineSlot {
            transcriber: Box::new(TestDummyTranscriber {
                name: "parakeet-tdt-ctc-110m".to_string(),
                text: text.to_string(),
            }),
            last_used: Instant::now(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use taurine_core::voice::VoiceDictionary;

    fn create_test_session() -> (VoiceSessionManager, Arc<AtomicBool>) {
        let buffer = Arc::new(super::super::capture::AudioFrameBuffer::new());
        let capture = Arc::new(AudioCapture::new(buffer));
        let paused = Arc::new(AtomicBool::new(false));
        let session = VoiceSessionManager::new(capture, paused.clone());
        (session, paused)
    }

    #[test]
    fn test_session_initial_state() {
        let (session, _) = create_test_session();
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(!session.is_active());
        assert!(!session.is_paused());
    }

    #[test]
    fn test_session_escape_stops_handsfree() {
        let (session, _) = create_test_session();
        // Manually set mode to HandsFree
        *session.mode.lock().unwrap() = VoiceMode::HandsFree;
        assert!(session.is_active());

        let res = session.on_escape_pressed();
        assert!(res.is_ok());
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(!session.is_active());
    }

    #[test]
    fn test_session_escape_stops_ptt() {
        let (session, _) = create_test_session();
        *session.mode.lock().unwrap() = VoiceMode::PushToTalk;
        assert!(session.is_active());

        let res = session.on_escape_pressed();
        assert!(res.is_ok());
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(!session.is_active());
    }

    #[test]
    fn test_session_cancel() {
        let (session, _) = create_test_session();
        *session.mode.lock().unwrap() = VoiceMode::PushToTalk;
        session
            .capture()
            .buffer()
            .push_samples(&[0.1; MIN_SAMPLES_COUNT]);
        assert_eq!(session.capture().buffer().len(), MIN_SAMPLES_COUNT);

        session.cancel();
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(session.capture().buffer().is_empty());
    }

    #[test]
    fn test_session_paused_ignores_activation() {
        let (session, paused) = create_test_session();
        paused.store(true, Ordering::Relaxed);

        let res = session.start_ptt();
        assert!(res.is_ok());
        assert_eq!(session.current_mode(), VoiceMode::Idle);

        let (mode, text) = session.toggle_handsfree().unwrap();
        assert_eq!(mode, VoiceMode::Idle);
        assert!(text.is_none());
        assert_eq!(session.current_mode(), VoiceMode::Idle);
    }

    #[test]
    fn test_session_process_audio_silence_rejection() {
        let (session, _) = create_test_session();
        // Array of zeroes
        let silent_samples = vec![0.0f32; 16000];
        let res = session.process_audio_samples(&silent_samples);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), None);
    }

    #[test]
    fn test_session_process_audio_short_rejection() {
        let (session, _) = create_test_session();
        let short_samples = vec![0.5f32; 500]; // less than 1600
        let res = session.process_audio_samples(&short_samples);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), None);
    }

    #[test]
    fn test_session_with_dictionary_builder() {
        let (session, _) = create_test_session();
        let dict = VoiceDictionary::from_csv("Taurine, FastConformer");
        let session = session.with_dictionary(dict);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
    }

    #[test]
    fn test_cascade_releases_loser_slot() {
        let (session, _) = create_test_session();
        session.inject_test_slots_with_text(Some("light output"), Some("quality output"));
        let loud_samples = vec![0.5f32; 16000];
        let res = session.process_audio_samples(&loud_samples).unwrap();
        assert_eq!(res, Some("Quality output".to_string()));
        assert!(session.light.lock().unwrap().is_none());
        assert!(session.quality.lock().unwrap().is_some());
    }

    #[test]
    fn test_cascade_falls_back_to_light_when_quality_missing() {
        let (session, _) = create_test_session();
        session.inject_test_slots_with_text(Some("light output"), None);
        let loud_samples = vec![0.5f32; 16000];
        let res = session.process_audio_samples(&loud_samples).unwrap();
        assert_eq!(res, Some("Light output".to_string()));
        assert!(session.light.lock().unwrap().is_some());
        assert!(session.quality.lock().unwrap().is_none());
    }

    #[test]
    fn test_cascade_ttl_expires_both_slots() {
        let (session, _) = create_test_session();
        let session = session.with_ttl(Duration::from_millis(10));
        session.inject_test_slots(true, true);
        let past = Instant::now() - Duration::from_millis(50);
        session.light.lock().unwrap().as_mut().unwrap().last_used = past;
        session.quality.lock().unwrap().as_mut().unwrap().last_used = past;

        assert!(session.light.lock().unwrap().is_some());
        assert!(session.quality.lock().unwrap().is_some());

        session.clean_expired_transcriber();

        assert!(session.light.lock().unwrap().is_none());
        assert!(session.quality.lock().unwrap().is_none());
    }

    #[test]
    fn test_pinned_mode_skips_cascade() {
        let (session, _) = create_test_session();
        session.set_model_name("parakeet-tdt-ctc-110m");
        session.trigger_load_engines();

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            if session.light.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(session.light.lock().unwrap().is_some());
        assert_eq!(
            session
                .light
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .transcriber
                .name(),
            "parakeet-tdt-ctc-110m"
        );
        assert!(session.quality.lock().unwrap().is_none());
    }

    #[test]
    fn test_escalation_on_empty_light_result() {
        let (session, _) = create_test_session();
        let quality_slot = Arc::clone(&session.quality);

        struct EscalatingDummyLight {
            quality_slot: Arc<Mutex<Option<EngineSlot>>>,
        }
        impl Transcriber for EscalatingDummyLight {
            fn name(&self) -> &str {
                "parakeet-tdt-ctc-110m"
            }
            fn transcribe(
                &mut self,
                _audio: &[f32],
                _sample_rate: u32,
            ) -> taurine_core::error::Result<taurine_core::voice::Transcription> {
                struct QualityDummy;
                impl Transcriber for QualityDummy {
                    fn name(&self) -> &str {
                        "parakeet-unified-en-0.6b"
                    }
                    fn transcribe(
                        &mut self,
                        _audio: &[f32],
                        _sample_rate: u32,
                    ) -> taurine_core::error::Result<taurine_core::voice::Transcription>
                    {
                        Ok(taurine_core::voice::Transcription::new(
                            "hello world",
                            1.0,
                            0.5,
                        ))
                    }
                }
                *self.quality_slot.lock().unwrap() = Some(EngineSlot {
                    transcriber: Box::new(QualityDummy),
                    last_used: Instant::now(),
                });
                Ok(taurine_core::voice::Transcription::new("", 0.0, 0.0))
            }
        }

        *session.light.lock().unwrap() = Some(EngineSlot {
            transcriber: Box::new(EscalatingDummyLight { quality_slot }),
            last_used: Instant::now(),
        });
        *session.quality.lock().unwrap() = None;

        let loud_samples = vec![0.5f32; 16000];
        let res = session.process_audio_samples(&loud_samples).unwrap();
        assert_eq!(res, Some("Hello world".to_string()));
        assert!(session.light.lock().unwrap().is_none());
        assert!(session.quality.lock().unwrap().is_some());
    }

    #[test]
    fn test_set_model_name_invalidates_cache() {
        let (session, _) = create_test_session();
        session.inject_test_slots(true, true);
        assert!(session.light.lock().unwrap().is_some());
        assert!(session.quality.lock().unwrap().is_some());

        session.set_model_name("parakeet-tdt-ctc-110m");
        assert!(session.light.lock().unwrap().is_none());
        assert!(session.quality.lock().unwrap().is_none());
    }

    #[test]
    fn test_unload_model_clears_cache() {
        let (session, _) = create_test_session();
        session.inject_test_slots(true, true);
        assert!(session.light.lock().unwrap().is_some());
        assert!(session.quality.lock().unwrap().is_some());

        session.unload_model();
        assert!(session.light.lock().unwrap().is_none());
        assert!(session.quality.lock().unwrap().is_none());
    }
}
