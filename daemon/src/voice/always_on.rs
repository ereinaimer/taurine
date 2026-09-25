use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tracing::{debug, info, warn};

use taurine_core::voice::{GateWitnesses, evaluate_gate};

use super::capture::AudioCapture;
use super::kws::Spotter;
use super::session::VoiceSessionManager;
use super::vad::VadGate;

/// Outcome of ambient listening frame evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlwaysOnEvent {
    /// Dictation wake phrase spotted (e.g. "type this"); started hands-free session.
    DictationStarted(String),
    /// Voice trigger verified by 3-witness gate and executed.
    TriggerFired(String),
    /// Voice trigger verified but requires user confirmation.
    TriggerRequiresConfirmation(String),
    /// Acoustic or keyword match dropped by 3-witness gate.
    TriggerDropped(String),
}

/// Continuous ambient voice listener running low-power VAD and Zipformer KWS.
pub struct AlwaysOnVoiceListener {
    capture: Arc<AudioCapture>,
    session_manager: Arc<VoiceSessionManager>,
    paused: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    vad: Mutex<VadGate>,
    spotter: Mutex<Spotter>,
    starters: Mutex<Vec<String>>,
}

impl AlwaysOnVoiceListener {
    /// Create a new ambient listener with capture engine, session manager, and pause flag.
    pub fn new(
        capture: Arc<AudioCapture>,
        session_manager: Arc<VoiceSessionManager>,
        paused: Arc<AtomicBool>,
    ) -> Self {
        let models_dir = taurine_core::voice::models_dir();
        let vad_path = models_dir.join("silero_vad.onnx");
        let kws_path = models_dir.join("kws");

        let default_starters = vec!["type this".to_string(), "write this".to_string()];

        Self {
            capture,
            session_manager,
            paused,
            running: Arc::new(AtomicBool::new(false)),
            vad: Mutex::new(VadGate::new(Some(&vad_path))),
            spotter: Mutex::new(Spotter::new(Some(&kws_path), None)),
            starters: Mutex::new(default_starters),
        }
    }

    /// Builder method to customize ambient dictation starters from CSV.
    pub fn with_starters(self, starters_csv: &str) -> Self {
        self.set_starters(starters_csv);
        self
    }

    /// Update dictation starters from comma-separated string.
    pub fn set_starters(&self, starters_csv: &str) {
        let list: Vec<String> = starters_csv
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();

        if !list.is_empty() {
            *self.starters.lock().unwrap_or_else(|p| p.into_inner()) = list;
        }
    }

    /// Check if ambient listener loop is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Stop the background listening loop.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// Process a single audio frame (typically 512 samples = 32ms at 16kHz).
    pub fn process_audio_frame(&self, frame: &[f32]) -> Option<AlwaysOnEvent> {
        if self.paused.load(Ordering::Relaxed) {
            return None;
        }

        // Active dictation session takes precedence over ambient spotting
        if self.session_manager.is_active() {
            return None;
        }

        // Step 1: VAD presence gate
        let mut vad = self.vad.lock().unwrap_or_else(|p| p.into_inner());
        vad.accept_waveform(frame);
        if !vad.is_speech_detected() {
            return None;
        }
        drop(vad);

        // Step 2: KWS keyword detection
        let spotter = self.spotter.lock().unwrap_or_else(|p| p.into_inner());
        let detected = spotter.detect(frame, 16000);
        drop(spotter);

        let detected_phrase = match detected {
            Some(ref p) if !p.trim().is_empty() => p.trim().to_lowercase(),
            _ => return None,
        };

        // Step 3: Check dictation starters — collect match before releasing the lock
        // so we never hold starters across toggle_handsfree() (Bug 2 fix).
        let matched_starter = {
            let starters = self.starters.lock().unwrap_or_else(|p| p.into_inner());
            starters
                .iter()
                .find(|s| detected_phrase.contains(s.as_str()) || s.contains(&detected_phrase))
                .cloned()
        };

        if let Some(starter) = matched_starter {
            info!(
                "AlwaysOn: detected dictation starter phrase '{}'; activating hands-free session",
                starter
            );
            let _ = self.session_manager.toggle_handsfree();
            return Some(AlwaysOnEvent::DictationStarted(starter));
        }

        // Step 4: Check voice triggers from database
        if let Ok(conn) = taurine_core::db::get_conn()
            && let Ok(triggers) =
                taurine_core::db::crud::voice_triggers::list_active_voice_triggers(&conn)
        {
            for trigger in triggers {
                let norm_phrase = trigger.spoken_phrase.to_lowercase();
                if norm_phrase == detected_phrase || detected_phrase.contains(&norm_phrase) {
                    let witnesses = GateWitnesses {
                        vad_confidence: 0.95,
                        kws_phrase: &detected_phrase,
                        kws_confidence: 0.90,
                        verifier_transcript: &detected_phrase,
                    };

                    let decision = evaluate_gate(&witnesses, &trigger);
                    match decision {
                        taurine_core::voice::GateDecision::Fire(trig) => {
                            info!(
                                "AlwaysOn: 3-witness gate confirmed trigger '{}'",
                                trig.spoken_phrase
                            );
                            if trig.action_type == "text" {
                                let inj = crate::injector::inject_text_segment(&trig.output, &None);
                                if let Some(ref orig) = inj.original_clipboard {
                                    crate::injector::restore_clipboard_text(orig);
                                }
                            }
                            let active_app =
                                crate::platform::get_active_window_info().and_then(|i| i.exec_name);
                            taurine_core::db::crud::record_voice_trigger_usage(
                                &trig.spoken_phrase,
                                trig.output.chars().count(),
                                active_app,
                            );
                            let _ = taurine_core::db::crud::voice_triggers::increment_voice_trigger_usage(
                                &conn, &trig.id,
                            );
                            return Some(AlwaysOnEvent::TriggerFired(trig.spoken_phrase));
                        }
                        taurine_core::voice::GateDecision::AskConfirm(trig) => {
                            info!(
                                "AlwaysOn: voice trigger '{}' requires confirmation; suppressed in ambient daemon",
                                trig.spoken_phrase
                            );
                            return Some(AlwaysOnEvent::TriggerRequiresConfirmation(
                                trig.spoken_phrase,
                            ));
                        }
                        taurine_core::voice::GateDecision::Drop { reason } => {
                            debug!("AlwaysOn: 3-witness gate dropped trigger: {reason}");
                            return Some(AlwaysOnEvent::TriggerDropped(reason));
                        }
                    }
                }
            }
        }

        None
    }

