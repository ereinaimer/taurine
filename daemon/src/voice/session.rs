use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

use taurine_core::voice::VoiceDictionary;
use taurine_core::voice::format_transcript;

use super::capture::AudioCapture;
use super::factory::create_transcriber;
use super::modes::VoiceMode;

/// Minimal energy threshold to consider audio frames as containing speech.
const MIN_SPEECH_RMS_ENERGY: f32 = 0.0001;

/// Minimum audio samples required for transcription (100ms at 16kHz = 1600 samples).
const MIN_SAMPLES_COUNT: usize = 1600;

/// Manages the lifecycle of voice recording, dictation sessions, and text injection.
///
/// Ensures 0 MB idle RAM by dynamically loading speech recognition engines on-demand
/// when dictation completes, transcribing the buffered audio, and unloading them immediately.
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

    /// Update the STT model name.
    pub fn set_model_name(&self, name: impl Into<String>) {
        *self.model_name.lock().unwrap_or_else(|p| p.into_inner()) = name.into();
    }

    /// Update personal dictionary rules.
    pub fn set_dictionary(&self, dict: VoiceDictionary) {
        *self.dictionary.lock().unwrap_or_else(|p| p.into_inner()) = dict;
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
        if *mode == VoiceMode::PushToTalk {
            return Ok(());
        }

        self.capture.buffer().clear();
        if let Err(e) = self.capture.start() {
            warn!("Failed to start audio capture stream for PTT: {e}");
            return Err(e);
        }

        *mode = VoiceMode::PushToTalk;
        info!("VoiceSessionManager: Push-To-Talk recording started");
        Ok(())
    }

    /// Stop Push-To-Talk recording, transcribe captured speech, inject text, and return to `Idle`.
    pub fn stop_ptt(&self) -> Result<Option<String>, String> {
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::PushToTalk {
                return Ok(None);
            }
            *mode = VoiceMode::Idle;
        }

        self.capture.stop();
        info!("VoiceSessionManager: Push-To-Talk recording stopped; processing audio");
        let result = self.process_audio_and_inject();
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
                *mode = VoiceMode::Idle;
                drop(mode);
                self.capture.stop();
                info!("VoiceSessionManager: Hands-Free dictation toggled off; processing audio");
                let result = self.process_audio_and_inject()?;
                self.restart_capture_if_always_on();
                Ok((VoiceMode::Idle, result))
            }
            _ => {
                self.capture.buffer().clear();
                if let Err(e) = self.capture.start() {
                    warn!("Failed to start audio capture stream for Hands-Free: {e}");
                    return Err(e);
                }
                *mode = VoiceMode::HandsFree;
                info!("VoiceSessionManager: Hands-Free dictation activated");
                Ok((VoiceMode::HandsFree, None))
            }
        }
    }

    /// Universal escape stopper.
    ///
    /// Pressing Escape immediately terminates any active dictation session (Push-to-Talk or Hands-Free),
    /// processes the captured audio, injects the formatted transcript, and resets mode to `Idle`.
    pub fn on_escape_pressed(&self) -> Result<Option<String>, String> {
        let was_active = {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if !mode.is_active() {
                false
            } else {
                *mode = VoiceMode::Idle;
                true
            }
        };

        if !was_active {
            return Ok(None);
        }

        self.capture.stop();
        info!("VoiceSessionManager: Escape pressed; terminating voice session and transcribing");
        let result = self.process_audio_and_inject();
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

        let model_name = self
            .model_name
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let models_dir: Option<&Path> = self.models_dir.as_deref();

        debug!("VoiceSessionManager: loading model '{model_name}' on the go...");
        let mut transcriber = create_transcriber(&model_name, models_dir);

        let transcription = transcriber
            .transcribe(samples, 16000)
            .map_err(|e| format!("Voice transcription error: {e}"))?;

        // Explicitly drop model to guarantee 0 MB idle RAM
        drop(transcriber);
        debug!("VoiceSessionManager: transcription completed; model dropped from RAM");

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

        // Record voice dictation stats
        let words_count = trimmed.split_whitespace().count();
        let chars_count = trimmed.chars().count();
        let active_app = crate::platform::get_active_window_info().and_then(|info| info.exec_name);

        taurine_core::db::crud::record_voice_dictation_usage(words_count, chars_count, active_app);

        // Inject text segment and restore clipboard
        let injection = crate::injector::inject_text_segment(trimmed, &None);
        if let Some(ref orig) = injection.original_clipboard {
            crate::injector::restore_clipboard_text(orig);
        }

        info!(
            "VoiceSessionManager: successfully injected {} words ({} chars)",
            words_count, chars_count
        );
        Ok(Some(trimmed.to_string()))
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
}
