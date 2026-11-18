use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use taurine_core::voice::{VoiceDictionary, format_transcript};

use super::capture::AudioCapture;
use super::modes::VoiceMode;

/// Minimum audio samples required for transcription (100ms at 16kHz = 1600 samples).
pub(crate) const MIN_SAMPLES_COUNT: usize = 1600;

/// Mash window after a session end: presses inside it cue instantly but defer
/// engine work to a still-held rescue instead of churning the mic.
pub(crate) const MASH_WINDOW_MS: u64 = 300;
/// Still-held rescue delay: a gated press starts only if the key is still
/// down after this wait; a released key was a bounce and records nothing.
pub(crate) const RESCUE_WAIT_MS: u64 = 200;

/// Grace after key-up before a still-"recording" PTT session is declared
/// stranded by a lost key-release. Normal release-to-stop completes in
/// milliseconds; only a lost release can still be recording with the key
/// up this long.
const STUCK_PTT_GRACE: Duration = Duration::from_secs(10);
/// Ceiling for the Processing state.
const STUCK_PROCESSING_BASE: Duration = Duration::from_secs(90);

/// Monotonic timestamps marking one press-to-feedback cycle.
#[derive(Debug, Clone, Copy)]
struct PttTiming {
    press_at: Instant,
    cue_at: Instant,
    recording_at: Instant,
}

/// Completed press-to-feedback timestamps for one dictation.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestLatency {
    press_at: Instant,
    cue_at: Instant,
    recording_at: Instant,
    transcript_at: Instant,
}

/// Test-only mic-open cue stub (records call order instead of playing audio).
#[cfg(test)]
type CueHook = Arc<dyn Fn() + Send + Sync>;
/// Test-only capture-start stub (records call order, injects device errors).
#[cfg(test)]
type CaptureHook = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;
/// Test-only transcription stub.
#[cfg(test)]
type TranscribeHook = Arc<dyn Fn(&[f32]) -> Result<String, String> + Send + Sync>;

/// Manages the lifecycle of voice recording, dictation sessions, and text injection.
pub struct VoiceSessionManager {
    mode: Mutex<VoiceMode>,
    capture: Arc<AudioCapture>,
    paused: Arc<AtomicBool>,
    engine_state: Option<Arc<taurine_core::engine::EngineState>>,
    dictionary: Mutex<VoiceDictionary>,
    cloud_client: Arc<taurine_core::voice::GroqVoiceClient>,
    active_req: Mutex<Option<(String, PttTiming)>>,
    media: Arc<dyn super::media_suspend::MediaSuspender>,
    media_paused: Arc<std::sync::Mutex<Vec<super::media_suspend::PausedSession>>>,
    media_suspend_inflight: Arc<AtomicBool>,
    req_counter: AtomicU64,
    pending_press: AtomicBool,
    self_ref: OnceLock<Weak<VoiceSessionManager>>,
    last_stop: Mutex<Option<Instant>>,
    ptt_started_at: Mutex<Option<Instant>>,
    processing_since: Mutex<Option<Instant>>,
    #[cfg(test)]
    last_latency: Mutex<Option<RequestLatency>>,
    #[cfg(test)]
    last_drained: Mutex<Option<Vec<f32>>>,
    #[cfg(test)]
    cue_hook: Mutex<Option<CueHook>>,
    #[cfg(test)]
    capture_hook: Mutex<Option<CaptureHook>>,
    #[cfg(test)]
    stop_hook: Mutex<Option<CueHook>>,
    #[cfg(test)]
    error_hook: Mutex<Option<CueHook>>,
    #[cfg(test)]
    transcribe_hook: Mutex<Option<TranscribeHook>>,
}

impl VoiceSessionManager {
    /// Create a new session manager with an audio capture interface and pause flag.
    pub fn new(capture: Arc<AudioCapture>, paused: Arc<AtomicBool>) -> Self {
        Self {
            mode: Mutex::new(VoiceMode::Idle),
            capture,
            paused,
            engine_state: None,
            dictionary: Mutex::new(VoiceDictionary::default()),
            cloud_client: Arc::new(taurine_core::voice::GroqVoiceClient::from_env_or_keyring()),
            active_req: Mutex::new(None),
            #[cfg(test)]
            media: Arc::new(super::media_suspend::NoopMediaSuspender),
            #[cfg(not(test))]
            media: {
                #[cfg(windows)]
                let m: Arc<dyn super::media_suspend::MediaSuspender> =
                    Arc::new(super::media_suspend::SmtcMediaSuspender);
                #[cfg(not(windows))]
                let m: Arc<dyn super::media_suspend::MediaSuspender> =
                    Arc::new(super::media_suspend::NoopMediaSuspender);
                m
            },
            media_paused: Arc::new(Mutex::new(Vec::new())),
            media_suspend_inflight: Arc::new(AtomicBool::new(false)),
            req_counter: AtomicU64::new(0),
            pending_press: AtomicBool::new(false),
            self_ref: OnceLock::new(),
            last_stop: Mutex::new(None),
            ptt_started_at: Mutex::new(None),
            processing_since: Mutex::new(None),
            #[cfg(test)]
            last_latency: Mutex::new(None),
            #[cfg(test)]
            last_drained: Mutex::new(None),
            #[cfg(test)]
            cue_hook: Mutex::new(None),
            #[cfg(test)]
            capture_hook: Mutex::new(None),
            #[cfg(test)]
            stop_hook: Mutex::new(None),
            #[cfg(test)]
            error_hook: Mutex::new(None),
            #[cfg(test)]
            transcribe_hook: Mutex::new(None),
        }
    }
    pub fn with_engine_state(mut self, state: Arc<taurine_core::engine::EngineState>) -> Self {
        self.engine_state = Some(state);
        self
    }
    pub fn with_cloud_client(mut self, client: Arc<taurine_core::voice::GroqVoiceClient>) -> Self {
        self.cloud_client = client;
        #[cfg(test)]
        {
            *self
                .transcribe_hook
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = None;
        }
        self
    }

    /// True when voice must stay silent: ignore-fullscreen on and a fullscreen
    /// app focused. Reads the live atomics, so settings hot-reload applies.
    fn voice_suppressed(&self) -> bool {
        self.engine_state
            .as_ref()
            .is_some_and(|s| s.fullscreen_suppressed())
    }

    pub fn with_dictionary(self, dict: VoiceDictionary) -> Self {
        *self.dictionary.lock().unwrap_or_else(|p| p.into_inner()) = dict;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_media(
        mut self,
        media: Arc<dyn super::media_suspend::MediaSuspender>,
    ) -> Self {
        self.media = media;
        self
    }
    #[cfg(test)]
    pub(crate) fn with_cue_hook(self, hook: CueHook) -> Self {
        *self.cue_hook.lock().unwrap_or_else(|p| p.into_inner()) = Some(hook);
        self
    }

    /// Builder method to stub capture start (tests only, records order).
    #[cfg(test)]
    pub(crate) fn with_capture_hook(self, hook: CaptureHook) -> Self {
        *self.capture_hook.lock().unwrap_or_else(|p| p.into_inner()) = Some(hook);
        self
    }

    /// Builder method to stub capture stop (tests only, records order).
    #[cfg(test)]
    pub(crate) fn with_stop_hook(self, hook: CueHook) -> Self {
        *self.stop_hook.lock().unwrap_or_else(|p| p.into_inner()) = Some(hook);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_error_hook(self, hook: CueHook) -> Self {
        *self.error_hook.lock().unwrap_or_else(|p| p.into_inner()) = Some(hook);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_transcribe_hook(self, hook: TranscribeHook) -> Self {
        *self
            .transcribe_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(hook);
        self
    }
    /// Fire the mic-close cue (stubbed in tests to observe ordering).
    /// Silent while suppressed: stop paths stay functional over a fullscreen
    /// app, but play nothing over it.
    #[cfg(test)]
    fn fire_stop_cue(&self) {
        if self.voice_suppressed() {
            return;
        }
        if let Some(hook) = self
            .cue_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            hook();
        } else {
            crate::services::audio::play_voice_stop_cue();
        }
    }

    /// Fire the mic-close cue.
    /// Silent while suppressed: stop paths stay functional over a fullscreen
    /// app, but play nothing over it.
    #[cfg(not(test))]
    fn fire_stop_cue(&self) {
        if self.voice_suppressed() {
            debug!("VoiceSessionManager: stop cue skipped while fullscreen app focused");
            return;
        }
        crate::services::audio::play_voice_stop_cue();
    }

    #[cfg(test)]
    fn fire_error_cue(&self) {
        if self.voice_suppressed() {
            return;
        }
        if let Some(hook) = self
            .error_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            hook();
        } else {
            crate::services::audio::play_voice_error_cue();
        }
    }

    #[cfg(not(test))]
    fn fire_error_cue(&self) {
        if self.voice_suppressed() {
            debug!("VoiceSessionManager: error cue skipped while fullscreen app focused");
            return;
        }
        crate::services::audio::play_voice_error_cue();
    }

    /// Close the microphone (stubbed in tests to observe ordering).
    #[cfg(test)]
    fn stop_capture(&self) {
        if let Some(hook) = self
            .stop_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            hook();
        }
        self.capture.stop();
    }

    /// Close the microphone.
    #[cfg(not(test))]
    fn stop_capture(&self) {
        self.capture.stop();
    }

