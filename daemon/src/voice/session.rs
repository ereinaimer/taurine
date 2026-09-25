use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use taurine_core::voice::{VoiceDictionary, format_transcript};

use super::capture::AudioCapture;
use super::modes::VoiceMode;
use super::worker_client::WorkerClient;

/// Minimal energy threshold to consider audio frames as containing speech.
pub(crate) const MIN_SPEECH_RMS_ENERGY: f32 = 1e-7;

/// Minimum audio samples required for transcription (100ms at 16kHz = 1600 samples).
pub(crate) const MIN_SAMPLES_COUNT: usize = 1600;

/// Default inactivity timeout before the voice model is dropped from RAM.
const DEFAULT_INACTIVITY_TTL: Duration = Duration::from_secs(10);

/// Chunk cadence for live streaming while recording.
const STREAM_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// TTL bookkeeping for the worker-side model. The audio and the recognizer
/// live in the `--voice-daemon` worker; this stays lean by design.
struct EngineMeta {
    model_id: String,
    last_used: Instant,
}

/// Lock-free progress shared with one chunk-forwarder thread.
struct StreamProgress {
    /// Samples already handed to the worker.
    sent: AtomicUsize,
    /// Next chunk sequence number.
    seq: AtomicU64,
    alive: AtomicBool,
}

impl StreamProgress {
    fn new() -> Self {
        Self {
            sent: AtomicUsize::new(0),
            seq: AtomicU64::new(0),
            alive: AtomicBool::new(true),
        }
    }
}

/// One live streaming request: shared progress plus a joinable thread.
///
/// `target` is snapshotted at request start so a settings change mid-record
/// can neither respawn the worker nor wipe its buffers under a live session.
struct StreamEntry {
    progress: Arc<StreamProgress>,
    handle: Option<std::thread::JoinHandle<()>>,
    target: String,
}

/// Manages the lifecycle of voice recording, dictation sessions, and text injection.
///
/// The single configured model lives in the isolated `--voice-daemon`
/// worker: it loads on demand when a Push-to-Talk or Hands-Free session
/// starts, stays pinned for the whole recording, and is fully evicted after
/// 10 seconds of inactivity. The microphone stream is closed whenever no
/// session is active.
pub struct VoiceSessionManager {
    mode: Mutex<VoiceMode>,
    capture: Arc<AudioCapture>,
    paused: Arc<AtomicBool>,
    model_name: Mutex<String>,
    dictionary: Mutex<VoiceDictionary>,
    worker: WorkerClient,
    meta: Mutex<Option<EngineMeta>>,
    active_req: Mutex<Option<String>>,
    streams: Arc<Mutex<HashMap<String, StreamEntry>>>,
    req_counter: AtomicU64,
    ttl: Duration,
}

impl VoiceSessionManager {
    /// Create a new session manager with an audio capture interface and pause flag.
    pub fn new(capture: Arc<AudioCapture>, paused: Arc<AtomicBool>) -> Self {
        Self {
            mode: Mutex::new(VoiceMode::Idle),
            capture,
            paused,
            model_name: Mutex::new("auto".to_string()),
            dictionary: Mutex::new(VoiceDictionary::default()),
            worker: WorkerClient::new().expect("voice worker runtime must build"),
            meta: Mutex::new(None),
            active_req: Mutex::new(None),
            streams: Arc::new(Mutex::new(HashMap::new())),
            req_counter: AtomicU64::new(0),
            ttl: DEFAULT_INACTIVITY_TTL,
        }
    }

