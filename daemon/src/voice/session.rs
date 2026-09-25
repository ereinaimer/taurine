use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use taurine_core::voice::{Transcriber, VoiceDictionary, format_transcript};

use super::capture::AudioCapture;
use super::capture::{
    normalize_snippet_rms, slice_utterance_with_postroll, trailing_silence_frames,
};
use super::factory::create_transcriber;
use super::modes::VoiceMode;

/// Minimal energy threshold to consider audio frames as containing speech.
const MIN_SPEECH_RMS_ENERGY: f32 = 1e-7;

/// Minimum audio samples required for transcription (100ms at 16kHz = 1600 samples).
const MIN_SAMPLES_COUNT: usize = 1600;

/// Default inactivity timeout before the voice model is dropped from RAM.
const DEFAULT_INACTIVITY_TTL: Duration = Duration::from_secs(10);

struct EngineSlot {
    transcriber: Box<dyn Transcriber>,
    model_id: String,
    last_used: Instant,
}

/// Manages the lifecycle of voice recording, dictation sessions, and text injection.
///
/// Holds at most one speech recognition model in RAM (the single configured
/// model), loaded on demand when a Push-to-Talk or Hands-Free session starts
/// and fully evicted after 10 seconds of inactivity for 0 MB idle voice RAM.
/// The microphone stream is closed whenever no session is active.
pub struct VoiceSessionManager {
    mode: Mutex<VoiceMode>,
    capture: Arc<AudioCapture>,
    paused: Arc<AtomicBool>,
    model_name: Mutex<String>,
    models_dir: Option<PathBuf>,
    dictionary: Mutex<VoiceDictionary>,
    engine: Arc<Mutex<Option<EngineSlot>>>,
    generation: Arc<AtomicU64>,
    loading: Arc<Mutex<Option<(u64, String)>>>,
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
            engine: Arc::new(Mutex::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
            loading: Arc::new(Mutex::new(None)),
            ttl: DEFAULT_INACTIVITY_TTL,
        }
    }

    /// Builder method to specify the voice model name.
    pub fn with_model_name(self, name: impl Into<String>) -> Self {
        *self.model_name.lock().unwrap_or_else(|p| p.into_inner()) = name.into();
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

    /// Resolve the configured model name to the single model ID to load.
    ///
    /// Respects `voice_model`: a pinned canonical name loads exactly that
    /// model, `auto` loads unified on >= 16 GiB total RAM and 110m below.
    fn target_model(&self) -> &'static str {
        let configured = self
            .model_name
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        taurine_core::voice::resolve_configured_model(&configured)
    }

    /// Update the STT model name. If the model changed, clears the warm cache.
    pub fn set_model_name(&self, name: impl Into<String>) {
        let new_name = name.into();
        let mut model = self.model_name.lock().unwrap_or_else(|p| p.into_inner());
        if *model != new_name {
            *model = new_name;
            let new_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
            let mut engine = self.engine.lock().unwrap_or_else(|p| p.into_inner());
            *engine = None;
            // Invalidate only loads older than this switch; a trigger that
            // raced past the bump owns its flag and its generation gate.
            let mut loading = self.loading.lock().unwrap_or_else(|p| p.into_inner());
            if matches!(&*loading, Some((g, _)) if *g < new_gen) {
                *loading = None;
            }
        }
    }

    /// Update personal dictionary rules.
    pub fn set_dictionary(&self, dict: VoiceDictionary) {
        *self.dictionary.lock().unwrap_or_else(|p| p.into_inner()) = dict;
    }

    /// Unloads the voice model from RAM if it has exceeded the inactivity TTL (10s).
    ///
    /// Only evicts while idle: an active recording or transcription pins the
    /// model so long dictations never pay a mid-session reload. Guarantees
    /// 0 MB idle voice memory 10 seconds after speech finishes.
    pub fn clean_expired_transcriber(&self) {
        if self.is_active() {
            return;
        }
        let mut guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ref slot) = *guard
            && slot.last_used.elapsed() > self.ttl
        {
            *guard = None;
            debug!("VoiceSessionManager: unloading idle voice model from RAM (10s TTL expired)");
        }
    }

    /// Refresh the loaded slot so a session starting on a stale-but-fresh
    /// model does not expire mid-recording.
    fn touch_loaded_slot(&self) {
        let mut guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ref mut slot) = *guard
            && slot.last_used.elapsed() <= self.ttl
        {
            slot.last_used = Instant::now();
        }
    }

    /// Immediately unloads the speech recognition model from RAM and invalidates any in-flight background loaders.
    pub fn unload_model(&self) {
        let new_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut engine = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        *engine = None;
        let mut loading = self.loading.lock().unwrap_or_else(|p| p.into_inner());
        if matches!(&*loading, Some((g, _)) if *g < new_gen) {
            *loading = None;
        }
        debug!("VoiceSessionManager: voice engine unloaded from RAM");
    }

    /// Trigger loading of the single configured STT engine in the background.
    ///
    /// Only the resolved `voice_model` is loaded. A fresh slot whose
    /// `last_used` is within the TTL is reused; older generation loaders
    /// are invalidated on model configuration switch.
    pub fn trigger_load_engines(&self) {
        let target = self.target_model().to_string();
        let fresh = {
            let guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
            match &*guard {
                Some(slot) => slot.model_id == target && slot.last_used.elapsed() <= self.ttl,
                None => false,
            }
        };
        if fresh {
            return;
        }

        // Single-flight: a second trigger while this target is already
        // loading waits on the in-flight load instead of spawning another.
        let cur_gen = {
            let mut loading = self.loading.lock().unwrap_or_else(|p| p.into_inner());
            match &*loading {
                Some((_, in_flight)) if *in_flight == target => return,
                _ => {
                    let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
                    *loading = Some((next, target.clone()));
                    next
                }
            }
        };
        let models_dir = self.models_dir.clone();
        let engine = Arc::clone(&self.engine);
        let generation = Arc::clone(&self.generation);
        let loading = Arc::clone(&self.loading);
        let ttl = self.ttl;

        let builder = std::thread::Builder::new().name("taurine-voice-loader".to_string());
        let failed_target = target.clone();
        let spawn_res = builder.spawn(move || {
            let need_load = {
                let guard = engine.lock().unwrap_or_else(|p| p.into_inner());
                match &*guard {
                    Some(slot) => slot.last_used.elapsed() > ttl || slot.model_id != target,
                    None => true,
                }
            };
            let release = |insert: Option<Box<dyn Transcriber>>| {
                if generation.load(Ordering::SeqCst) != cur_gen {
                    return;
                }
                let mut guard = engine.lock().unwrap_or_else(|p| p.into_inner());
                if generation.load(Ordering::SeqCst) != cur_gen {
                    return;
                }
                let mut in_flight = loading.lock().unwrap_or_else(|p| p.into_inner());
                // Only the loader that owns this (generation, target) pair
                // may clear it; a newer trigger for any target wins.
                if *in_flight != Some((cur_gen, target.clone())) {
                    return;
                }
                *in_flight = None;
                if let Some(transcriber) = insert {
                    *guard = Some(EngineSlot {
                        transcriber,
                        model_id: target.clone(),
                        last_used: Instant::now(),
                    });
                }
            };
            if !need_load {
                release(None);
                return;
            }
            debug!("taurine-voice-loader: loading voice model '{target}'");
            let transcriber = create_transcriber(&target, models_dir.as_deref());
            release(Some(transcriber));
        });
        if let Err(e) = spawn_res {
            warn!("VoiceSessionManager: failed to spawn loader thread: {e}");
            let mut in_flight = self.loading.lock().unwrap_or_else(|p| p.into_inner());
            if *in_flight == Some((cur_gen, failed_target)) {
                *in_flight = None;
            }
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
        drop(mode);
        self.touch_loaded_slot();
        info!("VoiceSessionManager: Push-To-Talk recording started");
        crate::services::audio::play_voice_start_cue();
        self.trigger_load_engines();
        Ok(())
    }

    /// Stop Push-To-Talk recording, transcribe captured speech, inject text, and return to `Idle`.
    ///
    /// Plays the mic-close (paste) cue immediately on PTT release, before
    /// transcription/paste runs, so feedback lines up with the hotkey.
    /// The microphone stream is closed immediately.
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
        result
    }

    /// Toggle Hands-Free continuous dictation mode.
    ///
    /// If `Idle`, starts capture and transitions to `HandsFree`.
    /// If `HandsFree`, stops capture, processes audio, injects text, and transitions to `Idle`.
    /// The microphone stream is closed whenever Hands-Free is off.
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
                Ok((VoiceMode::Idle, result))
            }
            VoiceMode::Idle => {
                self.capture.buffer().clear();
                if let Err(e) = self.capture.start() {
                    warn!("Failed to start audio capture stream for Hands-Free: {e}");
                    return Err(e);
                }
                *mode = VoiceMode::HandsFree;
                drop(mode);
                self.touch_loaded_slot();
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
        result
    }

    /// Cancel current voice recording immediately without transcribing or injecting text.
    pub fn cancel(&self) {
        let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
        *mode = VoiceMode::Idle;
        drop(mode);
        self.capture.stop();
        self.capture.buffer().clear();
        crate::services::audio::play_voice_stop_cue();
        debug!("VoiceSessionManager: Dictation cancelled");
    }

    /// Drain captured audio samples and execute full on-the-fly STT, formatting, and injection.
    pub fn process_audio_and_inject(&self) -> Result<Option<String>, String> {
        let samples = self.capture.buffer().drain();
        self.process_audio_samples(&samples)
    }

    /// Process a slice of 16kHz mono audio samples:
    /// 1. Trims excess trailing silence while preserving the 250 ms post-roll decay cushion.
    /// 2. Verifies minimum length and energy threshold.
    /// 3. Loads the single configured STT model on-the-fly.
    /// 4. Transcribes speech (model evicted after 10s idle via [`Self::clean_expired_transcriber`]).
    /// 5. Applies personal dictionary corrections and spoken punctuation formatting.
    /// 6. Matches voice triggers (trigger action) or injects dictation text.
    /// 7. Records usage statistics and injects text into the active window.
    pub fn process_audio_samples(&self, samples: &[f32]) -> Result<Option<String>, String> {
        // Preserve trailing consonant decay: trim dead silence beyond the 8-frame cushion.
        let silence_frames = trailing_silence_frames(samples);
        let trimmed = slice_utterance_with_postroll(samples, silence_frames);
        let samples = trimmed.as_slice();

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

        // Standardize speech level before recognition (raw energy above decided speech).
        let normed = normalize_snippet_rms(samples);
        let samples = normed.as_slice();

        let need_trigger = self
            .engine
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_none();
        if need_trigger {
            self.trigger_load_engines();
        }

        // Wait for the single engine up to 8s, polling every 50ms.
        let wait_start = Instant::now();
        let wait_timeout = Duration::from_secs(8);
        let poll_interval = Duration::from_millis(50);
        while wait_start.elapsed() < wait_timeout {
            if self
                .engine
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_some()
            {
                break;
            }
            std::thread::sleep(poll_interval);
        }

        let mut engine_guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        let Some(ref mut slot) = *engine_guard else {
            return Err("Voice engine failed to load; try again".to_string());
        };
        let transcription = slot
            .transcriber
            .transcribe(samples, 16000)
            .map_err(|e| format!("Voice transcription error: {e}"))?;
        slot.last_used = Instant::now();
        drop(engine_guard);

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
            let in_scope: Vec<taurine_core::db::crud::VoiceTriggerRow> = triggers
                .into_iter()
                .filter(|t| {
                    (t.target_os == "all" || t.target_os == current_os)
                        && match (&t.only_apps, &active_app) {
                            (Some(only), Some(app)) => {
                                only.split(',').any(|a| a.trim().eq_ignore_ascii_case(app))
                            }
                            _ => true,
                        }
                        && match (&t.except_apps, &active_app) {
                            (Some(except), Some(app)) => !except
                                .split(',')
                                .any(|a| a.trim().eq_ignore_ascii_case(app)),
                            _ => true,
                        }
                })
                .collect();

            if let Some(matched) =
                taurine_core::voice::rank_voice_triggers(&normalized_spoken, &in_scope)
            {
                let trigger = matched.trigger;
                info!(
                    "VoiceSessionManager: matched voice trigger '{}' (score {:.2}); expanding output",
                    trigger.spoken_phrase, matched.score
                );

                let output = trigger.output.clone();
                crate::voice::fire_voice_trigger(
                    trigger,
                    &conn,
                    active_app.clone(),
                    taurine_core::settings::SpinnerStyle::default(),
                );
                return Ok(Some(output));
            }
        }

        let final_text = trimmed;
        if final_text.is_empty() {
            return Ok(None);
        }

        // Record voice dictation stats
        let words_count = final_text.split_whitespace().count();
        let chars_count = final_text.chars().count();

        taurine_core::db::crud::record_voice_dictation_usage(words_count, chars_count, active_app);

        // Inject text segment using canonical expansion pipeline with IS_INJECTING guard
        crate::injector::inject_expansion(
            vec![taurine_core::engine::variables::ExpansionStep::Text(
                final_text.to_string(),
            )],
            0,
            taurine_core::settings::SpinnerStyle::default(),
        );

        info!(
            "VoiceSessionManager: successfully injected {} words ({} chars)",
            words_count, chars_count
        );
        Ok(Some(final_text.to_string()))
    }

    #[cfg(test)]
    pub(crate) fn inject_test_slot(&self, text: &str) {
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

        let target = self.target_model().to_string();
        let mut guard = self.engine.lock().unwrap();
        *guard = Some(EngineSlot {
            transcriber: Box::new(TestDummyTranscriber {
                name: target.clone(),
                text: text.to_string(),
            }),
            model_id: target,
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
    fn test_single_engine_transcribes_and_stays_resident() {
        let (session, _) = create_test_session();
        session.inject_test_slot("hello world");
        let loud_samples = vec![0.5f32; 16000];
        let res = session.process_audio_samples(&loud_samples).unwrap();
        assert_eq!(res, Some("Hello world".to_string()));
        assert!(session.engine.lock().unwrap().is_some());
    }

    #[test]
    fn test_single_engine_empty_transcription_returns_none() {
        let (session, _) = create_test_session();
        session.inject_test_slot("");
        let loud_samples = vec![0.5f32; 16000];
        let res = session.process_audio_samples(&loud_samples).unwrap();
        assert_eq!(res, None);
    }

    #[test]
    fn test_single_engine_inactivity_eviction() {
        let (session, _) = create_test_session();
        let session = session.with_ttl(Duration::from_millis(10));
        session.inject_test_slot("ok");
        assert!(session.engine.lock().unwrap().is_some());

        let past = Instant::now() - Duration::from_millis(50);
        session.engine.lock().unwrap().as_mut().unwrap().last_used = past;

        // Inactivity TTL unloads the single voice model for 0 MB idle RAM.
        session.clean_expired_transcriber();
        assert!(session.engine.lock().unwrap().is_none());

        // Fresh slots within the TTL are retained.
        session.inject_test_slot("ok");
        session.clean_expired_transcriber();
        assert!(session.engine.lock().unwrap().is_some());

        // Explicit unload drops the engine immediately.
        session.unload_model();
        assert!(session.engine.lock().unwrap().is_none());
    }

    #[test]
    fn test_pinned_mode_loads_single_slot() {
        let (session, _) = create_test_session();
        session.set_model_name("parakeet-tdt-ctc-110m");
        session.trigger_load_engines();

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            if session.engine.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let guard = session.engine.lock().unwrap();
        let slot = guard.as_ref().expect("single engine slot must load");
        assert_eq!(slot.model_id, "parakeet-tdt-ctc-110m");
        assert_eq!(slot.transcriber.name(), "parakeet-tdt-ctc-110m");
    }

    #[test]
    fn test_spam_press_loads_once() {
        let (session, _) = create_test_session();
        session.set_model_name("parakeet-tdt-ctc-110m");
        for _ in 0..10 {
            session.trigger_load_engines();
        }
        // One spawn only: the model switch above bumped the generation
        // to 1, and the burst after the first trigger sees the in-flight
        // load and returns without bumping it again.
        assert_eq!(session.generation.load(Ordering::SeqCst), 2);

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            if session.engine.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let guard = session.engine.lock().unwrap();
        let slot = guard.as_ref().expect("single engine slot must load");
        assert_eq!(slot.model_id, "parakeet-tdt-ctc-110m");
        assert_eq!(slot.transcriber.name(), "parakeet-tdt-ctc-110m");
    }

    #[test]
    fn test_active_session_never_evicts() {
        let (session, _) = create_test_session();
        let session = session.with_ttl(Duration::from_millis(10));
        session.inject_test_slot("ok");

        let past = Instant::now() - Duration::from_millis(50);
        session.engine.lock().unwrap().as_mut().unwrap().last_used = past;

        // Recording pins the model: a stale slot survives while active.
        *session.mode.lock().unwrap() = VoiceMode::HandsFree;
        session.clean_expired_transcriber();
        assert!(session.engine.lock().unwrap().is_some());

        // Idle with the same stale timestamp evicts.
        *session.mode.lock().unwrap() = VoiceMode::Idle;
        session.clean_expired_transcriber();
        assert!(session.engine.lock().unwrap().is_none());
    }

    #[test]
    fn test_set_model_name_invalidates_cache() {
        let (session, _) = create_test_session();
        session.inject_test_slot("ok");
        assert!(session.engine.lock().unwrap().is_some());

        session.set_model_name("parakeet-tdt-ctc-110m");
        assert!(session.engine.lock().unwrap().is_none());
    }

    #[test]
    fn test_unload_model_clears_cache() {
        let (session, _) = create_test_session();
        session.inject_test_slot("ok");
        assert!(session.engine.lock().unwrap().is_some());

        session.unload_model();
        assert!(session.engine.lock().unwrap().is_none());
    }
}