    /// Fire the mic-open cue (stubbed in tests to observe ordering).
    #[cfg(test)]
    fn fire_start_cue(&self) {
        if let Some(hook) = self
            .cue_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            hook();
        } else {
            crate::services::audio::play_voice_start_cue();
        }
    }

    /// Fire the mic-open cue.
    #[cfg(not(test))]
    fn fire_start_cue(&self) {
        crate::services::audio::play_voice_start_cue();
    }

    /// Open the microphone (stubbed in tests to observe ordering).
    #[cfg(test)]
    fn start_capture(&self) -> Result<(), String> {
        if let Some(hook) = self
            .capture_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            hook()
        } else {
            self.capture.start()
        }
    }

    /// Open the microphone.
    #[cfg(not(test))]
    fn start_capture(&self) -> Result<(), String> {
        self.capture.start()
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

    pub fn set_dictionary(&self, dict: VoiceDictionary) {
        *self.dictionary.lock().unwrap_or_else(|p| p.into_inner()) = dict;
    }

    /// Discovers and recovers any orphaned voice sessions from a previous crash or unexpected exit.
    pub fn recover_orphaned_sessions(&self) -> Result<Vec<String>, String> {
        let dict = self.dictionary.lock().unwrap_or_else(|p| p.into_inner());
        crate::voice::recovery::recover_orphaned_audio_sessions(&self.cloud_client, &dict)
    }

    /// Clean expired transcription state and recover capture devices.
    pub fn clean_expired_transcriber(&self) {
        if self.is_active() {
            self.recover_live_capture_device();
            self.recover_stranded_session();
        }
    }
    fn recover_live_capture_device(&self) {
        if !matches!(
            self.current_mode(),
            VoiceMode::PushToTalk | VoiceMode::HandsFree
        ) {
            return;
        }
        if self.capture.is_device_disconnected() {
            match self.capture.try_recover_device() {
                Err(e) => debug!("VoiceSessionManager: live mic recovery failed: {e}"),
                Ok(false) => {
                    debug!("VoiceSessionManager: no device ready; retrying on next sweep");
                }
                Ok(true) => {}
            }
        } else if self.capture.live_device_stale() {
            debug!("VoiceSessionManager: live mic stale after device change; reopening");
            if let Err(e) = self.capture.restart() {
                debug!("VoiceSessionManager: live mic reopen failed: {e}");
            }
        }
    }
    /// Last-resort recovery for sessions the event path lost: a PTT recording
    /// whose key is long released, or a Processing state older than any
    /// possible transcription.
    fn recover_stranded_session(&self) {
        match self.current_mode() {
            VoiceMode::PushToTalk => {
                let key_up = !crate::input::hotkey::PTT_KEY_DOWN.load(Ordering::Relaxed);
                let past_grace = self
                    .ptt_started_at
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .is_some_and(|t| t.elapsed() >= STUCK_PTT_GRACE);
                if key_up && past_grace {
                    warn!(
                        "VoiceSessionManager: PTT key released but session still recording; auto-stopping"
                    );
                    if let Err(e) = self.stop_ptt() {
                        warn!("VoiceSessionManager: stranded PTT auto-stop failed: {e}");
                    }
                }
            }
            VoiceMode::Processing => {
                let past_timeout = self
                    .processing_since
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .is_some_and(|t| t.elapsed() >= STUCK_PROCESSING_BASE);
                if past_timeout {
                    warn!(
                        "VoiceSessionManager: voice stuck processing with no owner; resetting to Idle"
                    );
                    self.reset_stranded_processing();
                }
            }
            _ => {}
        }
    }

    /// Reset a wedged Processing state.
    fn reset_stranded_processing(&self) {
        self.pending_press.store(false, Ordering::Relaxed);
        *self.mode.lock().unwrap_or_else(|p| p.into_inner()) = VoiceMode::Idle;
        *self
            .processing_since
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        self.note_session_end();
        self.stop_capture();
        self.resume_media();
        self.capture.buffer().clear();
        let _ = self
            .active_req
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        debug!("VoiceSessionManager: stranded session reset");
    }
    fn note_session_end(&self) {
        *self.last_stop.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
    }

    /// True when a session ended less than `MASH_WINDOW_MS` ago.
    fn in_mash_window(&self) -> bool {
        self.last_stop
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some_and(|t| t.elapsed() < Duration::from_millis(MASH_WINDOW_MS))
    }

    /// Publish the weak self-handle for the mash-gate rescue waiter.
    /// Call once after wrapping the manager in an `Arc`.
    pub fn set_self_ref(self: &Arc<Self>) {
        let _ = self.self_ref.set(Arc::downgrade(self));
    }

    /// Wrap a test-built manager for mash-gate tests (which need the rescue
    /// waiter to reach back into the session after its delay).
    #[cfg(test)]
    fn into_test_arc(self) -> Arc<Self> {
        let arc = Arc::new(self);
        arc.set_self_ref();
        arc
    }

    /// Deferred engine start for a mash-gated press. Returns immediately;
    /// after `RESCUE_WAIT_MS`, a still-held key with an Idle manager starts
    /// the recording it deferred. A released key records nothing. Upgrading
    /// the weak handle keeps a dropped manager dropped — the waiter never
    /// extends a lifetime, so it cannot outlive its session.
    fn spawn_mash_rescue(&self, handsfree: bool) {
        let Some(me) = self.self_ref.get().and_then(Weak::upgrade) else {
            return;
        };
        // Retry briefly: under parallel load thread creation can fail
        // transiently (like the chunk forwarder), and without the waiter a
        // held key never rescues the gated press.
        for _ in 0..5 {
            let waiter = Arc::clone(&me);
            let res = std::thread::Builder::new()
                .name("tau-ptt-rescue".to_string())
                .spawn(move || {
                    std::thread::sleep(Duration::from_millis(RESCUE_WAIT_MS));
                    let held = if handsfree {
                        crate::input::hotkey::HANDSFREE_KEY_DOWN.load(Ordering::Relaxed)
                    } else {
                        crate::input::hotkey::PTT_KEY_DOWN.load(Ordering::Relaxed)
                    };
                    if !held || waiter.is_paused() || waiter.current_mode() != VoiceMode::Idle {
                        return;
                    }
                    if handsfree {
                        if let Err(e) = waiter.start_handsfree_now() {
                            warn!("VoiceSessionManager: mash rescue hands-free start failed: {e}");
                        }
                    } else if let Err(e) = waiter.start_ptt_now(Instant::now(), Instant::now()) {
                        warn!("VoiceSessionManager: mash rescue PTT start failed: {e}");
                    }
                });
            if res.is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        warn!("VoiceSessionManager: failed to spawn mash rescue thread");
    }

    /// Honor a press parked while busy: when the previous session reaches
    /// Idle with the PTT key still physically held, begin the next recording
    /// immediately instead of forcing the user to re-press. A released key
    /// drops the pending press; silence means the user moved on.
    fn maybe_autostart(&self) {
        if !self.pending_press.swap(false, Ordering::Relaxed) {
            return;
        }
        if !crate::input::hotkey::PTT_KEY_DOWN.load(Ordering::Relaxed) {
            return;
        }
        if self.is_paused() {
            return;
        }
        // Explicit hold-continuation, not a new press: bypass the mash gate.
        if let Err(e) = self.start_ptt_now(Instant::now(), Instant::now()) {
            warn!("VoiceSessionManager: pending press auto-start failed: {e}");
        }
    }
    fn suspend_media(&self) {
        if !taurine_core::settings::get_cached_pause_media_while_dictating() {
            return;
        }
        if self.media_suspend_inflight.swap(true, Ordering::SeqCst) {
            return;
        }
        if !self
            .media_paused
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
        {
            self.media_suspend_inflight.store(false, Ordering::SeqCst);
            return;
        }
        let media = Arc::clone(&self.media);
        let paused_slot = Arc::clone(&self.media_paused);
        let inflight = Arc::clone(&self.media_suspend_inflight);
        if std::thread::Builder::new()
            .name("tau-media-suspend".to_string())
            .spawn(move || {
                let paused = media.suspend_playing();
                *paused_slot.lock().unwrap_or_else(|p| p.into_inner()) = paused;
                inflight.store(false, Ordering::SeqCst);
            })
            .is_err()
        {
            self.media_suspend_inflight.store(false, Ordering::SeqCst);
            debug!("media suspend: thread spawn failed");
        }
    }

    /// Resume exactly what suspend paused, on a background thread. The wait
    /// for an in-flight suspend lives inside the resume thread (bounded ~2s
    /// poll), never on the hotkey caller, so stops stay non-blocking while
    /// an immediate stop still resumes the token once it lands.
    fn resume_media(&self) {
        if !self.media_suspend_inflight.load(Ordering::SeqCst)
            && self
                .media_paused
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty()
        {
            return;
        }
        let media = Arc::clone(&self.media);
        let paused_slot = Arc::clone(&self.media_paused);
        let inflight = Arc::clone(&self.media_suspend_inflight);
        if std::thread::Builder::new()
            .name("tau-media-resume".to_string())
            .spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(2);
                while inflight.load(Ordering::SeqCst) {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let paused =
                    std::mem::take(&mut *paused_slot.lock().unwrap_or_else(|p| p.into_inner()));
                if paused.is_empty() {
                    return;
                }
                media.resume(&paused);
            })
            .is_err()
        {
            debug!("media resume: thread spawn failed");
        }
    }
    /// Return underlying audio capture handle.
    pub fn capture(&self) -> &Arc<AudioCapture> {
        &self.capture
    }

    /// Start a Push-To-Talk recording session.
    pub fn start_ptt(&self) -> Result<(), String> {
        let press_at = Instant::now();
        if self.is_paused() {
            debug!("VoiceSessionManager: start_ptt ignored because daemon is paused");
            return Ok(());
        }
        if self.voice_suppressed() {
            debug!("VoiceSessionManager: start_ptt ignored while fullscreen app focused");
            return Ok(());
        }

        self.fire_start_cue();
        let cue_at = Instant::now();

        {
            let mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::Idle {
                self.pending_press.store(true, Ordering::Relaxed);
                return Ok(());
            }
        }
        if self.in_mash_window() {
            self.spawn_mash_rescue(false);
            return Ok(());
        }
        self.start_ptt_now(press_at, cue_at)
    }

    /// Immediate engine start: open the device and begin recording.
    fn start_ptt_now(&self, press_at: Instant, cue_at: Instant) -> Result<(), String> {
        if self.voice_suppressed() {
            debug!("VoiceSessionManager: deferred PTT start ignored while fullscreen app focused");
            return Ok(());
        }
        {
            let mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::Idle {
                self.pending_press.store(true, Ordering::Relaxed);
                return Ok(());
            }
        }
        self.pending_press.store(false, Ordering::Relaxed);
        self.capture.buffer().clear();
        if let Err(e) = self.start_capture() {
            warn!("Failed to start audio capture stream for PTT: {e}");
            self.fire_error_cue();
            return Err(e);
        }
        let recording_at = Instant::now();
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::Idle {
                self.pending_press.store(true, Ordering::Relaxed);
                return Ok(());
            }
            *mode = VoiceMode::PushToTalk;
            *self
                .ptt_started_at
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
        }
        self.suspend_media();
        info!("VoiceSessionManager: Push-To-Talk recording started");
        self.begin_request(PttTiming {
            press_at,
            cue_at,
            recording_at,
        });
        Ok(())
    }

    /// Register a new streaming request.
    fn begin_request(&self, timing: PttTiming) {
        let req_id = format!("r{}", self.req_counter.fetch_add(1, Ordering::Relaxed));
        if let Ok(rec_dir) = crate::voice::recovery::recovery_dir()
            && let Ok(spooler) = taurine_core::voice::AudioSpooler::new(&rec_dir, &req_id)
        {
            self.capture.buffer().set_spooler(spooler);
        }
        *self.active_req.lock().unwrap_or_else(|p| p.into_inner()) = Some((req_id, timing));
    }

    /// Finalize request, decode audio, and inject.
    /// Transcribe captured audio samples using the cloud speech-to-text client,
    /// passing personal dictionary terms and active voice triggers as prompt biasing.
    pub fn transcribe_audio(&self, audio: &[f32]) -> Result<String, String> {
        let dict = self.dictionary.lock().unwrap_or_else(|p| p.into_inner());
        let triggers: Vec<String> = if let Ok(conn) = taurine_core::db::get_conn()
            && let Ok(invocations) = taurine_core::db::crud::list_active_voice_invocations(&conn)
        {
            invocations.into_iter().map(|inv| inv.invocation).collect()
        } else {
            Vec::new()
        };
        let prompt = taurine_core::voice::build_whisper_prompt(&dict, &triggers);
        let terms = dict.terms().to_vec();
        drop(dict);

        self.cloud_client
            .transcribe_blocking(audio, 16000, prompt.as_deref(), Some(&terms))
            .map_err(|e| e.to_string())
    }

    fn finalize_request(&self) -> Result<Option<String>, String> {
        let active = self
            .active_req
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        let Some((_req_id, timing)) = active else {
            return Ok(None);
        };

        if self.capture.buffer().len() < MIN_SAMPLES_COUNT {
            let _tapped = self.capture.buffer().drain();
            #[cfg(test)]
            {
                *self.last_drained.lock().unwrap_or_else(|p| p.into_inner()) = Some(_tapped);
            }
            debug!("VoiceSessionManager: tap too short; skipping worker entirely");
            return Ok(None);
        }

        let drained = self.capture.buffer().drain();
        #[cfg(test)]
        {
            *self.last_drained.lock().unwrap_or_else(|p| p.into_inner()) = Some(drained.clone());
        }

        if drained.len() < MIN_SAMPLES_COUNT {
            debug!(
                "VoiceSessionManager: audio buffer too short ({} samples); skipping STT",
                drained.len()
            );
            return Ok(None);
        }

        if !taurine_core::voice::has_voice_activity(&drained) {
            info!("VoiceSessionManager: no speech detected in audio (VAD gate); skipping STT");
            return Ok(None);
        }

        let transcript_text = {
            #[cfg(test)]
            {
                if let Some(hook) = self
                    .transcribe_hook
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone()
                {
                    hook(&drained)?
                } else {
                    self.transcribe_audio(&drained)?
                }
            }
            #[cfg(not(test))]
            {
                self.transcribe_audio(&drained)?
            }
        };

        let transcript_at = Instant::now();
        debug!(
            "VoiceSessionManager: voice latency press_to_cue_ms={:.2} press_to_recording_ms={:.2} press_to_transcript_ms={:.2}",
            timing.cue_at.duration_since(timing.press_at).as_secs_f64() * 1000.0,
            timing
                .recording_at
                .duration_since(timing.press_at)
                .as_secs_f64()
                * 1000.0,
            transcript_at.duration_since(timing.press_at).as_secs_f64() * 1000.0
        );
        #[cfg(test)]
        {
            *self.last_latency.lock().unwrap_or_else(|p| p.into_inner()) = Some(RequestLatency {
                press_at: timing.press_at,
                cue_at: timing.cue_at,
                recording_at: timing.recording_at,
                transcript_at,
            });
        }

        crate::voice::recovery::update_active_transcript_dump(&transcript_text);
        let res = self.inject_transcript(&transcript_text);
        crate::voice::recovery::clear_active_transcript_dump();
        res
    }

    /// Stop Push-To-Talk recording, transcribe captured speech, inject text, and return to `Idle`.
    pub fn stop_ptt(&self) -> Result<Option<String>, String> {
        self.fire_stop_cue();
        self.resume_media();
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::PushToTalk {
                return Ok(None);
            }
            *mode = VoiceMode::Processing;
        }
        *self
            .ptt_started_at
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        *self
            .processing_since
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());

        self.stop_capture();
        info!("VoiceSessionManager: Push-To-Talk recording stopped; processing audio");
        let result = self.finalize_request();
        if result.is_err() {
            self.fire_error_cue();
        }
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            *mode = VoiceMode::Idle;
        }
        *self
            .processing_since
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        *self
            .ptt_started_at
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        self.note_session_end();
        self.maybe_autostart();
        result
    }

    /// Immediate hands-free start.
    fn start_handsfree_now(&self) -> Result<(VoiceMode, Option<String>), String> {
        if self.voice_suppressed() {
            debug!(
                "VoiceSessionManager: deferred hands-free start ignored while fullscreen app focused"
            );
            return Ok((self.current_mode(), None));
        }
        let press_at = Instant::now();
        {
            let mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::Idle {
                self.pending_press.store(true, Ordering::Relaxed);
                return Ok((*mode, None));
            }
        }
        self.capture.buffer().clear();
        let cue_at = Instant::now();
        if let Err(e) = self.start_capture() {
            warn!("Failed to start audio capture stream for Hands-Free: {e}");
            self.fire_error_cue();
            return Err(e);
        }
        let recording_at = Instant::now();
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::Idle {
                self.pending_press.store(true, Ordering::Relaxed);
                return Ok((*mode, None));
            }
            *mode = VoiceMode::HandsFree;
        }
        self.suspend_media();
        info!("VoiceSessionManager: Hands-Free dictation activated");
        self.begin_request(PttTiming {
            press_at,
            cue_at,
            recording_at,
        });
        Ok((VoiceMode::HandsFree, None))
    }
    pub fn toggle_handsfree(&self) -> Result<(VoiceMode, Option<String>), String> {
        if self.is_paused() {
            debug!("VoiceSessionManager: toggle_handsfree ignored because daemon is paused");
            return Ok((VoiceMode::Idle, None));
        }
        // Start direction only: a live HandsFree session must always be able
        // to stop (the inject gate discards its audio when suppressed).
        {
            let mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode == VoiceMode::Idle && self.voice_suppressed() {
                debug!(
                    "VoiceSessionManager: toggle_handsfree ignored while fullscreen app focused"
                );
                return Ok((VoiceMode::Idle, None));
            }
        }

        // Direction cue first, so feedback never waits on locks or devices.
        {
            let mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            match *mode {
                VoiceMode::Idle => self.fire_start_cue(),
                VoiceMode::HandsFree => self.fire_stop_cue(),
                _ => {}
            }
        }

        let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
        match *mode {
            VoiceMode::HandsFree => {
                *mode = VoiceMode::Processing;
                drop(mode);
                *self
                    .ptt_started_at
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = None;
                *self
                    .processing_since
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
                self.stop_capture();
                self.resume_media();
                info!("VoiceSessionManager: Hands-Free dictation toggled off; processing audio");
                // Never strand the session in Processing: a worker failure
                // must degrade to one lost dictation, never to a voice path
                // that stays silent until restart while expansions (a
                // separate pipeline) keep working and hide the outage.
                let result = self.finalize_request();
                if result.is_err() {
                    self.fire_error_cue();
                }
                {
                    let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
                    *mode = VoiceMode::Idle;
                }
                *self
                    .processing_since
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = None;
                *self
                    .ptt_started_at
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = None;
                self.note_session_end();
                self.maybe_autostart();
                result.map(|text| (VoiceMode::Idle, text))
            }
            VoiceMode::Idle => {
                drop(mode);
                if self.in_mash_window() {
                    // Bounce: acknowledged by the entry cue, engine work
                    // deferred to the rescue waiter.
                    self.spawn_mash_rescue(true);
                    return Ok((VoiceMode::Idle, None));
                }
                self.start_handsfree_now()
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
        // Escape is the universal unsticker: a past toggle-off failure
        // could strand the session in Processing with no owner thread, and
        // is_active() keeps swallowing Escape presses while bricked. Reset
        // to Idle via cancel() instead of swallowing forever. When a live
        // decode owns Processing, cancel() is nearly a no-op (buffer
        // already drained, request already taken) and the decode still
        // injects on completion.
        if self.current_mode() == VoiceMode::Processing {
            warn!(
                "VoiceSessionManager: Escape pressed while in Processing; resetting voice to Idle"
            );
            self.cancel();
            self.resume_media();
            return Ok(None);
        }
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
        *self
            .ptt_started_at
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        *self
            .processing_since
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());

        // Escape aborts everything: a parked press dies with the session.
        self.pending_press.store(false, Ordering::Relaxed);
        self.fire_stop_cue();
        self.stop_capture();
        self.resume_media();
        info!("VoiceSessionManager: Escape pressed; terminating voice session and transcribing");
        let result = self.finalize_request();
        if result.is_err() {
            self.fire_error_cue();
        }
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            *mode = VoiceMode::Idle;
        }
        *self
            .processing_since
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        *self
            .ptt_started_at
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        self.note_session_end();
        result
    }
    /// Cancel current voice recording immediately without transcribing or injecting text.
    pub fn cancel(&self) {
        self.pending_press.store(false, Ordering::Relaxed);
        self.fire_stop_cue();
        let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
        let was_live = *mode != VoiceMode::Idle
            || self
                .active_req
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_some();
        *mode = VoiceMode::Idle;
        drop(mode);
        *self
            .processing_since
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        *self
            .ptt_started_at
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        if was_live {
            self.note_session_end();
        }
        self.stop_capture();
        self.resume_media();
        self.capture.buffer().clear();
        let _ = self
            .active_req
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        debug!("VoiceSessionManager: Dictation cancelled");
    }
    pub fn inject_transcript(&self, text: &str) -> Result<Option<String>, String> {
        if text.trim().is_empty() {
            debug!("VoiceSessionManager: transcription returned empty text");
            return Ok(None);
        }
        // Backstop for recordings that span a fullscreen transition: discard
        // before quota, dictionary, stats, trigger expansion, and injection.
        // Covers dictation and voice triggers — both funnel through here.
        if self.voice_suppressed() {
            debug!("VoiceSessionManager: discarding transcript while fullscreen app focused");
            return Ok(None);
        }

        // Fast pre-filter: stop early if weekly quota is exhausted or unauthenticated
        let guard = crate::engine::quota_guard::QuotaGuard::global();
        if !guard.can_expand() {
            if guard.is_authenticated() {
                warn!("Dictation dropped: weekly quota exhausted");
            } else {
                debug!("Please log in to use Taurine.");
            }
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
        if let Ok(mut conn) = taurine_core::db::get_conn()
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
                taurine_core::voice::rank_voice_invocations(&normalized_spoken, &in_scope)
            {
                let inv = matched.trigger;
                info!(
                    "VoiceSessionManager: matched voice trigger '{}' (score {:.2}); expanding output",
                    inv.invocation, matched.score
                );

                let output = crate::voice::fire_voice_trigger_with_args(
                    inv,
                    &matched.args,
                    &normalized_spoken,
                    &mut conn,
                    active_app.clone(),
                    taurine_core::settings::SpinnerStyle::default(),
                );
                return Ok(output);
            }
        }

        let final_text = trimmed;
        if final_text.is_empty() {
            return Ok(None);
        }

        let words_count = final_text.split_whitespace().count();

        // Inject text segment using canonical expansion pipeline with IS_INJECTING guard
        let report = crate::injector::inject_expansion_for_voice(
            vec![taurine_core::engine::variables::ExpansionStep::Text(
                final_text.to_string(),
            )],
            taurine_core::settings::SpinnerStyle::default(),
        );

        if !report.completed {
            warn!("Voice dictation injection incomplete or aborted; quota not deducted");
            return Ok(None);
        }

        // Deduct quota only after successful injection
        let guard = crate::engine::quota_guard::QuotaGuard::global();
        if !guard.deplete_words(words_count) {
            if guard.is_authenticated() {
                warn!("Dictation dropped: weekly quota exhausted");
            } else {
                debug!("Please log in to use Taurine.");
            }
            return Ok(None);
        }

        // Record voice dictation stats
        let chars_count = final_text.chars().count();

        taurine_core::db::crud::record_voice_dictation_usage(
            words_count,
            chars_count,
            active_app,
            Some(final_text.to_string()),
        );

        info!(
            "VoiceSessionManager: successfully injected {} words ({} chars)",
            words_count, chars_count
        );
        Ok(Some(final_text.to_string()))
    }
    #[cfg(test)]
    pub(crate) fn active_request(&self) -> Option<String> {
        self.active_req
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|(id, _)| id.clone())
    }

    #[cfg(test)]
    pub(crate) fn last_latency(&self) -> Option<RequestLatency> {
        *self.last_latency.lock().unwrap_or_else(|p| p.into_inner())
    }

    #[cfg(test)]
    pub(crate) fn last_drained(&self) -> Option<Vec<f32>> {
        self.last_drained
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn clear_last_stop_for_test(&self) {
        *self.last_stop.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    #[cfg(test)]
    pub(crate) fn set_ptt_started_for_test(&self, at: Instant) {
        *self
            .ptt_started_at
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(at);
    }

    #[cfg(test)]
    pub(crate) fn set_processing_since_for_test(&self, at: Instant) {
        *self
            .processing_since
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(at);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use taurine_core::voice::VoiceDictionary;

    fn wait_for_mode(session: &VoiceSessionManager, want: VoiceMode) -> VoiceMode {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let got = session.current_mode();
            if got == want || Instant::now() >= deadline {
                return got;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_request(session: &VoiceSessionManager) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if session.active_request().is_some() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn create_test_session() -> (VoiceSessionManager, Arc<AtomicBool>) {
        create_ordering_session(None, None, None)
    }

    fn create_latency_session() -> VoiceSessionManager {
        let (session, _) = create_test_session();
        session.with_transcribe_hook(Arc::new(|_| Ok("hello stub".to_string())))
    }

    fn suppressed_state() -> Arc<taurine_core::engine::EngineState> {
        let state = Arc::new(taurine_core::engine::EngineState::new());
        state.is_os_fullscreen.store(true, Ordering::Relaxed);
        state
    }
    #[test]
    fn voice_suppressed_defaults_to_false() {
        let (session, _) = create_test_session();
        assert!(!session.voice_suppressed());
    }

    #[test]
    fn voice_suppressed_follows_engine_state() {
        let (session, _) = create_test_session();
        let session = session.with_engine_state(suppressed_state());
        assert!(session.voice_suppressed());
        session
            .engine_state
            .as_ref()
            .expect("test wired engine state")
            .ignore_fullscreen_enabled
            .store(false, Ordering::Relaxed);
        assert!(!session.voice_suppressed());
    }

    /// Build a session recording start-cue and capture-start into one order
    /// log. Hooks replace the mic and speaker, so even an ungated press never
    /// touches the host in hermetic tests.
    fn create_start_ordering_session() -> (VoiceSessionManager, Arc<Mutex<Vec<&'static str>>>) {
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let cue_order = Arc::clone(&order);
        let cap_order = Arc::clone(&order);
        let (session, _) = create_ordering_session(
            Some(Arc::new(move || {
                cue_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("cue");
            })),
            Some(Arc::new(move || {
                cap_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("capture");
                Ok(())
            })),
            None,
        );
        (session, order)
    }

    fn start_order_of(order: &Arc<Mutex<Vec<&'static str>>>) -> Vec<&'static str> {
        order.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    #[test]
    fn suppressed_ptt_press_is_silent_noop() {
        let (session, order) = create_start_ordering_session();
        let session = session.with_engine_state(suppressed_state());
        session.start_ptt().unwrap();
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(session.capture().buffer().is_empty());
        assert!(
            start_order_of(&order).is_empty(),
            "suppressed PTT must fire no cue and open no capture, got {:?}",
            start_order_of(&order)
        );
    }

    #[test]
    fn suppressed_handsfree_toggle_from_idle_is_noop() {
        let (session, order) = create_start_ordering_session();
        let session = session.with_engine_state(suppressed_state());
        let (mode, text) = session.toggle_handsfree().unwrap();
        assert_eq!(mode, VoiceMode::Idle);
        assert_eq!(text, None);
        assert!(
            start_order_of(&order).is_empty(),
            "suppressed hands-free toggle must fire no cue, got {:?}",
            start_order_of(&order)
        );
    }

    #[test]
    fn suppressed_handsfree_toggle_off_still_stops() {
        // Empty buffer takes the tap-too-short path in finalize_request: no
        // worker needed, returns Ok(None), mode returns to Idle.
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, _) = create_test_session();
        let session = session.with_engine_state(suppressed_state());
        *session.mode.lock().unwrap() = VoiceMode::HandsFree;
        let (mode, text) = session.toggle_handsfree().unwrap();
        assert_eq!(mode, VoiceMode::Idle);
        assert_eq!(text, None);
    }

    #[test]
    fn suppressed_deferred_ptt_start_is_noop() {
        // Direct engine start (mash rescue, pending-press autostart) must
        // honor suppression even though it bypasses the gated entries.
        let (session, order) = create_start_ordering_session();
        let session = session.with_engine_state(suppressed_state());
        session
            .start_ptt_now(Instant::now(), Instant::now())
            .unwrap();
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(session.capture().buffer().is_empty());
        assert!(
            start_order_of(&order).is_empty(),
            "suppressed deferred PTT must not capture, got {:?}",
            start_order_of(&order)
        );
    }

    #[test]
    fn suppressed_deferred_handsfree_start_is_noop() {
        let (session, order) = create_start_ordering_session();
        let session = session.with_engine_state(suppressed_state());
        let (mode, text) = session.start_handsfree_now().unwrap();
        assert_eq!(mode, VoiceMode::Idle);
        assert_eq!(text, None);
        assert!(
            start_order_of(&order).is_empty(),
            "suppressed deferred hands-free must not capture, got {:?}",
            start_order_of(&order)
        );
    }

    #[test]
    fn suppressed_stop_ptt_is_silent() {
        // Stop paths stay functional but play no cue over a fullscreen app.
        let (session, order) = create_stop_ordering_session();
        let session = session.with_engine_state(suppressed_state());
        session.stop_ptt().unwrap();
        assert!(
            stop_order_of(&order).is_empty(),
            "suppressed stop must fire no cue, got {:?}",
            stop_order_of(&order)
        );
    }

    #[test]
    fn suppressed_inject_transcript_discards_without_injecting() {
        // Hermetic: the gate returns before DB, keystore, and injector
        // access, so only the injector recorder needs clearing for the
        // negative assert. Serialized on the global lock with the other
        // injector-recording tests.
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (session, _) = create_test_session();
        let session = session.with_engine_state(suppressed_state());
        crate::platform::test_injector().clear();
        let res = session.inject_transcript("hello world").unwrap();
        assert_eq!(res, None);
        assert!(
            crate::platform::test_injector().recorded().is_empty(),
            "suppressed dictation must not inject, got {:?}",
            crate::platform::test_injector().recorded()
        );
    }

    #[test]
    fn suppressed_inject_transcript_discards_matching_trigger() {
        // Hermetic: temp DB + mock keystore + recording injector, serialized
        // on the global lock like test_parameterized_voice_trigger_injects_args.
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let conn = taurine_core::db::get_conn().expect("temp db conn");
        taurine_core::db::crud::create_entry(
            &conn,
            taurine_core::db::crud::NewEntry {
                description: None,
                content: "Hello, [person=there]!".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                tags_json: "[]".to_string(),
                auto_case: false,
                interpreter: None,
                behavior: None,
                invocations: vec![(
                    taurine_core::db::crud::InvocationType::Voice,
                    "say hi to [person]".to_string(),
                    false,
                )],
            },
        )
        .expect("seed parameterized voice trigger");
        let (session, _) = create_test_session();
        let session = session.with_engine_state(suppressed_state());
        crate::platform::test_injector().clear();
        let res = session.inject_transcript("say hi to Bob").unwrap();
        assert_eq!(res, None);
        assert!(
            crate::platform::test_injector().recorded().is_empty(),
            "suppressed voice trigger must not inject, got {:?}",
            crate::platform::test_injector().recorded()
        );
    }

    fn create_ordering_session(
        cue_hook: Option<CueHook>,
        capture_hook: Option<CaptureHook>,
        stop_hook: Option<CueHook>,
    ) -> (VoiceSessionManager, Arc<AtomicBool>) {
        let buffer = Arc::new(super::super::capture::AudioFrameBuffer::new());
        let capture = Arc::new(AudioCapture::new(buffer));
        let paused = Arc::new(AtomicBool::new(false));
        let mut session = VoiceSessionManager::new(capture, paused.clone());
        session = session.with_transcribe_hook(Arc::new(|_| Ok("Mock transcription".to_string())));
        if let Some(hook) = cue_hook {
            session = session.with_cue_hook(hook);
        }
        if let Some(hook) = capture_hook {
            session = session.with_capture_hook(hook);
        }
        if let Some(hook) = stop_hook {
            session = session.with_stop_hook(hook);
        }
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
    fn test_handsfree_toggle_off_failure_resets_to_idle() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, _) =
            create_ordering_session(Some(Arc::new(|| {})), None, Some(Arc::new(|| {})));
        let session =
            session.with_transcribe_hook(Arc::new(|_| Err("no worker in tests".to_string())));
        let (mode, _) = session.toggle_handsfree().expect("toggle on");
        assert_eq!(mode, VoiceMode::HandsFree);

        session.capture.buffer().push_samples(&[0.5; 3200]);

        let err = session
            .toggle_handsfree()
            .expect_err("toggle off must surface the worker error");
        assert!(err.contains("no worker"), "unexpected error surface: {err}");
        assert_eq!(
            session.current_mode(),
            VoiceMode::Idle,
            "failed toggle-off must not strand Processing"
        );

        session.clear_last_stop_for_test();
        let (mode, _) = session.toggle_handsfree().expect("toggle on again");
        assert_eq!(mode, VoiceMode::HandsFree);
        session.cancel();
    }

    fn error_counter() -> (CueHook, Arc<AtomicUsize>) {
        let fired = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&fired);
        let hook: CueHook = Arc::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
        });
        (hook, fired)
    }

    #[test]
    fn test_stop_ptt_engine_failure_fires_error_cue() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (error_hook, fired) = error_counter();
        let (session, _) = create_ordering_session(None, Some(Arc::new(|| Ok(()))), None);
        let session = session
            .with_error_hook(error_hook)
            .with_transcribe_hook(Arc::new(|_| Err("no worker in tests".to_string())));
        session.start_ptt().expect("ptt start");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        session.capture.buffer().push_samples(&[0.5; 3200]);
        let err = session
            .stop_ptt()
            .expect_err("stop must surface the worker error");
        assert!(err.contains("no worker"), "unexpected error surface: {err}");
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert_eq!(
            fired.load(Ordering::Relaxed),
            1,
            "engine failure must fire error cue exactly once"
        );
    }

    #[test]
    fn test_handsfree_toggle_off_engine_failure_fires_error_cue() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (error_hook, fired) = error_counter();
        let (session, _) =
            create_ordering_session(Some(Arc::new(|| {})), None, Some(Arc::new(|| {})));
        let session = session
            .with_error_hook(error_hook)
            .with_transcribe_hook(Arc::new(|_| Err("no worker in tests".to_string())));
        let (mode, _) = session.toggle_handsfree().expect("toggle on");
        assert_eq!(mode, VoiceMode::HandsFree);
        session.capture.buffer().push_samples(&[0.5; 3200]);
        let err = session
            .toggle_handsfree()
            .expect_err("toggle off must surface the worker error");
        assert!(err.contains("no worker"), "unexpected error surface: {err}");
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert_eq!(
            fired.load(Ordering::Relaxed),
            1,
            "engine failure must fire error cue exactly once"
        );
        session.cancel();
    }

    #[test]
    fn test_escape_engine_failure_fires_error_cue() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (error_hook, fired) = error_counter();
        let (session, _) =
            create_ordering_session(Some(Arc::new(|| {})), None, Some(Arc::new(|| {})));
        let session = session
            .with_error_hook(error_hook)
            .with_transcribe_hook(Arc::new(|_| Err("no worker in tests".to_string())));
        session.start_ptt().expect("ptt start");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        session.capture.buffer().push_samples(&[0.5; 3200]);
        let err = session
            .on_escape_pressed()
            .expect_err("escape must surface the worker error");
        assert!(err.contains("no worker"), "unexpected error surface: {err}");
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert_eq!(
            fired.load(Ordering::Relaxed),
            1,
            "engine failure must fire error cue exactly once"
        );
    }
    #[test]
    fn test_stop_ptt_tap_short_stays_silent() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (error_hook, fired) = error_counter();
        let (session, _) = create_ordering_session(None, Some(Arc::new(|| Ok(()))), None);
        let session = session.with_error_hook(error_hook);
        session.start_ptt().expect("ptt start");
        let res = session.stop_ptt().expect("tap-short stop");
        assert_eq!(res, None);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert_eq!(fired.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn test_escape_recovers_stranded_processing() {
        let (session, _) = create_test_session();
        *session.mode.lock().unwrap() = VoiceMode::Processing;
        let res = session.on_escape_pressed().expect("escape");
        assert!(res.is_none());
        assert_eq!(session.current_mode(), VoiceMode::Idle);
    }

    #[test]
    fn test_start_ptt_fires_cue_before_capture_on_failure() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let cue_order = Arc::clone(&order);
        let cap_order = Arc::clone(&order);
        let (session, _) = create_ordering_session(
            Some(Arc::new(move || {
                cue_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("cue");
            })),
            Some(Arc::new(move || {
                cap_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("capture");
                Err("no device in tests".to_string())
            })),
            None,
        );
        let res = session.start_ptt();
        assert!(res.is_err());
        assert_eq!(
            *order.lock().unwrap_or_else(|p| p.into_inner()),
            vec!["cue", "capture"]
        );
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert_eq!(session.active_request(), None);
    }

    #[test]
    fn test_ptt_capture_failure_fires_error_cue() {
        let err_order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let err_clone = Arc::clone(&err_order);
        let (session, _) = create_ordering_session(
            Some(Arc::new(move || {
                err_clone
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("cue");
            })),
            Some(Arc::new(|| Err("no mic".to_string()))),
            None,
        );
        let session = session.with_error_hook(Arc::new({
            let err_order = Arc::clone(&err_order);
            move || {
                err_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("error");
            }
        }));
        let res = session.start_ptt_now(Instant::now(), Instant::now());
        assert!(res.is_err());
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        let got = err_order.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert!(
            got.contains(&"error"),
            "capture failure must fire error cue, got {got:?}"
        );
    }

    #[test]
    fn test_start_ptt_fires_cue_before_capture_on_success() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let cue_order = Arc::clone(&order);
        let cap_order = Arc::clone(&order);
        let (session, _) = create_ordering_session(
            Some(Arc::new(move || {
                cue_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("cue");
            })),
            Some(Arc::new(move || {
                cap_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("capture");
                Ok(())
            })),
            None,
        );
        let res = session.start_ptt();
        assert!(res.is_ok());
        assert_eq!(
            *order.lock().unwrap_or_else(|p| p.into_inner()),
            vec!["cue", "capture"]
        );
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        assert!(session.active_request().is_some());
        session.cancel();
    }

    /// Build an ordering session whose stop cue pushes "stop-cue" and whose
    /// capture stop pushes "stop" into one shared order log.
    fn create_stop_ordering_session() -> (VoiceSessionManager, Arc<Mutex<Vec<&'static str>>>) {
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let cue_order = Arc::clone(&order);
        let stop_order = Arc::clone(&order);
        let (session, _) = create_ordering_session(
            Some(Arc::new(move || {
                cue_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("stop-cue");
            })),
            None,
            Some(Arc::new(move || {
                stop_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("stop");
            })),
        );
        (session, order)
    }

    fn stop_order_of(order: &Arc<Mutex<Vec<&'static str>>>) -> Vec<&'static str> {
        order.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    #[test]
    fn test_stop_ptt_fires_stop_cue_before_capture_stop() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, order) = create_stop_ordering_session();
        *session.mode.lock().unwrap() = VoiceMode::PushToTalk;
        let res = session.stop_ptt();
        assert!(res.is_ok());
        assert_eq!(stop_order_of(&order), vec!["stop-cue", "stop"]);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
    }

    #[test]
    fn test_handsfree_off_fires_stop_cue_before_capture_stop() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, order) = create_stop_ordering_session();
        *session.mode.lock().unwrap() = VoiceMode::HandsFree;
        let (mode, _) = session.toggle_handsfree().expect("toggle off");
        assert_eq!(mode, VoiceMode::Idle);
        assert_eq!(stop_order_of(&order), vec!["stop-cue", "stop"]);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
    }

    #[test]
    fn test_escape_fires_stop_cue_before_capture_stop() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, order) = create_stop_ordering_session();
        *session.mode.lock().unwrap() = VoiceMode::PushToTalk;
        let res = session.on_escape_pressed();
        assert!(res.is_ok());
        assert_eq!(stop_order_of(&order), vec!["stop-cue", "stop"]);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
    }

    #[test]
    fn test_cancel_fires_stop_cue_before_capture_stop() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, order) = create_stop_ordering_session();
        *session.mode.lock().unwrap() = VoiceMode::PushToTalk;
        session.cancel();
        assert_eq!(stop_order_of(&order), vec!["stop-cue", "stop"]);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
    }

    /// Poll until the media guard holds a suspend token (or a 5 s deadline
    /// expires). Suspend runs on a background thread, so tests must wait for
    /// it before stopping; otherwise the stop drain races the suspend write.
    fn wait_for_media_paused(session: &VoiceSessionManager) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if !session
                .media_paused
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty()
            {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Poll until `counter` reaches `want` (or a 5 s deadline expires).
    /// Resume runs on a background thread, so tests must wait for it.
    fn wait_for_counter(counter: &AtomicUsize, want: usize) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if counter.load(Ordering::Relaxed) >= want {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Poll until the mash gate clears (or a 5 s deadline expires). A stop
    /// stamps `last_stop`, so an immediate restart would defer to the rescue
    /// waiter instead of starting; race tests must wait for a genuine start.
    fn wait_for_mash_clear(session: &VoiceSessionManager) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if !session.in_mash_window() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Counting suspender for tests where `FakeMediaSuspender` cannot
    /// observe: it records suspend calls, not just resume tokens.
    struct CountingMedia {
        suspends: AtomicUsize,
        resumes: AtomicUsize,
    }

    impl super::super::media_suspend::MediaSuspender for CountingMedia {
        fn suspend_playing(&self) -> Vec<super::super::media_suspend::PausedSession> {
            self.suspends.fetch_add(1, Ordering::Relaxed);
            vec![super::super::media_suspend::PausedSession {
                source_id: "fake-player".to_string(),
                display_name: "fake".to_string(),
            }]
        }
        fn resume(&self, sessions: &[super::super::media_suspend::PausedSession]) {
            self.resumes.fetch_add(sessions.len(), Ordering::Relaxed);
        }
    }

    /// Suspender whose suspend blocks until released, forcing the
    /// start/stop race deterministically: the stop drain runs while the
    /// suspend write is still in flight. Hermetic: plain atomics, no host.
    struct GatedMedia {
        suspends: AtomicUsize,
        resumes: AtomicUsize,
        release: AtomicBool,
    }

    impl super::super::media_suspend::MediaSuspender for GatedMedia {
        fn suspend_playing(&self) -> Vec<super::super::media_suspend::PausedSession> {
            self.suspends.fetch_add(1, Ordering::Relaxed);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self.release.load(Ordering::Relaxed) {
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            vec![super::super::media_suspend::PausedSession {
                source_id: "fake-player".to_string(),
                display_name: "fake".to_string(),
            }]
        }
        fn resume(&self, sessions: &[super::super::media_suspend::PausedSession]) {
            self.resumes.fetch_add(sessions.len(), Ordering::Relaxed);
        }
    }

    #[test]
    fn test_media_suspends_on_start_and_resumes_on_stop() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        taurine_core::settings::set_cached_pause_media_while_dictating(true);
        let concrete = Arc::new(super::super::media_suspend::FakeMediaSuspender::new());
        let fake: Arc<dyn super::super::media_suspend::MediaSuspender> = concrete.clone();
        let (session, _) = create_ordering_session(
            Some(Arc::new(|| {})),
            Some(Arc::new(|| Ok(()))),
            Some(Arc::new(|| {})),
        );
        let session = session.with_media(fake);
        session.start_ptt().expect("ptt start");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        assert!(
            wait_for_media_paused(&session),
            "suspend must land after start"
        );
        assert_eq!(
            session
                .media_paused
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .len(),
            1,
            "fake suspender pauses exactly one session"
        );
        // Empty buffer takes the tap-too-short path: Ok(None), no injection.
        let res = session.stop_ptt().expect("tap-short stop");
        assert_eq!(res, None);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(
            wait_for_counter(&concrete.resumes, 1),
            "resume must fire after stop"
        );
        assert_eq!(concrete.resumes.load(Ordering::Relaxed), 1);
        taurine_core::settings::set_cached_pause_media_while_dictating(false);
    }

    #[test]
    fn test_media_silent_when_setting_off() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        taurine_core::settings::set_cached_pause_media_while_dictating(false);
        let concrete = Arc::new(super::super::media_suspend::FakeMediaSuspender::new());
        let fake: Arc<dyn super::super::media_suspend::MediaSuspender> = concrete.clone();
        let (session, _) = create_ordering_session(
            Some(Arc::new(|| {})),
            Some(Arc::new(|| Ok(()))),
            Some(Arc::new(|| {})),
        );
        let session = session.with_media(fake);
        session.start_ptt().expect("ptt start");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        // No suspend thread is spawned while off; sleep past any spawn delay
        // so a stray suspend would have landed before asserting silence.
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            session
                .media_paused
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty(),
            "setting off must never suspend"
        );
        let res = session.stop_ptt().expect("tap-short stop");
        assert_eq!(res, None);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            concrete.resumes.load(Ordering::Relaxed),
            0,
            "setting off must never resume"
        );
        taurine_core::settings::set_cached_pause_media_while_dictating(false);
    }

    #[test]
    fn test_media_resumes_on_cancel_and_escape() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        taurine_core::settings::set_cached_pause_media_while_dictating(true);
        // Cancel path: fresh session so the mash gate cannot swallow the start.
        let concrete = Arc::new(super::super::media_suspend::FakeMediaSuspender::new());
        let fake: Arc<dyn super::super::media_suspend::MediaSuspender> = concrete.clone();
        let (session, _) = create_ordering_session(
            Some(Arc::new(|| {})),
            Some(Arc::new(|| Ok(()))),
            Some(Arc::new(|| {})),
        );
        let session = session.with_media(fake);
        session.start_ptt().expect("ptt start");
        assert!(
            wait_for_media_paused(&session),
            "suspend must land before cancel"
        );
        session.cancel();
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(wait_for_counter(&concrete.resumes, 1), "cancel must resume");
        // Escape path: another fresh session, same shape.
        let concrete = Arc::new(super::super::media_suspend::FakeMediaSuspender::new());
        let fake: Arc<dyn super::super::media_suspend::MediaSuspender> = concrete.clone();
        let (session, _) = create_ordering_session(
            Some(Arc::new(|| {})),
            Some(Arc::new(|| Ok(()))),
            Some(Arc::new(|| {})),
        );
        let session = session.with_media(fake);
        session.start_ptt().expect("ptt start");
        assert!(
            wait_for_media_paused(&session),
            "suspend must land before escape"
        );
        let res = session.on_escape_pressed().expect("escape");
        assert_eq!(res, None);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(wait_for_counter(&concrete.resumes, 1), "escape must resume");
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        taurine_core::settings::set_cached_pause_media_while_dictating(false);
    }

    #[test]
    fn test_media_no_double_pause_on_busy_press() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        taurine_core::settings::set_cached_pause_media_while_dictating(true);
        let concrete = Arc::new(CountingMedia {
            suspends: AtomicUsize::new(0),
            resumes: AtomicUsize::new(0),
        });
        let media: Arc<dyn super::super::media_suspend::MediaSuspender> = concrete.clone();
        let (session, _) = create_ordering_session(
            Some(Arc::new(|| {})),
            Some(Arc::new(|| Ok(()))),
            Some(Arc::new(|| {})),
        );
        let session = session.with_media(media);
        session.start_ptt().expect("first press starts");
        assert!(
            wait_for_counter(&concrete.suspends, 1),
            "first press must suspend"
        );
        // Busy press parks as pending without reaching engine start.
        session.start_ptt().expect("busy press is silent");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            concrete.suspends.load(Ordering::Relaxed),
            1,
            "busy press must not suspend twice"
        );
        session.cancel();
        assert!(
            wait_for_counter(&concrete.resumes, 1),
            "cancel must resume the single pause"
        );
        assert_eq!(concrete.resumes.load(Ordering::Relaxed), 1);
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        taurine_core::settings::set_cached_pause_media_while_dictating(false);
    }

    #[test]
    fn test_media_immediate_stop_then_restart_pauses_fresh() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        taurine_core::settings::set_cached_pause_media_while_dictating(true);
        let concrete = Arc::new(GatedMedia {
            suspends: AtomicUsize::new(0),
            resumes: AtomicUsize::new(0),
            release: AtomicBool::new(false),
        });
        let media: Arc<dyn super::super::media_suspend::MediaSuspender> = concrete.clone();
        let (session, _) = create_ordering_session(
            Some(Arc::new(|| {})),
            Some(Arc::new(|| Ok(()))),
            Some(Arc::new(|| {})),
        );
        let session = session.with_media(media);
        session.start_ptt().expect("first press starts");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        // Immediate stop with the suspend still gated: the stop drain races
        // the in-flight suspend write (the stale-guard bug drained empty).
        let res = session.stop_ptt().expect("tap-short stop");
        assert_eq!(res, None);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        concrete.release.store(true, Ordering::Relaxed);
        // The exit must still resume exactly the one pause once it lands.
        assert!(
            wait_for_counter(&concrete.resumes, 1),
            "immediate stop must resume the in-flight pause"
        );
        assert!(
            session
                .media_paused
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty(),
            "no stale guard may linger while idle"
        );
        assert!(
            wait_for_mash_clear(&session),
            "mash window must clear before the restart"
        );
        session.start_ptt().expect("restart starts");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        assert!(
            wait_for_counter(&concrete.suspends, 2),
            "restart must pause exactly once per real start"
        );
        assert!(wait_for_media_paused(&session), "restart suspend must land");
        session.cancel();
        assert!(
            wait_for_counter(&concrete.resumes, 2),
            "restart stop must resume"
        );
        assert!(
            session
                .media_paused
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty(),
            "no stale guard may linger after restart"
        );
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        taurine_core::settings::set_cached_pause_media_while_dictating(false);
    }

    #[test]
    fn test_handsfree_start_fires_cue_before_capture() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let cue_order = Arc::clone(&order);
        let cap_order = Arc::clone(&order);
        let stop_order = Arc::clone(&order);
        let (session, _) = create_ordering_session(
            Some(Arc::new(move || {
                cue_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("cue");
            })),
            Some(Arc::new(move || {
                cap_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("capture");
                Ok(())
            })),
            Some(Arc::new(move || {
                stop_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("stop");
            })),
        );
        let (mode, _) = session.toggle_handsfree().expect("toggle on");
        assert_eq!(mode, VoiceMode::HandsFree);
        assert_eq!(stop_order_of(&order), vec!["cue", "capture"]);
        // Cancel reuses the same cue hook, so its stop cue logs "cue" too;
        // the assertion that matters is cue-before-stop on the cancel path.
        session.cancel();
        assert_eq!(stop_order_of(&order), vec!["cue", "capture", "cue", "stop"]);
    }

    #[test]
    fn test_busy_press_records_pending_without_starting() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let cue_order = Arc::clone(&order);
        let (session, _) = create_ordering_session(
            Some(Arc::new(move || {
                cue_order
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push("cue");
            })),
            None,
            None,
        );
        *session.mode.lock().unwrap() = VoiceMode::Processing;
        let res = session.start_ptt();
        assert!(res.is_ok());
        // The press is acknowledged even though no session can start.
        assert_eq!(stop_order_of(&order), vec!["cue"]);
        assert_eq!(session.current_mode(), VoiceMode::Processing);
        assert!(session.active_request().is_none());
    }

    #[test]
    fn test_stop_ptt_idle_still_cues_release() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, order) = create_stop_ordering_session();
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        let res = session.stop_ptt();
        assert!(res.is_ok());
        // Nothing to stop, but the release itself is acknowledged.
        assert_eq!(stop_order_of(&order), vec!["stop-cue"]);
        assert_eq!(session.current_mode(), VoiceMode::Idle);
    }

    #[test]
    fn test_stop_autostarts_when_key_still_held() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let opens = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&opens);
        let (session, _) = create_ordering_session(
            None,
            Some(Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })),
            None,
        );
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        // Swallowed press while busy records a pending start.
        *session.mode.lock().unwrap() = VoiceMode::Processing;
        session.start_ptt().expect("busy press is silent");
        // Previous session stops with the key still physically held.
        *session.mode.lock().unwrap() = VoiceMode::PushToTalk;
        crate::input::hotkey::PTT_KEY_DOWN.store(true, Ordering::Relaxed);
        session.stop_ptt().expect("stop");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        assert!(session.active_request().is_some());
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        session.cancel();
    }

    #[test]
    fn test_stop_drops_pending_when_key_released() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, _) = create_ordering_session(None, Some(Arc::new(move || Ok(()))), None);
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        *session.mode.lock().unwrap() = VoiceMode::Processing;
        session.start_ptt().expect("busy press is silent");
        *session.mode.lock().unwrap() = VoiceMode::PushToTalk;
        session.stop_ptt().expect("stop");
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(session.active_request().is_none());
        // No stuck pending: a fresh press starts normally afterwards.
        session.clear_last_stop_for_test();
        session.start_ptt().expect("fresh press");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        session.cancel();
    }

    #[test]
    fn test_mash_press_gated_but_cue_fires() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        let cues = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&cues);
        let (session, _) = create_ordering_session(
            Some(Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            })),
            Some(Arc::new(move || Ok(()))),
            None,
        );
        let session = session.into_test_arc();
        session.start_ptt().expect("first press");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        session.stop_ptt().expect("stop");
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        let cued = cues.load(Ordering::SeqCst);
        // Immediate repress is a bounce: the press is acknowledged, but no
        // engine work starts.
        session.start_ptt().expect("mash press cues");
        assert_eq!(
            cues.load(Ordering::SeqCst),
            cued + 1,
            "mash press must still fire the start cue"
        );
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(session.active_request().is_none());
        // Key released: the rescue waiter must record nothing. Sleep past the
        // rescue wait so the detached waiter never outlives this session.
        std::thread::sleep(Duration::from_millis(RESCUE_WAIT_MS + 100));
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
    }

    #[test]
    fn test_mash_rescue_starts_when_key_held() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let session = create_latency_session().into_test_arc();
        crate::platform::test_injector().clear();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        session.start_ptt().expect("first press");
        session.stop_ptt().expect("stop");
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        // Mash repress, key held: engine work defers to the rescue waiter.
        // Re-stamp the gate explicitly: the stop-to-repress gap is
        // microseconds in practice, but a descheduled test thread could age
        // past MASH_WINDOW_MS and take the direct path instead.
        session.note_session_end();
        crate::input::hotkey::PTT_KEY_DOWN.store(true, Ordering::Relaxed);
        let mash_at = Instant::now();
        session.start_ptt().expect("mash press");
        // Engine work must defer: immediately after the mash press no
        // request exists yet. Gated on elapsed time so a descheduled test
        // thread past RESCUE_WAIT_MS (rescue legitimately fired) never flakes.
        if mash_at.elapsed() < Duration::from_millis(RESCUE_WAIT_MS / 2) {
            assert_eq!(session.current_mode(), VoiceMode::Idle);
            assert!(session.active_request().is_none());
        }
        assert_eq!(
            wait_for_mode(&session, VoiceMode::PushToTalk),
            VoiceMode::PushToTalk,
            "held key must rescue the gated press"
        );
        assert!(
            wait_for_request(&session),
            "rescued request must register before stop"
        );
        session.capture().buffer().push_samples(&[0.5; 3200]);
        let res = session.stop_ptt().expect("stop rescued");
        assert!(res.is_some(), "rescued recording must transcribe");
        let drained = session.last_drained().expect("drained audio recorded");
        assert_eq!(
            drained.len(),
            3200,
            "rescued recording starts clean with no pre-roll tail"
        );
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
    }

    #[test]
    fn test_press_after_mash_window_starts_instantly() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        let (session, _) = create_ordering_session(None, Some(Arc::new(move || Ok(()))), None);
        session.start_ptt().expect("first press");
        session.stop_ptt().expect("stop");
        std::thread::sleep(Duration::from_millis(MASH_WINDOW_MS + 50));
        session.start_ptt().expect("fresh press");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        assert!(session.active_request().is_some());
        // Cold mic: the buffer was drained at the last stop, so the snapshot
        // is empty and only newly pushed speech is buffered.
        session.capture().buffer().push_samples(&[0.5; 3200]);
        assert_eq!(session.capture().buffer().len(), 3200);
        session.cancel();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
    }

    #[test]
    fn test_room_tone_plus_speech_passes_energy_gate() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let session = create_latency_session();
        crate::platform::test_injector().clear();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        session.start_ptt().expect("start");
        session.capture().buffer().push_samples(&[0.002; 6400]);
        session.capture().buffer().push_samples(&[0.5; 3200]);
        let res = session.stop_ptt().expect("stop");
        assert!(
            res.is_some(),
            "400 ms of room tone plus speech must transcribe"
        );
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
    }
    #[test]
    fn test_pure_silence_skips_transcription() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, _) = create_ordering_session(None, Some(Arc::new(move || Ok(()))), None);
        session.start_ptt().expect("start");
        session.capture().buffer().push_samples(&[0.0; 9600]);
        let res = session.stop_ptt().expect("stop");
        assert!(res.is_none(), "pure silence must skip STT");
    }

    #[test]
    fn test_handsfree_mash_gate_and_rescue() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        crate::input::hotkey::HANDSFREE_KEY_DOWN.store(false, Ordering::Relaxed);
        let (session, _) = create_ordering_session(None, Some(Arc::new(move || Ok(()))), None);
        let session = session.into_test_arc();
        let (mode, _) = session.toggle_handsfree().expect("toggle on");
        assert_eq!(mode, VoiceMode::HandsFree);
        let (mode, _) = session.toggle_handsfree().expect("toggle off");
        assert_eq!(mode, VoiceMode::Idle);
        // Immediate re-toggle is a bounce: cue fired at entry, engine gated.
        // Re-stamp the gate explicitly so a descheduled test thread cannot
        // age past MASH_WINDOW_MS and take the direct path instead.
        session.note_session_end();
        crate::input::hotkey::HANDSFREE_KEY_DOWN.store(true, Ordering::Relaxed);
        let (mode, _) = session.toggle_handsfree().expect("mash toggle");
        assert_eq!(mode, VoiceMode::Idle);
        assert_eq!(
            wait_for_mode(&session, VoiceMode::HandsFree),
            VoiceMode::HandsFree,
            "held key must rescue the gated hands-free press"
        );
        crate::input::hotkey::HANDSFREE_KEY_DOWN.store(false, Ordering::Relaxed);
        let (mode, _) = session.toggle_handsfree().expect("toggle off");
        assert_eq!(mode, VoiceMode::Idle);
    }

    #[test]
    fn test_escape_clears_pending_without_autostart() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, _) = create_test_session();
        crate::input::hotkey::PTT_KEY_DOWN.store(true, Ordering::Relaxed);
        *session.mode.lock().unwrap() = VoiceMode::Processing;
        session.start_ptt().expect("busy press is silent");
        *session.mode.lock().unwrap() = VoiceMode::PushToTalk;
        session.on_escape_pressed().expect("escape");
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(session.active_request().is_none());
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
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
        // Headless Linux CI has no display, so injection falls back to direct
        // typing and never touches the clipboard; nothing to assert there.
        if cfg!(target_os = "linux")
            && std::env::var("DISPLAY").is_err()
            && std::env::var("WAYLAND_DISPLAY").is_err()
        {
            return;
        }
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
                .any(|c| c == "paste"),
            "dictation must route through injection via paste, got {:?}",
            crate::platform::test_injector().recorded()
        );
    }

    #[test]
    fn test_inject_transcript_empty_returns_none() {
        let (session, _) = create_test_session();
        assert_eq!(session.inject_transcript("   ").unwrap(), None);
    }

    #[test]
    fn test_inject_transcript_blocked_when_quota_exhausted() {
        // Hermetic: fake injector, temp DB, mock keystore; serialized because
        // the quota guard is process-global.
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, _) = create_test_session();
        crate::platform::test_injector().clear();

        let guard = crate::engine::quota_guard::QuotaGuard::global();
        let original_balance = guard.balance();
        let original_unlimited = guard.is_unlimited();
        let original_authenticated = guard.is_authenticated();
        guard.set_unlimited(false);
        guard.set_authenticated(true);
        guard.set_balance(0.0);

        let result = session.inject_transcript("hello world");

        guard.set_balance(original_balance);
        guard.set_unlimited(original_unlimited);
        guard.set_authenticated(original_authenticated);

        assert_eq!(result.unwrap(), None);
        assert!(
            crate::platform::test_injector().recorded().is_empty(),
            "exhausted quota must not inject, got {:?}",
            crate::platform::test_injector().recorded()
        );
    }

    #[test]
    fn test_parameterized_voice_trigger_injects_args() {
        // Hermetic: temp DB + mock keystore + recording injector, serialized
        // on the global lock like test_inject_transcript_formats_and_returns_text.
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let conn = taurine_core::db::get_conn().expect("temp db conn");
        taurine_core::db::crud::create_entry(
            &conn,
            taurine_core::db::crud::NewEntry {
                description: None,
                content: "Hello, [person=there]!".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                tags_json: "[]".to_string(),
                auto_case: false,
                interpreter: None,
                behavior: None,
                invocations: vec![(
                    taurine_core::db::crud::InvocationType::Voice,
                    "say hi to [person]".to_string(),
                    false,
                )],
            },
        )
        .expect("seed parameterized voice trigger");
        let (session, _) = create_test_session();
        let res = session.inject_transcript("say hi to Bob").unwrap();
        assert_eq!(res, Some("Hello, bob!".to_string()));
    }

    #[test]
    fn test_voice_trigger_soft_landing_when_low_quota() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let conn = taurine_core::db::get_conn().expect("temp db conn");
        taurine_core::db::crud::create_entry(
            &conn,
            taurine_core::db::crud::NewEntry {
                description: None,
                content: "Expanded result".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                tags_json: "[]".to_string(),
                auto_case: false,
                interpreter: None,
                behavior: None,
                invocations: vec![(
                    taurine_core::db::crud::InvocationType::Voice,
                    "expand snippet".to_string(),
                    false,
                )],
            },
        )
        .expect("seed voice trigger");

        let (session, _) = create_test_session();
        let guard = crate::engine::quota_guard::QuotaGuard::global();
        let original_balance = guard.balance();
        let original_unlimited = guard.is_unlimited();
        let original_authenticated = guard.is_authenticated();

        guard.set_unlimited(false);
        guard.set_authenticated(true);
        // Set low balance: 0.005% is less than the 0.90% voice trigger cost
        // and less than the 2-word cost (0.008%).
        guard.set_balance(0.005);
        crate::platform::test_injector().clear();

        let res = session.inject_transcript("expand snippet").unwrap();

        let delta = guard.unflushed_delta();
        let balance = guard.balance();

        guard.set_balance(original_balance);
        guard.set_unlimited(original_unlimited);
        guard.set_authenticated(original_authenticated);

        assert_eq!(res, Some("Expanded result".to_string()));
        assert_eq!(balance, 0.0);
        // Must charge only the 0.90% voice trigger cost, without word double-charge (0.008%)
        assert!(
            (delta - 0.90).abs() < 1e-6,
            "expected delta 0.90, got {delta}"
        );
        assert!(
            crate::platform::test_injector()
                .recorded()
                .iter()
                .any(|c| c == "paste"),
            "soft-landing trigger must inject via paste, got {:?}",
            crate::platform::test_injector().recorded()
        );
    }

    #[test]
    fn test_voice_trigger_blocked_when_exhausted_quota_returns_none() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let conn = taurine_core::db::get_conn().expect("temp db conn");
        taurine_core::db::crud::create_entry(
            &conn,
            taurine_core::db::crud::NewEntry {
                description: None,
                content: "Expanded result".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                tags_json: "[]".to_string(),
                auto_case: false,
                interpreter: None,
                behavior: None,
                invocations: vec![(
                    taurine_core::db::crud::InvocationType::Voice,
                    "expand snippet".to_string(),
                    false,
                )],
            },
        )
        .expect("seed voice trigger");

        let (session, _) = create_test_session();
        let guard = crate::engine::quota_guard::QuotaGuard::global();
        let original_balance = guard.balance();
        let original_unlimited = guard.is_unlimited();
        let original_authenticated = guard.is_authenticated();

        guard.set_unlimited(false);
        guard.set_authenticated(true);
        guard.set_balance(0.0);
        crate::platform::test_injector().clear();

        let res = session.inject_transcript("expand snippet").unwrap();

        let delta = guard.unflushed_delta();

        guard.set_balance(original_balance);
        guard.set_unlimited(original_unlimited);
        guard.set_authenticated(original_authenticated);

        // Exhausted quota must return None, not a phantom fallback Some(...)
        assert_eq!(res, None);
        assert_eq!(delta, 0.0);
        assert!(
            crate::platform::test_injector().recorded().is_empty(),
            "exhausted voice trigger must not inject anything, got {:?}",
            crate::platform::test_injector().recorded()
        );
    }

    #[test]
    fn test_voice_dictation_soft_landing_when_low_quota() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, _) = create_test_session();

        let guard = crate::engine::quota_guard::QuotaGuard::global();
        let original_balance = guard.balance();
        let original_unlimited = guard.is_unlimited();
        let original_authenticated = guard.is_authenticated();

        guard.set_unlimited(false);
        guard.set_authenticated(true);
        // 0.005% balance is less than the 10-word cost (10 * 0.004% = 0.040%)
        guard.set_balance(0.005);
        crate::platform::test_injector().clear();

        let res = session
            .inject_transcript("one two three four five six seven eight nine ten")
            .unwrap();

        let delta = guard.unflushed_delta();
        let balance = guard.balance();

        guard.set_balance(original_balance);
        guard.set_unlimited(original_unlimited);
        guard.set_authenticated(original_authenticated);

        assert!(res.is_some());
        assert_eq!(balance, 0.0);
        assert!(
            (delta - 0.040).abs() < 1e-6,
            "expected delta 0.040, got {delta}"
        );
        assert!(
            crate::platform::test_injector()
                .recorded()
                .iter()
                .any(|c| c == "paste"),
            "soft-landing dictation must inject via paste, got {:?}",
            crate::platform::test_injector().recorded()
        );
    }

    #[test]
    fn test_ptt_latency_spans_recorded_in_order() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let session = create_latency_session();
        crate::platform::test_injector().clear();
        session.start_ptt().expect("ptt start");
        session.capture().buffer().push_samples(&[0.5; 3200]);
        let res = session.stop_ptt().expect("ptt stop");
        assert!(res.is_some(), "stub worker must return a transcript");
        let lat = session.last_latency().expect("latency spans recorded");
        assert!(lat.press_at <= lat.cue_at, "cue must fire at/after press");
        assert!(
            lat.cue_at <= lat.recording_at,
            "capture ready must follow cue"
        );
        assert!(
            lat.recording_at <= lat.transcript_at,
            "transcript must follow capture"
        );
    }
    #[test]
    fn test_ptt_start_always_opens_capture() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let opens = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&opens);
        let (session, _) = create_ordering_session(
            None,
            Some(Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })),
            None,
        );
        // No parked mic exists: stop closes the stream, so the press opens fresh.
        session.capture().start().expect("prior open");
        session.capture().stop();
        session.start_ptt().expect("press inside grace");
        assert_eq!(
            opens.load(Ordering::SeqCst),
            1,
            "every press opens the mic; nothing is parked"
        );
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        session.cancel();
    }

    #[test]
    fn test_ptt_start_after_stop_reopens_capture() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let opens = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&opens);
        let (session, _) = create_ordering_session(
            None,
            Some(Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })),
            None,
        );
        session.capture().start().expect("prior open");
        session.capture().stop();
        session.start_ptt().expect("press after stop");
        assert_eq!(
            opens.load(Ordering::SeqCst),
            1,
            "press after stop must reopen the mic"
        );
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        session.cancel();
    }

    #[test]
    fn test_sweep_recovers_disconnected_live_recording() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::input::hotkey::PTT_KEY_DOWN.store(true, Ordering::Relaxed);
        let (session, _) = create_test_session();
        session.start_ptt().expect("start");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        session.capture().simulate_disconnect_for_test();
        session.clean_expired_transcriber();
        assert_eq!(
            session.current_mode(),
            VoiceMode::PushToTalk,
            "recovery must keep recording, not abort it"
        );
        assert_eq!(
            session.capture().open_count_for_test(),
            2,
            "sweep must reopen the dead stream"
        );
        assert!(
            !session.capture().is_device_disconnected(),
            "recovery must clear the disconnect flag"
        );
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
    }

    #[test]
    fn test_sweep_reopens_stale_live_recording() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::input::hotkey::PTT_KEY_DOWN.store(true, Ordering::Relaxed);
        let (session, _) = create_test_session();
        session.capture().set_test_default_identity("mic-a");
        session.start_ptt().expect("start");
        session.capture().set_test_default_identity("mic-b");
        session.clean_expired_transcriber();
        assert_eq!(
            session.capture().open_count_for_test(),
            2,
            "OS default move mid-press must reopen, never record stale"
        );
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
    }

    #[test]
    fn test_sweep_skips_live_recovery_while_processing() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (session, _) = create_test_session();
        *session.mode.lock().unwrap() = VoiceMode::Processing;
        session.capture().simulate_disconnect_for_test();
        session.clean_expired_transcriber();
        assert_eq!(session.current_mode(), VoiceMode::Processing);
        assert_eq!(
            session.capture().open_count_for_test(),
            0,
            "sweep must not reopen the mic during transcription"
        );
        assert!(
            session.capture().is_device_disconnected(),
            "sweep must leave the flag for the next real recovery"
        );
    }

    #[test]
    fn test_sweep_autostops_ptt_with_lost_key_release() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::input::hotkey::PTT_KEY_DOWN.store(true, Ordering::Relaxed);
        let (session, _) = create_test_session();
        session.start_ptt().expect("start");
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        // The release the hook reinstall ate: flag cleared, stop never ran.
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        session.set_ptt_started_for_test(Instant::now() - Duration::from_secs(30));
        session.clean_expired_transcriber();
        assert_eq!(
            session.current_mode(),
            VoiceMode::Idle,
            "lost release must self-heal to Idle, not stick until restart"
        );
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
    }

    #[test]
    fn test_sweep_leaves_fresh_keyup_ptt_alone() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::input::hotkey::PTT_KEY_DOWN.store(true, Ordering::Relaxed);
        let (session, _) = create_test_session();
        session.start_ptt().expect("start");
        // Key already up (tap racing start) but inside grace: hands off.
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
        session.clean_expired_transcriber();
        assert_eq!(
            session.current_mode(),
            VoiceMode::PushToTalk,
            "grace must protect fresh sessions from the failsafe"
        );
        session.cancel();
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);
    }

    #[test]
    fn test_sweep_resets_stranded_processing() {
        let (session, _) = create_test_session();
        *session.mode.lock().unwrap() = VoiceMode::Processing;
        session.set_processing_since_for_test(Instant::now() - Duration::from_secs(120));
        session.clean_expired_transcriber();
        assert_eq!(
            session.current_mode(),
            VoiceMode::Idle,
            "Processing older than any transcription must reset"
        );
    }

    #[test]
    fn test_sweep_leaves_fresh_processing_alone() {
        let (session, _) = create_test_session();
        *session.mode.lock().unwrap() = VoiceMode::Processing;
        session.set_processing_since_for_test(Instant::now());
        session.clean_expired_transcriber();
        assert_eq!(
            session.current_mode(),
            VoiceMode::Processing,
            "a live transcription must never be reset"
        );
        *session.mode.lock().unwrap() = VoiceMode::Idle;
    }

    #[test]
    fn test_sweep_leaves_handsfree_alone() {
        let (session, _) = create_test_session();
        *session.mode.lock().unwrap() = VoiceMode::HandsFree;
        session.clean_expired_transcriber();
        assert_eq!(
            session.current_mode(),
            VoiceMode::HandsFree,
            "toggle-based hands-free is exempt from the key-up net"
        );
        *session.mode.lock().unwrap() = VoiceMode::Idle;
    }

    #[test]
    fn test_cloud_voice_transcribe_and_injection_e2e() {
        use std::io::BufReader;

        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut header_bytes = Vec::new();
            let mut byte = [0u8; 1];
            while reader.read_exact(&mut byte).is_ok() {
                header_bytes.push(byte[0]);
                if header_bytes.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let headers = String::from_utf8_lossy(&header_bytes).to_string();
            assert!(headers.starts_with("POST /transcribe HTTP/1.1"));
            assert!(
                headers
                    .to_lowercase()
                    .contains("authorization: bearer mock-test-key")
            );

            let mut content_length = 0usize;
            for line in headers.lines() {
                if let Some(val) = line.to_lowercase().strip_prefix("content-length:") {
                    content_length = val.trim().parse().unwrap_or(0);
                }
            }

            if content_length > 0 {
                let mut body = vec![0u8; content_length];
                let _ = reader.read_exact(&mut body);
            }

            let response_body = r#"{"text": "Cloud voice dictation verified"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let cloud_client = Arc::new(taurine_core::voice::GroqVoiceClient::with_endpoint(
            Some("mock-test-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        ));

        let (session, _) = create_test_session();
        let session = session.with_cloud_client(cloud_client);

        crate::platform::test_injector().clear();

        let guard = crate::engine::quota_guard::QuotaGuard::global();
        let original_balance = guard.balance();
        let original_authenticated = guard.is_authenticated();
        let original_unlimited = guard.is_unlimited();

        guard.set_authenticated(true);
        guard.set_unlimited(false);
        guard.set_balance(100.0);

        session.start_ptt().expect("ptt start");
        session.capture().buffer().push_samples(&[0.5; 3200]);
        let res = session.stop_ptt().expect("ptt stop");

        assert_eq!(res, Some("Cloud voice dictation verified".to_string()));

        // 1. Verify injection occurred via fake clipboard
        let recorded = crate::platform::test_injector().recorded();
        assert!(
            !recorded.is_empty(),
            "Recorded injection events must not be empty"
        );

        // 2. Verify quota was deducted for 4 words (4 * 0.004 = 0.016%)
        let post_quota = guard.balance();
        assert!(
            post_quota < 100.0,
            "Quota must be depleted for spoken words"
        );
        assert!((guard.unflushed_delta() - 0.016).abs() < 1e-4);

        // 3. Verify telemetry and activity_events history in SQLite
        let conn = taurine_core::db::get_conn().unwrap();
        let events = taurine_core::db::crud::get_recent_activity_events(
            &conn,
            10,
            0,
            Some(taurine_core::db::crud::ActivityCategory::Voice),
        )
        .unwrap();
        assert!(
            !events.is_empty(),
            "Dictation history must be recorded in activity_events"
        );
        assert_eq!(
            events[0].content.as_deref(),
            Some("Cloud voice dictation verified")
        );

        // Restore quota guard state
        guard.set_balance(original_balance);
        guard.set_unlimited(original_unlimited);
        guard.set_authenticated(original_authenticated);

        server_thread.join().unwrap();
    }

    #[test]
    fn test_live_spooler_deleted_on_clean_stop() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, _) = create_ordering_session(None, Some(Arc::new(move || Ok(()))), None);

        session.start_ptt().expect("start");
        // Spooler attached and file created
        let rec_dir = crate::voice::recovery::recovery_dir().unwrap();
        let files_during = taurine_core::voice::list_orphaned_sessions(&rec_dir).unwrap();
        assert_eq!(
            files_during.len(),
            1,
            "Spooler journal must exist while recording"
        );

        session.capture().buffer().push_samples(&[0.5; 3200]);
        let _ = session.stop_ptt().expect("stop");

        // After clean stop, spooler is discarded and file is deleted
        let files_after = taurine_core::voice::list_orphaned_sessions(&rec_dir).unwrap();
        assert!(
            files_after.is_empty(),
            "Spooler journal must be discarded on clean stop"
        );
    }

    #[test]
    fn test_orphaned_audio_session_recovered_on_startup() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = std::thread::spawn(move || {
            use std::io::{BufRead, Read, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(&mut stream);
            let mut line = String::new();
            let mut content_length = 0;
            while let Ok(n) = reader.read_line(&mut line) {
                if n == 0 || line == "\r\n" {
                    break;
                }
                if line.to_lowercase().starts_with("content-length:")
                    && let Some(val) = line.split(':').nth(1)
                {
                    content_length = val.trim().parse::<usize>().unwrap_or(0);
                }
                line.clear();
            }
            if content_length > 0 {
                let mut body = vec![0u8; content_length];
                let _ = reader.read_exact(&mut body);
            }

            let response_body = r#"{"text": "Rescued speech from crash"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
            let _ = stream.flush();
        });

        let cloud_client = Arc::new(taurine_core::voice::GroqVoiceClient::with_endpoint(
            Some("mock-test-key".to_string()),
            format!("http://127.0.0.1:{port}/transcribe"),
            "whisper-large-v3".to_string(),
        ));

        let (session, _) = create_test_session();
        let session = session.with_cloud_client(cloud_client);

        crate::platform::test_injector().clear();

        // Create an orphaned recovery file on disk
        let rec_dir = crate::voice::recovery::recovery_dir().unwrap();
        let mut spooler =
            taurine_core::voice::AudioSpooler::new(&rec_dir, "crashed_session_42").unwrap();
        spooler.append_samples(&[0.5; 3200]).unwrap();
        let journal_path = spooler.finish().unwrap();
        assert!(journal_path.exists());

        // Call startup recovery
        let recovered = session.recover_orphaned_sessions().unwrap();
        assert_eq!(recovered, vec!["Rescued speech from crash".to_string()]);

        // Verify the file was cleaned up
        assert!(
            !journal_path.exists(),
            "Orphaned journal must be deleted after recovery"
        );

        // Verify clipboard got the rescued text
        let clip_text = crate::platform::read_clipboard_text().unwrap();
        assert_eq!(clip_text, "Rescued speech from crash");

        // Verify activity events contains the recovered text
        let conn = taurine_core::db::get_conn().unwrap();
        let events = taurine_core::db::crud::get_recent_activity_events(
            &conn,
            10,
            0,
            Some(taurine_core::db::crud::ActivityCategory::Voice),
        )
        .unwrap();
        assert!(!events.is_empty());
        assert_eq!(
            events[0].content.as_deref(),
            Some("Rescued speech from crash")
        );

        server_thread.join().unwrap();
    }
}