    /// Builder method to specify the voice model name.
    pub fn with_model_name(self, name: impl Into<String>) -> Self {
        *self.model_name.lock().unwrap_or_else(|p| p.into_inner()) = name.into();
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

    /// Builder method to inject a worker client (tests only, no spawn).
    #[cfg(test)]
    pub(crate) fn with_worker(mut self, worker: WorkerClient) -> Self {
        self.worker = worker;
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

    /// Update the STT model name. If the model changed, the worker is shut
    /// down so the next session handshakes fresh on the new model.
    pub fn set_model_name(&self, name: impl Into<String>) {
        let new_name = name.into();
        let mut model = self.model_name.lock().unwrap_or_else(|p| p.into_inner());
        if *model != new_name {
            *model = new_name;
            self.worker.shutdown();
            let mut meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
            *meta = None;
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
        let mut guard = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ref meta) = *guard
            && meta.last_used.elapsed() > self.ttl
        {
            *guard = None;
            self.worker.unload_model();
            debug!("VoiceSessionManager: unloading idle voice model from RAM (10s TTL expired)");
        }
    }

    /// Refresh a fresh model timestamp so a session starting on a stale-but
    /// resident model does not expire mid-recording.
    fn touch_loaded_slot(&self) {
        let mut guard = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ref mut meta) = *guard
            && meta.last_used.elapsed() <= self.ttl
        {
            meta.last_used = Instant::now();
        }
    }

    /// Shut the worker process down entirely (daemon shutdown path).
    pub fn shutdown_worker(&self) {
        self.worker.shutdown();
        let mut meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        *meta = None;
        debug!("VoiceSessionManager: voice worker shut down");
    }

    /// Immediately unloads the speech recognition model from RAM.
    pub fn unload_model(&self) {
        let mut meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        *meta = None;
        self.worker.unload_model();
        debug!("VoiceSessionManager: voice engine unloaded from RAM");
    }

    /// Ensure the worker serves the single configured model.
    ///
    /// Only the resolved `voice_model` is ever loaded. A fresh model whose
    /// `last_used` is within the TTL is reused; the client singleton keeps
    /// concurrent triggers on one worker.
    pub fn ensure_worker_ready(&self) -> Result<(), String> {
        let target = self.target_model().to_string();
        let fresh = {
            let guard = self.meta.lock().unwrap_or_else(|p| p.into_inner());
            match &*guard {
                Some(meta) => meta.model_id == target && meta.last_used.elapsed() <= self.ttl,
                None => false,
            }
        };
        if fresh {
            return Ok(());
        }
        debug!("taurine-voice-loader: loading voice model '{target}'");
        self.worker.ensure_worker(&target)?;
        let mut guard = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        *guard = Some(EngineMeta {
            model_id: target,
            last_used: Instant::now(),
        });
        Ok(())
    }

    /// Stream newly captured audio to the worker while the request is alive.
    ///
    /// Runs on its own thread; the stop path stops it, joins it, sends the
    /// tail, and transcribes. Joining before reading progress guarantees the
    /// tail is sent exactly once (never duplicated, never dropped).
    fn spawn_chunk_forwarder(&self, req_id: String, target: String) {
        let progress = Arc::new(StreamProgress::new());
        let buffer = self.capture.buffer().clone();
        let worker = self.worker.clone();
        let thread_progress = Arc::clone(&progress);
        let thread_req = req_id.clone();
        let entry_target = target.clone();
        let builder = std::thread::Builder::new().name("taurine-voice-stream".to_string());
        let spawn_res = builder.spawn(move || {
            if let Err(e) = worker.ensure_worker(&target) {
                debug!("VoiceSessionManager: worker unavailable for '{target}': {e}");
                return;
            }
            loop {
                if !thread_progress.alive.load(Ordering::Relaxed) {
                    break;
                }
                let len = buffer.len();
                let sent = thread_progress.sent.load(Ordering::Relaxed);
                if len > sent {
                    let seq = thread_progress.seq.load(Ordering::Relaxed);
                    if worker
                        .append(&thread_req, seq, &buffer.peek_tail(len - sent))
                        .is_err()
                    {
                        break;
                    }
                    thread_progress.sent.store(len, Ordering::Relaxed);
                    thread_progress.seq.store(seq + 1, Ordering::Relaxed);
                }
                std::thread::sleep(STREAM_POLL_INTERVAL);
            }
        });
        match spawn_res {
            Ok(handle) => {
                self.streams
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(
                        req_id,
                        StreamEntry {
                            progress,
                            handle: Some(handle),
                            target: entry_target,
                        },
                    );
            }
            Err(e) => warn!("VoiceSessionManager: failed to spawn stream thread: {e}"),
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
        self.begin_request();
        Ok(())
    }

    /// Register a new streaming request and start its chunk forwarder.
    fn begin_request(&self) {
        let req_id = format!("r{}", self.req_counter.fetch_add(1, Ordering::Relaxed));
        *self.active_req.lock().unwrap_or_else(|p| p.into_inner()) = Some(req_id.clone());
        self.spawn_chunk_forwarder(req_id, self.target_model().to_string());
    }

    /// Stop the forwarder, send the unsent tail, decode, and inject.
    fn finalize_request(&self) -> Result<Option<String>, String> {
        let req_id = self
            .active_req
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let Some(req_id) = req_id else {
            return Ok(None);
        };
        if let Some(mut entry) = self
            .streams
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&req_id)
        {
            // Accidental taps skip the join: the forwarder may be blocked in
            // worker startup for seconds to transcribe nothing. Flagging off
            // makes it self-exit; nothing is joined, nothing is sent.
            if self.capture.buffer().len() < MIN_SAMPLES_COUNT {
                entry.progress.alive.store(false, Ordering::Relaxed);
                self.capture.buffer().drain();
                debug!("VoiceSessionManager: tap too short; skipping worker entirely");
                return Ok(None);
            }
            entry.progress.alive.store(false, Ordering::Relaxed);
            if let Some(handle) = entry.handle.take() {
                let _ = handle.join();
            }
            let drained = self.capture.buffer().drain();
            let sent = entry
                .progress
                .sent
                .load(Ordering::Relaxed)
                .min(drained.len());
            if drained.len() < MIN_SAMPLES_COUNT {
                debug!(
                    "VoiceSessionManager: audio buffer too short ({} samples); skipping STT",
                    drained.len()
                );
                return Ok(None);
            }
            let energy: f32 = drained.iter().map(|s| s * s).sum::<f32>() / drained.len() as f32;
            if energy < MIN_SPEECH_RMS_ENERGY {
                debug!(
                    "VoiceSessionManager: audio below RMS energy threshold ({:.6}); skipping STT",
                    energy
                );
                return Ok(None);
            }
            // The request snapshot wins over live settings: switching models
            // mid-record must not wipe the buffers under a live session.
            let target = entry.target.clone();
            self.worker.ensure_worker(&target)?;
            if sent < drained.len() {
                let seq = entry.progress.seq.load(Ordering::Relaxed);
                self.worker.append(&req_id, seq, &drained[sent..])?;
            }
            // Long hands-free audio needs headroom past old load budgets;
            // the worker already popped the buffer, so a timeout loses audio.
            let transcript = self.worker.transcribe(&req_id, Duration::from_secs(60))?;
            let mut meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
            *meta = Some(EngineMeta {
                model_id: target,
                last_used: Instant::now(),
            });
            drop(meta);
            self.inject_transcript(&transcript.text)
        } else {
            Ok(None)
        }
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
        let result = self.finalize_request();
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
                let result = self.finalize_request()?;
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
                self.begin_request();
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
        let result = self.finalize_request();
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            *mode = VoiceMode::Idle;
        }
        result
    }