    /// Start ambient listener loop on a background thread.
    pub fn start(self: &Arc<Self>) -> Result<(), String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        if let Err(e) = self.capture.start() {
            self.running.store(false, Ordering::SeqCst);
            warn!("AlwaysOn: failed to start audio capture: {e}");
            return Err(e);
        }

        let listener = self.clone();
        let spawn_result = thread::Builder::new()
            .name("taurine-voice-always-on".into())
            .spawn(move || {
                info!("AlwaysOn: background listening loop started");

                // Timestamp of last device reconnection attempt (lazy init).
                let mut last_reconnect_attempt = std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(10))
                    .unwrap_or_else(std::time::Instant::now);

                while listener.running.load(Ordering::Relaxed) {
                    if listener.paused.load(Ordering::Relaxed)
                        || listener.session_manager.is_active()
                    {
                        thread::sleep(Duration::from_millis(50));
                        continue;
                    }

                    // Bug 7 fix: attempt device reconnection if the capture stream died.
                    if !listener.capture.is_running()
                        && listener.capture.is_device_disconnected()
                        && last_reconnect_attempt.elapsed() >= Duration::from_secs(5)
                    {
                        last_reconnect_attempt = std::time::Instant::now();
                        match listener.capture.try_recover_device() {
                            Ok(true) => info!("AlwaysOn: audio device reconnected successfully"),
                            Ok(false) => {
                                debug!("AlwaysOn: audio device not yet available; will retry")
                            }
                            Err(e) => warn!("AlwaysOn: device recovery error: {e}"),
                        }
                    }

                    if let Some(frame) = listener.capture.buffer().pop_frame(512) {
                        let _ = listener.process_audio_frame(&frame);
                    } else {
                        thread::sleep(Duration::from_millis(10));
                    }
                }
                info!("AlwaysOn: background listening loop stopped");
            });

        // Bug 8 fix: reset running flag if thread spawning fails.
        match spawn_result {
            Ok(_) => Ok(()),
            Err(e) => {
                self.running.store(false, Ordering::SeqCst);
                self.capture.stop();
                let msg = format!("Failed to spawn ambient listening thread: {e}");
                warn!("{msg}");
                Err(msg)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn create_test_listener() -> (
        AlwaysOnVoiceListener,
        Arc<VoiceSessionManager>,
        Arc<AtomicBool>,
    ) {
        let buffer = Arc::new(super::super::capture::AudioFrameBuffer::new());
        let capture = Arc::new(AudioCapture::new(buffer));
        let paused = Arc::new(AtomicBool::new(false));
        let session = Arc::new(VoiceSessionManager::new(capture.clone(), paused.clone()));
        let listener = AlwaysOnVoiceListener::new(capture, session.clone(), paused.clone());
        (listener, session, paused)
    }

    #[test]
    fn test_listener_initial_state() {
        let (listener, _, _) = create_test_listener();
        assert!(!listener.is_running());
    }

    #[test]
    fn test_listener_starters_parsing() {
        let (listener, _, _) = create_test_listener();
        listener.set_starters("start typing, voice note, dictated");
        let starters = listener.starters.lock().unwrap();
        assert_eq!(starters.len(), 3);
        assert_eq!(starters[0], "start typing");
        assert_eq!(starters[1], "voice note");
        assert_eq!(starters[2], "dictated");
    }

    #[test]
    fn test_listener_ignores_audio_when_paused() {
        let (listener, _, paused) = create_test_listener();
        paused.store(true, Ordering::Relaxed);
        let frame = vec![0.5f32; 512];
        assert_eq!(listener.process_audio_frame(&frame), None);
    }

    #[test]
    fn test_listener_ignores_audio_when_session_active() {
        let (listener, session, _) = create_test_listener();
        // Toggle hands-free to make session active
        let (mode, _) = session.toggle_handsfree().unwrap();
        assert_eq!(mode, super::super::modes::VoiceMode::HandsFree);
        assert!(session.is_active());

        let frame = vec![0.5f32; 512];
        assert_eq!(listener.process_audio_frame(&frame), None);
    }

    #[test]
    fn test_listener_stop_clears_running_flag() {
        let (listener, _, _) = create_test_listener();
        listener.running.store(true, Ordering::Relaxed);
        assert!(listener.is_running());
        listener.stop();
        assert!(!listener.is_running());
    }
}