    /// Cancel current voice recording immediately without transcribing or injecting text.
    ///
    /// Frees the worker-side buffer in the background so a cancelled
    /// recording never leaks audio state into the next session.
    pub fn cancel(&self) {
        let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
        *mode = VoiceMode::Idle;
        drop(mode);
        self.capture.stop();
        self.capture.buffer().clear();
        if let Some(req_id) = self
            .active_req
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            && let Some(entry) = self
                .streams
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&req_id)
        {
            entry.progress.alive.store(false, Ordering::Relaxed);
            if let Some(handle) = entry.handle {
                let _ = handle.join();
            }
            let worker = self.worker.clone();
            std::thread::Builder::new()
                .name("taurine-voice-discard".to_string())
                .spawn(move || {
                    let _ = worker.transcribe(&req_id, Duration::from_secs(5));
                })
                .ok();
        }
        crate::services::audio::play_voice_stop_cue();
        debug!("VoiceSessionManager: Dictation cancelled");
    }

    /// Apply dictionary corrections, spoken formatting, trigger matching,
    /// usage stats, and injection to worker-returned transcript text.
    ///
    /// Audio-side gating and recognition run in the `--voice-daemon`
    /// worker; this is the unchanged text-side pipeline.
    pub fn inject_transcript(&self, text: &str) -> Result<Option<String>, String> {
        if text.trim().is_empty() {
            debug!("VoiceSessionManager: transcription returned empty text");
            return Ok(None);
        }

        // Apply personal dictionary
        let dict = self.dictionary.lock().unwrap_or_else(|p| p.into_inner());
        let corrected = dict.apply(text);
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
            && let Ok(invocations) = taurine_core::db::crud::list_active_voice_invocations(&conn)
        {
            let in_scope: Vec<taurine_core::db::crud::ResolvedInvocation> = invocations
                .into_iter()
                .filter(|inv| {
                    (match (&inv.action.only_apps, &active_app) {
                        (Some(only), Some(app)) => {
                            only.split(',').any(|a| a.trim().eq_ignore_ascii_case(app))
                        }
                        _ => true,
                    }) && match (&inv.action.except_apps, &active_app) {
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
                let inv = matched.trigger;
                info!(
                    "VoiceSessionManager: matched voice trigger '{}' (score {:.2}); expanding output",
                    inv.invocation, matched.score
                );

                let output = inv.action.output.clone();
                crate::voice::fire_voice_trigger(
                    inv,
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
    pub(crate) fn set_test_meta(&self, model_id: &str, last_used: Instant) {
        *self.meta.lock().unwrap() = Some(EngineMeta {
            model_id: model_id.to_string(),
            last_used,
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
    fn test_session_with_dictionary_builder() {
        let (session, _) = create_test_session();
        let dict = VoiceDictionary::from_csv("Taurine, FastConformer");
        let session = session.with_dictionary(dict);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
    }

    /// Points TAURINE_DATA_DIR at a temp dir for the test body, restoring the
    /// previous value on drop. Serialized via TEST_LOCK by callers.
    struct TempDataDir {
        _dir: tempfile::TempDir,
        prev: Option<String>,
    }

    impl TempDataDir {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp data dir");
            let prev = std::env::var("TAURINE_DATA_DIR").ok();
            // SAFETY: Serialized via TEST_LOCK; restored in Drop.
            unsafe { std::env::set_var("TAURINE_DATA_DIR", dir.path()) };
            Self { _dir: dir, prev }
        }
    }

    impl Drop for TempDataDir {
        fn drop(&mut self) {
            // SAFETY: Serialized via TEST_LOCK; paired with the set_var at entry.
            unsafe {
                match &self.prev {
                    Some(prev) => std::env::set_var("TAURINE_DATA_DIR", prev),
                    None => std::env::remove_var("TAURINE_DATA_DIR"),
                }
            }
        }
    }

    /// Process-local mock OS keystore so the temp-DB open below never touches
    /// the real keystore (reads or first-run key creation). Installed once.
    mod mock_keystore {
        use std::collections::HashMap;
        use std::sync::{Mutex, Once, OnceLock};

        type PasswordMap = HashMap<(String, String), Vec<u8>>;

        const FIXED_TEST_DB_KEY_HEX: &[u8] =
            b"4343434343434343434343434343434343434343434343434343434343434343";

        static PASSWORDS: OnceLock<Mutex<PasswordMap>> = OnceLock::new();
        static INSTALL: Once = Once::new();

        fn passwords() -> &'static Mutex<PasswordMap> {
            PASSWORDS.get_or_init(|| {
                let mut map = HashMap::new();
                map.insert(
                    ("taurine".to_string(), "db-key".to_string()),
                    FIXED_TEST_DB_KEY_HEX.to_vec(),
                );
                Mutex::new(map)
            })
        }

        #[derive(Debug)]
        struct TestCredential {
            service: String,
            user: String,
        }

        impl keyring::credential::CredentialApi for TestCredential {
            fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
                passwords()
                    .lock()
                    .expect("test keystore poisoned")
                    .insert((self.service.clone(), self.user.clone()), secret.to_vec());
                Ok(())
            }

            fn get_secret(&self) -> keyring::Result<Vec<u8>> {
                passwords()
                    .lock()
                    .expect("test keystore poisoned")
                    .get(&(self.service.clone(), self.user.clone()))
                    .cloned()
                    .ok_or(keyring::Error::NoEntry)
            }

            fn delete_credential(&self) -> keyring::Result<()> {
                passwords()
                    .lock()
                    .expect("test keystore poisoned")
                    .remove(&(self.service.clone(), self.user.clone()))
                    .map(|_| ())
                    .ok_or(keyring::Error::NoEntry)
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(self, f)
            }
        }

        #[derive(Debug)]
        struct TestCredentialBuilder;

        impl keyring::credential::CredentialBuilderApi for TestCredentialBuilder {
            fn build(
                &self,
                _target: Option<&str>,
                service: &str,
                user: &str,
            ) -> keyring::Result<Box<keyring::credential::Credential>> {
                Ok(Box::new(TestCredential {
                    service: service.to_string(),
                    user: user.to_string(),
                }))
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn persistence(&self) -> keyring::credential::CredentialPersistence {
                keyring::credential::CredentialPersistence::ProcessOnly
            }
        }

        pub(super) fn use_mock_keystore() {
            INSTALL.call_once(|| {
                keyring::set_default_credential_builder(Box::new(TestCredentialBuilder));
            });
        }
    }

    #[test]
    fn test_inject_transcript_formats_and_returns_text() {
        // Hermetic: injection records to the fake injector, never the host;
        // stats land in a temp DB, never the real one; keystore is mocked.
        // Serialized: all three are process-global.
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, _) = create_test_session();
        crate::platform::test_injector().clear();
        let res = session.inject_transcript("hello world").unwrap();
        assert_eq!(res, Some("Hello world".to_string()));
        assert!(
            crate::platform::test_injector()
                .recorded()
                .iter()
                .any(|c| c.contains("Hello world")),
            "dictation must route through injection, got {:?}",
            crate::platform::test_injector().recorded()
        );
    }

    #[test]
    fn test_inject_transcript_empty_returns_none() {
        let (session, _) = create_test_session();
        assert_eq!(session.inject_transcript("   ").unwrap(), None);
    }

    #[test]
    fn test_meta_inactivity_eviction() {
        let (session, _) = create_test_session();
        let session = session.with_ttl(Duration::from_millis(10));
        let past = Instant::now() - Duration::from_millis(50);
        session.set_test_meta("parakeet-tdt-ctc-110m", past);
        assert!(session.meta.lock().unwrap().is_some());

        // Inactivity TTL unloads the single voice model for 0 MB idle RAM.
        session.clean_expired_transcriber();
        assert!(session.meta.lock().unwrap().is_none());

        // Fresh models within the TTL are retained.
        session.set_test_meta("parakeet-tdt-ctc-110m", Instant::now());
        session.clean_expired_transcriber();
        assert!(session.meta.lock().unwrap().is_some());

        // Explicit unload drops the model immediately.
        session.unload_model();
        assert!(session.meta.lock().unwrap().is_none());
    }

    #[test]
    fn test_active_session_never_evicts() {
        let (session, _) = create_test_session();
        let session = session.with_ttl(Duration::from_millis(10));
        let past = Instant::now() - Duration::from_millis(50);
        session.set_test_meta("parakeet-tdt-ctc-110m", past);

        // Recording pins the model: stale metadata survives while active.
        *session.mode.lock().unwrap() = VoiceMode::HandsFree;
        session.clean_expired_transcriber();
        assert!(session.meta.lock().unwrap().is_some());

        // Idle with the same stale timestamp evicts.
        *session.mode.lock().unwrap() = VoiceMode::Idle;
        session.clean_expired_transcriber();
        assert!(session.meta.lock().unwrap().is_none());
    }

    #[test]
    fn test_set_model_name_clears_meta() {
        let (session, _) = create_test_session();
        session.set_test_meta("parakeet-tdt-ctc-110m", Instant::now());
        assert!(session.meta.lock().unwrap().is_some());

        session.set_model_name("parakeet-tdt-ctc-110m");
        assert!(session.meta.lock().unwrap().is_none());
    }

    #[test]
    fn test_unload_model_clears_meta() {
        let (session, _) = create_test_session();
        session.set_test_meta("parakeet-tdt-ctc-110m", Instant::now());
        assert!(session.meta.lock().unwrap().is_some());

        session.unload_model();
        assert!(session.meta.lock().unwrap().is_none());
    }

    #[test]
    fn test_ensure_worker_ready_errors_without_worker() {
        // Hermetic: failing spawner, so no child process spawns. The error
        // must surface instead of hanging the caller.
        let buffer = Arc::new(super::super::capture::AudioFrameBuffer::new());
        let capture = Arc::new(AudioCapture::new(buffer));
        let paused = Arc::new(AtomicBool::new(false));
        let spawner: super::super::worker_client::SpawnFn =
            Arc::new(|_, _| Err(std::io::Error::other("no worker in tests")));
        let connector: super::super::worker_client::ConnectFn = Arc::new(|_, _, _, _| {
            Box::pin(async { Err("no worker in tests".to_string()) })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<_, String>> + Send>>
        });
        let worker = WorkerClient::with_hooks(spawner, connector).expect("test client");
        let session = VoiceSessionManager::new(capture, paused).with_worker(worker);
        assert!(session.ensure_worker_ready().is_err());
    }
}
