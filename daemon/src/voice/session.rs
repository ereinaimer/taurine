use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use taurine_core::voice::{
    VoiceDictionary, format_mb, format_transcript, model_disk_size_mb, process_rss_mb,
};

use super::capture::AudioCapture;
use super::modes::VoiceMode;
use super::worker_client::WorkerClient;

/// Minimal energy threshold to consider audio frames as containing speech.
pub(crate) const MIN_SPEECH_RMS_ENERGY: f32 = 1e-7;

/// Minimum audio samples required for transcription (100ms at 16kHz = 1600 samples).
pub(crate) const MIN_SAMPLES_COUNT: usize = 1600;

/// Mash window after a session end: presses inside it cue instantly but defer
/// engine work to a still-held rescue instead of churning the worker and mic.
pub(crate) const MASH_WINDOW_MS: u64 = 300;
/// Still-held rescue delay: a gated press starts only if the key is still
/// down after this wait; a released key was a bounce and records nothing.
pub(crate) const RESCUE_WAIT_MS: u64 = 200;
/// Pre-roll cushion (ms): prior mic audio restored at the front of each
/// recording so the first syllable is never clipped.
pub(crate) const PREROLL_MS: u64 = 400;
/// Pre-roll samples at 16 kHz mono.
pub(crate) const PREROLL_SAMPLES: usize = PREROLL_MS as usize * 16;

/// Hold ladder rungs mirroring the worker sweep: a fresh load holds 10s;
/// genuine use inside the window steps one rung up to the 60s cap, so at
/// most 60s of residency follows last use, then zero. Both tables must agree.
const HOLD_RUNGS_SECS: [u64; 4] = [10, 20, 30, 60];

fn base_hold() -> Duration {
    Duration::from_secs(HOLD_RUNGS_SECS[0])
}

fn next_rung(hold: Duration) -> Duration {
    match HOLD_RUNGS_SECS.iter().position(|r| *r == hold.as_secs()) {
        Some(i) => Duration::from_secs(HOLD_RUNGS_SECS[(i + 1).min(HOLD_RUNGS_SECS.len() - 1)]),
        None => base_hold(),
    }
}

/// Chunk cadence for live streaming while recording.
const STREAM_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// TTL bookkeeping for the worker-side model. The audio and the recognizer
/// live in the `--voice-daemon` worker; this stays lean by design.
struct EngineMeta {
    model_id: String,
    last_used: Instant,
    hold: Duration,
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

/// One live streaming request: shared progress plus a joinable thread.
///
/// `target` is snapshotted at request start so a settings change mid-record
/// can neither respawn the worker nor wipe its buffers under a live session.
struct StreamEntry {
    progress: Arc<StreamProgress>,
    handle: Option<std::thread::JoinHandle<()>>,
    target: String,
    timing: PttTiming,
}

/// Test-only mic-open cue stub (records call order instead of playing audio).
#[cfg(test)]
type CueHook = Arc<dyn Fn() + Send + Sync>;
/// Test-only capture-start stub (records call order, injects device errors).
#[cfg(test)]
type CaptureHook = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

/// Manages the lifecycle of voice recording, dictation sessions, and text injection.
///
/// The single configured model lives in the isolated `--voice-daemon`
/// worker: it loads on demand when a Push-to-Talk or Hands-Free session
/// starts, stays pinned for the whole recording, and is fully evicted once
/// its hold rung (10s base, up to 60s with use) expires after last use.
/// The microphone stream is parked for a short grace after each session,
/// then closed on silence.
pub struct VoiceSessionManager {
    mode: Mutex<VoiceMode>,
    capture: Arc<AudioCapture>,
    paused: Arc<AtomicBool>,
    model_name: Mutex<String>,
    dictionary: Mutex<VoiceDictionary>,
    worker: WorkerClient,
    meta: Arc<Mutex<Option<EngineMeta>>>,
    active_req: Mutex<Option<String>>,
    streams: Arc<Mutex<HashMap<String, StreamEntry>>>,
    req_counter: AtomicU64,
    /// Press remembered while busy: a press landing during decode cannot
    /// start, so it is parked here and honored the moment the session
    /// returns to Idle with the key still held. Never auto-records from a
    /// released key; Escape and cancel always clear it.
    pending_press: AtomicBool,
    /// Weak self-handle for the mash-gate rescue waiter. Set once the
    /// manager lives in an `Arc` (production `start()`, tests via
    /// `into_test_arc`); the waiter upgrades it after its delay and
    /// no-ops if the manager is already gone, so it never extends a lifetime.
    self_ref: OnceLock<Weak<VoiceSessionManager>>,
    /// End of the last dictation session, for the mash gate. Stamped by every
    /// path that terminates a session; presses inside `MASH_WINDOW_MS` cue
    /// instantly but defer engine work to the still-held rescue waiter.
    last_stop: Mutex<Option<Instant>>,
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
            meta: Arc::new(Mutex::new(None)),
            active_req: Mutex::new(None),
            streams: Arc::new(Mutex::new(HashMap::new())),
            req_counter: AtomicU64::new(0),
            pending_press: AtomicBool::new(false),
            self_ref: OnceLock::new(),
            last_stop: Mutex::new(None),
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

    /// Builder method to inject a worker client (tests only, no spawn).
    #[cfg(test)]
    pub(crate) fn with_worker(mut self, worker: WorkerClient) -> Self {
        self.worker = worker;
        self
    }

    /// Builder method to stub the mic-open cue (tests only, records order).
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

    /// Fire the mic-close cue (stubbed in tests to observe ordering).
    #[cfg(test)]
    fn fire_stop_cue(&self) {
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
    #[cfg(not(test))]
    fn fire_stop_cue(&self) {
        crate::services::audio::play_voice_stop_cue();
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

    /// Unloads the voice model from RAM once its hold rung expires.
    ///
    /// Only evicts while idle: an active recording or transcription pins the
    /// model so long dictations never pay a mid-session reload. Guarantees
    /// 0 MB idle voice memory once the rung deadline passes after last use.
    /// Also reaps a silence-expired held mic every tick, and closes any held
    /// mic on model eviction, so the device is never held longer than the
    /// model is warm.
    pub fn clean_expired_transcriber(&self) {
        self.capture.reclaim_expired_held();
        if self.is_active() {
            return;
        }
        let mut guard = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ref meta) = *guard
            && meta.last_used.elapsed() > meta.hold
        {
            let hold_secs = meta.hold.as_secs();
            let rss_before = process_rss_mb();
            *guard = None;
            self.worker.unload_model();
            self.capture.close_held_now();
            let rss_after = process_rss_mb();
            info!(
                "VoiceSessionManager: model evicted ({hold_secs}s hold expired, RSS {} -> {} MB)",
                format_mb(rss_before),
                format_mb(rss_after),
            );
        }
    }

    /// Refresh a fresh model timestamp so a session starting on a stale-but
    /// resident model does not expire mid-recording. Start never steps the
    /// rung: one completed dictation steps exactly once, in finalize_request,
    /// matching the worker transcribe-success step.
    fn touch_loaded_slot(&self) {
        let mut guard = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ref mut meta) = *guard
            && meta.last_used.elapsed() <= meta.hold
        {
            meta.last_used = Instant::now();
        }
    }

    /// Streaming pin for the daemon mirror: each successful chunk forward
    /// refreshes last_used without stepping the rung, matching the worker
    /// OP_APPEND pin so a long recording agrees at finalize instead of the
    /// daemon resetting to base while the worker steps up.
    fn pin_stream_meta(meta: &Mutex<Option<EngineMeta>>, target: &str) {
        if let Some(ref mut entry) = *meta.lock().unwrap_or_else(|p| p.into_inner())
            && entry.model_id == target
        {
            entry.last_used = Instant::now();
        }
    }

    /// Record one completed dictation as genuine use: step up inside the
    /// window, restart at base past the deadline or on a model switch.
    fn record_use(&self, target: String) {
        let mut meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        let hold = match &*meta {
            Some(prev) if prev.model_id == target && prev.last_used.elapsed() <= prev.hold => {
                let next = next_rung(prev.hold);
                if next != prev.hold {
                    debug!(
                        "VoiceSessionManager: extending voice model hold to {}s",
                        next.as_secs()
                    );
                }
                next
            }
            _ => {
                let rss = process_rss_mb();
                let file_mb = model_disk_size_mb(&target, None);
                info!(
                    "VoiceSessionManager: model '{target}' resident ({} MB on disk, RSS {} -> {} MB)",
                    format_mb(file_mb),
                    format_mb(rss),
                    format_mb(rss),
                );
                base_hold()
            }
        };
        *meta = Some(EngineMeta {
            model_id: target,
            last_used: Instant::now(),
            hold,
        });
    }

    /// Stamp the end of a dictation session for the mash gate.
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
        std::thread::Builder::new()
            .name("tau-ptt-rescue".to_string())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(RESCUE_WAIT_MS));
                let held = if handsfree {
                    crate::input::hotkey::HANDSFREE_KEY_DOWN.load(Ordering::Relaxed)
                } else {
                    crate::input::hotkey::PTT_KEY_DOWN.load(Ordering::Relaxed)
                };
                if !held || me.is_paused() || me.current_mode() != VoiceMode::Idle {
                    return;
                }
                if handsfree {
                    if let Err(e) = me.start_handsfree_now() {
                        warn!("VoiceSessionManager: mash rescue hands-free start failed: {e}");
                    }
                } else if let Err(e) = me.start_ptt_now(Instant::now(), Instant::now()) {
                    warn!("VoiceSessionManager: mash rescue PTT start failed: {e}");
                }
            })
            .ok();
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

    /// Shut the worker process down entirely (daemon shutdown path).
    pub fn shutdown_worker(&self) {
        self.worker.shutdown();
        let mut meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        *meta = None;
        debug!("VoiceSessionManager: voice worker shut down");
    }

    /// Pre-spawn the worker process without loading any model.
    ///
    /// Silent: failures fall back to the lazy first-press path.
    pub fn warm_worker(&self) {
        self.worker.warm_up();
    }

    /// Immediately unloads the speech recognition model from RAM.
    pub fn unload_model(&self) {
        let mut meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        let hold_secs = meta.as_ref().map(|m| m.hold.as_secs()).unwrap_or(0);
        let rss_before = process_rss_mb();
        *meta = None;
        self.worker.unload_model();
        let rss_after = process_rss_mb();
        info!(
            "VoiceSessionManager: model evicted ({hold_secs}s hold expired, RSS {} -> {} MB)",
            format_mb(rss_before),
            format_mb(rss_after),
        );
    }

    fn ensure_model_loaded(
        worker: &WorkerClient,
        meta: &Arc<Mutex<Option<EngineMeta>>>,
        target: &str,
    ) -> Result<(), String> {
        let fresh = {
            let guard = meta.lock().unwrap_or_else(|p| p.into_inner());
            match &*guard {
                Some(meta) => meta.model_id == target && meta.last_used.elapsed() <= meta.hold,
                None => false,
            }
        };
        if fresh {
            return Ok(());
        }
        debug!("taurine-voice-loader: loading voice model '{target}'");
        let rss_before = process_rss_mb();
        worker.ensure_worker(target)?;
        let rss_after = process_rss_mb();
        let file_mb = model_disk_size_mb(target, None);
        info!(
            "VoiceSessionManager: model '{target}' resident ({} MB on disk, RSS {} -> {} MB)",
            format_mb(file_mb),
            format_mb(rss_before),
            format_mb(rss_after),
        );
        let mut guard = meta.lock().unwrap_or_else(|p| p.into_inner());
        *guard = Some(EngineMeta {
            model_id: target.to_string(),
            last_used: Instant::now(),
            hold: base_hold(),
        });
        Ok(())
    }

    /// Ensure the worker serves the single configured model.
    ///
    /// Only the resolved `voice_model` is ever loaded. A fresh model whose
    /// `last_used` is inside its hold rung is reused; the client singleton keeps
    /// concurrent triggers on one worker. A miss loads at the base rung.
    pub fn ensure_worker_ready(&self) -> Result<(), String> {
        Self::ensure_model_loaded(&self.worker, &self.meta, self.target_model())
    }

    /// Preload the model in the background on a dedicated thread at genuine engine start.
    ///
    /// Fire-and-forget: press and capture never wait for the load. If the worker is already
    /// warm this is a millisecond ping; if cold, background load overlaps speech capture.
    fn kick_preload(&self) {
        let worker = self.worker.clone();
        let meta = Arc::clone(&self.meta);
        let target = self.target_model().to_string();
        let builder = std::thread::Builder::new().name("tau-voice-preload".to_string());
        if let Err(e) = builder.spawn(move || {
            if let Err(e) = Self::ensure_model_loaded(&worker, &meta, &target) {
                debug!("VoiceSessionManager: preload ensure_worker failed: {e}");
            }
        }) {
            debug!("VoiceSessionManager: failed to spawn preload thread: {e}");
        }
    }

    /// Stream newly captured audio to the worker while the request is alive.
    ///
    /// Runs on its own thread; the stop path stops it, joins it, sends the
    /// tail, and transcribes. Joining before reading progress guarantees the
    /// tail is sent exactly once (never duplicated, never dropped).
    fn spawn_chunk_forwarder(&self, req_id: String, target: String, timing: PttTiming) {
        let progress = Arc::new(StreamProgress::new());
        let buffer = self.capture.buffer().clone();
        let worker = self.worker.clone();
        let thread_progress = Arc::clone(&progress);
        let thread_req = req_id.clone();
        let thread_meta = Arc::clone(&self.meta);
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
                    Self::pin_stream_meta(&thread_meta, &target);
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
                            timing,
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
    /// The cue fires on entry for every genuine press, before locks and
    /// state checks: it acknowledges the hotkey, never the recording.
    /// Silent only when paused. Clears any previous buffer, starts
    /// microphone capture, and transitions to `PushToTalk`.
    pub fn start_ptt(&self) -> Result<(), String> {
        let press_at = Instant::now();
        if self.is_paused() {
            debug!("VoiceSessionManager: start_ptt ignored because daemon is paused");
            return Ok(());
        }

        self.fire_start_cue();
        let cue_at = Instant::now();

        {
            let mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::Idle {
                // Busy (recording or decoding): park the press instead of
                // dropping it. The stop path honors it when the key is held.
                self.pending_press.store(true, Ordering::Relaxed);
                return Ok(());
            }
        }
        if self.in_mash_window() {
            // Bounce: acknowledged by the cue above, engine work deferred to
            // the rescue waiter so flutter never churns the worker and mic.
            self.spawn_mash_rescue(false);
            return Ok(());
        }
        self.start_ptt_now(press_at, cue_at)
    }

    /// Immediate engine start: snapshot the mic pre-roll, open or reuse the
    /// device, and begin recording. Callers must have passed the mash gate
    /// (or hold an explicit continuation like the pending-press autostart).
    fn start_ptt_now(&self, press_at: Instant, cue_at: Instant) -> Result<(), String> {
        {
            let mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::Idle {
                // Lost a race with another starter: park instead of doubling.
                self.pending_press.store(true, Ordering::Relaxed);
                return Ok(());
            }
        }
        self.pending_press.store(false, Ordering::Relaxed);
        // Snapshot the live pre-roll BEFORE clearing: a parked mic keeps
        // pushing while held, so the tail is the audio just before the press.
        // A cold mic has nothing (the last stop drained it) and the prepend
        // below is a no-op.
        let preroll = self.capture.buffer().peek_tail(PREROLL_SAMPLES);
        self.capture.buffer().clear();
        if !self.capture.try_reuse_held_stream()
            && let Err(e) = self.start_capture()
        {
            warn!("Failed to start audio capture stream for PTT: {e}");
            return Err(e);
        }
        if !preroll.is_empty() {
            self.capture.buffer().prepend_samples(&preroll);
        }
        let recording_at = Instant::now();

        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::Idle {
                self.pending_press.store(true, Ordering::Relaxed);
                return Ok(());
            }
            *mode = VoiceMode::PushToTalk;
        }
        self.touch_loaded_slot();
        self.kick_preload();
        info!("VoiceSessionManager: Push-To-Talk recording started");
        self.begin_request(PttTiming {
            press_at,
            cue_at,
            recording_at,
        });
        Ok(())
    }

    /// Register a new streaming request and start its chunk forwarder.
    fn begin_request(&self, timing: PttTiming) {
        let req_id = format!("r{}", self.req_counter.fetch_add(1, Ordering::Relaxed));
        *self.active_req.lock().unwrap_or_else(|p| p.into_inner()) = Some(req_id.clone());
        self.spawn_chunk_forwarder(req_id, self.target_model().to_string(), timing);
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
                let _tapped = self.capture.buffer().drain();
                #[cfg(test)]
                {
                    *self.last_drained.lock().unwrap_or_else(|p| p.into_inner()) =
                        Some(_tapped.clone());
                }
                debug!("VoiceSessionManager: tap too short; skipping worker entirely");
                return Ok(None);
            }
            entry.progress.alive.store(false, Ordering::Relaxed);
            if let Some(handle) = entry.handle.take() {
                let _ = handle.join();
            }
            let drained = self.capture.buffer().drain();
            #[cfg(test)]
            {
                *self.last_drained.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(drained.clone());
            }
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
            let timing = entry.timing;
            self.worker.ensure_worker(&target)?;
            if sent < drained.len() {
                let seq = entry.progress.seq.load(Ordering::Relaxed);
                self.worker.append(&req_id, seq, &drained[sent..])?;
            }
            let hotwords = {
                let dict = self.dictionary.lock().unwrap_or_else(|p| p.into_inner());
                let active_app =
                    crate::platform::get_active_window_info().and_then(|info| info.exec_name);
                let triggers = if let Ok(conn) = taurine_core::db::get_conn()
                    && let Ok(invocations) =
                        taurine_core::db::crud::list_active_voice_invocations(&conn)
                {
                    invocations
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
                        .map(|inv| inv.invocation)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                taurine_core::voice::build_hotwords_payload(&dict, &triggers)
            };
            // Long hands-free audio needs headroom past old load budgets;
            // the worker already popped the buffer, so a timeout loses audio.
            let transcript =
                self.worker
                    .transcribe(&req_id, Duration::from_secs(60), hotwords.as_deref())?;
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
                *self.last_latency.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(RequestLatency {
                        press_at: timing.press_at,
                        cue_at: timing.cue_at,
                        recording_at: timing.recording_at,
                        transcript_at,
                    });
            }
            self.record_use(target);
            self.inject_transcript(&transcript.text)
        } else {
            Ok(None)
        }
    }

    /// Stop Push-To-Talk recording, transcribe captured speech, inject text, and return to `Idle`.
    ///
    /// Fires the mic-close cue first to acknowledge the key release itself,
    /// before locks and state checks, then stops the mic and transcribes:
    /// feedback lines up with the hotkey, never with paste. The microphone
    /// stays parked for the grace window.
    pub fn stop_ptt(&self) -> Result<Option<String>, String> {
        self.fire_stop_cue();
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::PushToTalk {
                return Ok(None);
            }
            *mode = VoiceMode::Processing;
        }

        self.stop_capture();
        info!("VoiceSessionManager: Push-To-Talk recording stopped; processing audio");
        let result = self.finalize_request();
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            *mode = VoiceMode::Idle;
        }
        self.note_session_end();
        self.maybe_autostart();
        result
    }

    /// Immediate hands-free start: snapshot the mic pre-roll, open the
    /// device, and transition to `HandsFree`. Mash-blind like `start_ptt_now`;
    /// the gated `toggle_handsfree` entry decides when to call it.
    fn start_handsfree_now(&self) -> Result<(VoiceMode, Option<String>), String> {
        let press_at = Instant::now();
        {
            let mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            if *mode != VoiceMode::Idle {
                self.pending_press.store(true, Ordering::Relaxed);
                return Ok((*mode, None));
            }
        }
        let preroll = self.capture.buffer().peek_tail(PREROLL_SAMPLES);
        self.capture.buffer().clear();
        // Already cued at entry; timestamp it here for the telemetry.
        let cue_at = Instant::now();
        if let Err(e) = self.start_capture() {
            warn!("Failed to start audio capture stream for Hands-Free: {e}");
            return Err(e);
        }
        if !preroll.is_empty() {
            self.capture.buffer().prepend_samples(&preroll);
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
        self.touch_loaded_slot();
        self.kick_preload();
        info!("VoiceSessionManager: Hands-Free dictation activated");
        self.begin_request(PttTiming {
            press_at,
            cue_at,
            recording_at,
        });
        Ok((VoiceMode::HandsFree, None))
    }

    /// Toggle Hands-Free continuous dictation mode.
    ///
    /// The cue fires on entry by direction, before locks and state checks:
    /// it acknowledges the toggle, never the transcription. Silent when
    /// paused, and silent when there is nothing to toggle.
    /// If `Idle`, starts capture and transitions to `HandsFree`.
    /// If `HandsFree`, stops capture, processes audio, injects text, and transitions to `Idle`.
    /// The microphone stays parked for the grace window while Hands-Free is off.
    pub fn toggle_handsfree(&self) -> Result<(VoiceMode, Option<String>), String> {
        if self.is_paused() {
            debug!("VoiceSessionManager: toggle_handsfree ignored because daemon is paused");
            return Ok((VoiceMode::Idle, None));
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
                self.stop_capture();
                info!("VoiceSessionManager: Hands-Free dictation toggled off; processing audio");
                let result = self.finalize_request()?;
                {
                    let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
                    *mode = VoiceMode::Idle;
                }
                self.note_session_end();
                self.maybe_autostart();
                Ok((VoiceMode::Idle, result))
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

        // Escape aborts everything: a parked press dies with the session.
        self.pending_press.store(false, Ordering::Relaxed);
        self.fire_stop_cue();
        self.stop_capture();
        info!("VoiceSessionManager: Escape pressed; terminating voice session and transcribing");
        let result = self.finalize_request();
        {
            let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
            *mode = VoiceMode::Idle;
        }
        self.note_session_end();
        result
    }

    /// Cancel current voice recording immediately without transcribing or injecting text.
    ///
    /// Frees the worker-side buffer in the background so a cancelled
    /// recording never leaks audio state into the next session.
    pub fn cancel(&self) {
        // Acknowledge the cancel first: the forwarder join below can block,
        // and feedback belongs to the keypress, never to teardown.
        // A cancel voids any parked press along with the live session.
        self.pending_press.store(false, Ordering::Relaxed);
        self.fire_stop_cue();
        let mut mode = self.mode.lock().unwrap_or_else(|p| p.into_inner());
        // Only a live session arms the mash gate; cancelling idle noise must
        // not defer the user's next genuine press.
        let was_live = *mode != VoiceMode::Idle
            || self
                .active_req
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_some();
        *mode = VoiceMode::Idle;
        drop(mode);
        if was_live {
            self.note_session_end();
        }
        self.stop_capture();
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
                    let _ = worker.transcribe(&req_id, Duration::from_secs(5), None);
                })
                .ok();
        }
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
                taurine_core::voice::rank_voice_invocations(&normalized_spoken, &in_scope)
            {
                let inv = matched.trigger;
                info!(
                    "VoiceSessionManager: matched voice trigger '{}' (score {:.2}); expanding output",
                    inv.invocation, matched.score
                );

                let fallback = inv.action.output.clone();
                let output = crate::voice::fire_voice_trigger_with_args(
                    inv,
                    &matched.args,
                    &normalized_spoken,
                    &conn,
                    active_app.clone(),
                    taurine_core::settings::SpinnerStyle::default(),
                )
                .unwrap_or(fallback);
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
        crate::injector::inject_expansion_for_voice(
            vec![taurine_core::engine::variables::ExpansionStep::Text(
                final_text.to_string(),
            )],
            taurine_core::settings::SpinnerStyle::default(),
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
            .clone()
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
    pub(crate) fn set_test_meta(&self, model_id: &str, last_used: Instant) {
        *self.meta.lock().unwrap() = Some(EngineMeta {
            model_id: model_id.to_string(),
            last_used,
            hold: base_hold(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::worker_client::{ConnectFn, DynStream, SpawnFn, transact};
    use super::super::worker_protocol as proto;
    use super::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::process::Command;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicUsize;
    use taurine_core::voice::VoiceDictionary;

    fn create_test_session() -> (VoiceSessionManager, Arc<AtomicBool>) {
        create_ordering_session(None, None, None)
    }

    fn create_ordering_session(
        cue_hook: Option<CueHook>,
        capture_hook: Option<CaptureHook>,
        stop_hook: Option<CueHook>,
    ) -> (VoiceSessionManager, Arc<AtomicBool>) {
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
        let mut session = VoiceSessionManager::new(capture, paused.clone()).with_worker(worker);
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
    fn test_mash_rescue_starts_when_key_held_with_preroll() {
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
        crate::input::hotkey::PTT_KEY_DOWN.store(true, Ordering::Relaxed);
        session.start_ptt().expect("mash press");
        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert!(session.active_request().is_none());
        // The parked mic keeps pushing while held: pre-press audio arrives
        // before the rescue wait elapses.
        const MARKER: f32 = 0.7;
        session
            .capture()
            .buffer()
            .push_samples(&[MARKER; PREROLL_SAMPLES]);
        std::thread::sleep(Duration::from_millis(RESCUE_WAIT_MS + 100));
        assert_eq!(
            session.current_mode(),
            VoiceMode::PushToTalk,
            "held key must rescue the gated press"
        );
        session.capture().buffer().push_samples(&[0.5; 3200]);
        let res = session.stop_ptt().expect("stop rescued");
        assert!(res.is_some(), "rescued recording must transcribe");
        let drained = session.last_drained().expect("drained audio recorded");
        assert!(drained.len() >= PREROLL_SAMPLES + 3200);
        assert!(
            drained[..PREROLL_SAMPLES].iter().all(|s| *s == MARKER),
            "drained audio must begin with the pre-roll tail"
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
        session
            .capture()
            .buffer()
            .push_samples(&[0.002; PREROLL_SAMPLES]);
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
        session
            .capture()
            .buffer()
            .push_samples(&[0.0; PREROLL_SAMPLES + 3200]);
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
        crate::input::hotkey::HANDSFREE_KEY_DOWN.store(true, Ordering::Relaxed);
        let (mode, _) = session.toggle_handsfree().expect("mash toggle");
        assert_eq!(mode, VoiceMode::Idle);
        std::thread::sleep(Duration::from_millis(RESCUE_WAIT_MS + 100));
        assert_eq!(
            session.current_mode(),
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
                name: String::new(),
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
    fn test_meta_inactivity_eviction() {
        let (session, _) = create_test_session();
        // Past the 15s base rung, the stale entry evicts for 0 MB idle RAM.
        session.set_test_meta(
            "parakeet-tdt-ctc-110m",
            Instant::now() - Duration::from_secs(16),
        );
        assert!(session.meta.lock().unwrap().is_some());

        // Inactivity past the rung unloads the single voice model.
        session.clean_expired_transcriber();
        assert!(session.meta.lock().unwrap().is_none());

        // Fresh models within the rung are retained.
        session.set_test_meta("parakeet-tdt-ctc-110m", Instant::now());
        session.clean_expired_transcriber();
        assert!(session.meta.lock().unwrap().is_some());

        // Explicit unload drops the model immediately.
        session.unload_model();
        assert!(session.meta.lock().unwrap().is_none());
    }

    #[test]
    fn test_base_rung_holds_5s_evicts_15s() {
        let (session, _) = create_test_session();
        session.set_test_meta(
            "parakeet-tdt-ctc-110m",
            Instant::now() - Duration::from_secs(5),
        );
        session.clean_expired_transcriber();
        assert!(
            session.meta.lock().unwrap().is_some(),
            "5s past activity is warm on the base rung"
        );

        session.set_test_meta(
            "parakeet-tdt-ctc-110m",
            Instant::now() - Duration::from_secs(15),
        );
        session.clean_expired_transcriber();
        assert!(
            session.meta.lock().unwrap().is_none(),
            "15s past activity passes the base rung"
        );
    }

    #[test]
    fn test_active_session_never_evicts() {
        let (session, _) = create_test_session();
        session.set_test_meta(
            "parakeet-tdt-ctc-110m",
            Instant::now() - Duration::from_secs(11),
        );

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

    /// Duplex stub peer answering hello/ping/append/transcribe without a process.
    async fn latency_stub_peer(mut peer: tokio::io::DuplexStream, text: String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = Vec::new();
        loop {
            let mut chunk = [0u8; 4096];
            let n = match peer.read(&mut chunk).await {
                Ok(n) => n,
                Err(_) => return,
            };
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
            while let Some((header, _)) = proto::decode_frame(&mut buf).unwrap_or(None) {
                let mut resp = match header.op.as_str() {
                    proto::OP_HELLO => {
                        let mut r = proto::Header::op(proto::OP_READY);
                        r.version = header.version.clone();
                        r.model = header.model.clone();
                        r
                    }
                    proto::OP_PING => proto::Header::op(proto::OP_PONG),
                    proto::OP_APPEND => {
                        let mut r = proto::Header::op(proto::OP_ACK);
                        r.seq = header.seq;
                        r
                    }
                    proto::OP_TRANSCRIBE => {
                        let mut r = proto::Header::op(proto::OP_RESULT);
                        r.text = Some(text.clone());
                        r.confidence = Some(0.9);
                        r.duration_secs = Some(1.0);
                        r
                    }
                    _ => proto::Header::op(proto::OP_ACK),
                };
                resp.req_id = header.req_id.clone();
                let bytes = proto::encode_frame(&resp, &[]).expect("encode");
                if peer.write_all(&bytes).await.is_err() {
                    return;
                }
            }
        }
    }

    /// Session with a working stub worker: short-lived spawner child plus an
    /// in-process duplex peer, so begin/finalize transcribe hermetically.
    fn create_latency_session() -> VoiceSessionManager {
        let buffer = Arc::new(super::super::capture::AudioFrameBuffer::new());
        let capture = Arc::new(AudioCapture::new(buffer));
        let paused = Arc::new(AtomicBool::new(false));
        let spawns = Arc::new(AtomicUsize::new(0));
        let spawner: SpawnFn = Arc::new(move |_, _| {
            spawns.fetch_add(1, Ordering::SeqCst);
            #[cfg(all(unix, not(target_os = "android")))]
            let child = Command::new("true")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            #[cfg(target_os = "windows")]
            let child = {
                use std::os::windows::process::CommandExt;
                Command::new("cmd")
                    .arg("/C")
                    .arg("exit")
                    .arg("0")
                    .creation_flags(0x0800_0000)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()?
            };
            #[cfg(target_os = "android")]
            let child = Command::new("true").spawn()?;
            Ok(child)
        });
        let connector: ConnectFn = Arc::new(|_, model, version, _| {
            Box::pin(async move {
                let (client, peer) = tokio::io::duplex(256 * 1024);
                tokio::spawn(latency_stub_peer(peer, "hello stub".to_string()));
                let mut ready = proto::Header::op(proto::OP_HELLO);
                ready.version = Some(version);
                ready.model = Some(model);
                let mut client: DynStream = Box::new(client);
                let (resp, _) = transact(&mut client, ready, &[], Duration::from_secs(5)).await?;
                if resp.op != proto::OP_READY {
                    return Err("stub handshake failed".to_string());
                }
                Ok(client)
            }) as Pin<Box<dyn Future<Output = Result<DynStream, String>> + Send>>
        });
        let worker = WorkerClient::with_hooks(spawner, connector).expect("test client");
        VoiceSessionManager::new(capture, paused).with_worker(worker)
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

    fn backdate_meta(session: &VoiceSessionManager, ago: Duration) {
        if let Some(ref mut meta) = *session.meta.lock().unwrap() {
            meta.last_used = Instant::now() - ago;
        }
    }

    fn hold_secs(session: &VoiceSessionManager) -> Option<u64> {
        session
            .meta
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|meta| meta.hold.as_secs())
    }

    #[test]
    fn test_ladder_escalates_and_caps() {
        let (session, _) = create_test_session();
        let target = session.target_model().to_string();
        session.set_test_meta(&target, Instant::now() - Duration::from_secs(5));
        assert_eq!(hold_secs(&session), Some(10), "fresh load starts at base");
        for expected in [20u64, 30, 60, 60] {
            session.record_use(target.clone());
            assert_eq!(hold_secs(&session), Some(expected));
        }
    }

    #[test]
    fn test_touch_pins_without_stepping() {
        let (session, _) = create_test_session();
        session.set_test_meta(
            "parakeet-tdt-ctc-110m",
            Instant::now() - Duration::from_secs(5),
        );
        session.touch_loaded_slot();
        assert_eq!(
            hold_secs(&session),
            Some(10),
            "session start pins without stepping"
        );
        let elapsed = session
            .meta
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .expect("meta")
            .last_used
            .elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "start refreshes last_used, got {elapsed:?}"
        );
    }

    #[test]
    fn test_ladder_stale_touch_extends_nothing() {
        let (session, _) = create_test_session();
        session.set_test_meta(
            "parakeet-tdt-ctc-110m",
            Instant::now() - Duration::from_secs(11),
        );
        session.touch_loaded_slot();
        assert_eq!(hold_secs(&session), Some(10));
        session.clean_expired_transcriber();
        assert_eq!(hold_secs(&session), None);
    }

    #[test]
    fn test_ladder_silence_evicts_and_restarts_at_base() {
        let (session, _) = create_test_session();
        let target = session.target_model().to_string();
        session.set_test_meta(&target, Instant::now());
        session.record_use(target.clone());
        session.record_use(target.clone());
        assert_eq!(hold_secs(&session), Some(30));
        backdate_meta(&session, Duration::from_secs(31));
        session.clean_expired_transcriber();
        assert_eq!(hold_secs(&session), None, "silence past deadline evicts");
        session.set_test_meta(&target, Instant::now());
        assert_eq!(hold_secs(&session), Some(10), "next load restarts at base");
    }

    #[test]
    fn test_ladder_resets_on_model_switch() {
        let (session, _) = create_test_session();
        let target = session.target_model().to_string();
        session.set_test_meta(&target, Instant::now());
        session.record_use(target.clone());
        assert_eq!(hold_secs(&session), Some(20));
        session.set_model_name("other-model");
        assert_eq!(hold_secs(&session), None);
    }

    #[test]
    fn test_dictation_steps_exactly_one_rung() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let session = create_latency_session();
        crate::platform::test_injector().clear();
        let target = session.target_model().to_string();
        session.set_test_meta(&target, Instant::now());
        assert_eq!(hold_secs(&session), Some(10));
        session.start_ptt().expect("ptt start");
        assert_eq!(
            hold_secs(&session),
            Some(10),
            "session start must pin without stepping"
        );
        session.capture().buffer().push_samples(&[0.5; 3200]);
        let res = session.stop_ptt().expect("ptt stop");
        assert!(res.is_some(), "stub worker must return a transcript");
        assert_eq!(
            hold_secs(&session),
            Some(20),
            "one dictation steps exactly one rung"
        );
    }

    #[test]
    fn test_long_recording_pin_agrees_with_worker() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let session = create_latency_session();
        crate::platform::test_injector().clear();
        let target = session.target_model().to_string();
        session.set_test_meta(&target, Instant::now());
        session.start_ptt().expect("ptt start");
        // 15s of recording elapses past the 10s base rung...
        backdate_meta(&session, Duration::from_secs(15));
        // ...but streaming audio pins last_used, as the forwarder does.
        VoiceSessionManager::pin_stream_meta(&session.meta, &target);
        session.capture().buffer().push_samples(&[0.5; 3200]);
        let res = session.stop_ptt().expect("ptt stop");
        assert!(res.is_some(), "stub worker must return a transcript");
        assert_eq!(
            hold_secs(&session),
            Some(20),
            "streaming pin must prevent reset-while-warm"
        );
    }

    #[test]
    fn test_ptt_start_inside_mic_grace_skips_capture_open() {
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
        // A prior burst leaves the mic parked in grace on the real capture.
        session.capture().start().expect("prior open");
        session.capture().stop();
        session.start_ptt().expect("press inside grace");
        assert_eq!(
            opens.load(Ordering::SeqCst),
            0,
            "press inside the grace window must reuse the held mic"
        );
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        session.cancel();
    }

    #[test]
    fn test_ptt_start_past_mic_grace_reopens_capture() {
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
        session
            .capture()
            .backdate_held_deadline_for_test(Duration::from_secs(30));
        session.start_ptt().expect("press past grace");
        assert_eq!(
            opens.load(Ordering::SeqCst),
            1,
            "press past the grace window must reopen the mic"
        );
        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        session.cancel();
    }

    fn create_preload_test_session() -> (VoiceSessionManager, Arc<AtomicUsize>) {
        let buffer = Arc::new(super::super::capture::AudioFrameBuffer::new());
        let capture = Arc::new(AudioCapture::new(buffer));
        let paused = Arc::new(AtomicBool::new(false));
        let hellos = Arc::new(AtomicUsize::new(0));
        let hellos_connector = Arc::clone(&hellos);
        let spawner: SpawnFn = Arc::new(move |_, _| {
            #[cfg(all(unix, not(target_os = "android")))]
            let child = Command::new("true")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            #[cfg(target_os = "windows")]
            let child = {
                use std::os::windows::process::CommandExt;
                Command::new("cmd")
                    .arg("/C")
                    .arg("exit")
                    .arg("0")
                    .creation_flags(0x0800_0000)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()?
            };
            #[cfg(target_os = "android")]
            let child = Command::new("true").spawn()?;
            Ok(child)
        });
        let connector: ConnectFn = Arc::new(move |_, model, version, _| {
            let hellos = Arc::clone(&hellos_connector);
            Box::pin(async move {
                hellos.fetch_add(1, Ordering::SeqCst);
                let (client, peer) = tokio::io::duplex(256 * 1024);
                tokio::spawn(latency_stub_peer(peer, "hello stub".to_string()));
                let mut ready = proto::Header::op(proto::OP_HELLO);
                ready.version = Some(version);
                ready.model = Some(model);
                let mut client: DynStream = Box::new(client);
                let (resp, _) = transact(&mut client, ready, &[], Duration::from_secs(5)).await?;
                if resp.op != proto::OP_READY {
                    return Err("stub handshake failed".to_string());
                }
                Ok(client)
            }) as Pin<Box<dyn Future<Output = Result<DynStream, String>> + Send>>
        });
        let worker = WorkerClient::with_hooks(spawner, connector).expect("test client");
        let session = VoiceSessionManager::new(capture, paused).with_worker(worker);
        (session, hellos)
    }

    #[test]
    fn test_preload_kicks_at_keypress_without_blocking_press() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, hellos) = create_preload_test_session();
        let session = session.into_test_arc();
        crate::platform::test_injector().clear();

        assert_eq!(hellos.load(Ordering::SeqCst), 0);
        assert_eq!(hold_secs(&session), None);

        let press_start = Instant::now();
        session.start_ptt().expect("ptt start");
        let press_duration = press_start.elapsed();

        assert_eq!(session.current_mode(), VoiceMode::PushToTalk);
        assert!(
            press_duration < Duration::from_millis(500),
            "press must return immediately without waiting for preload"
        );

        let deadline = Instant::now() + Duration::from_secs(2);
        while hold_secs(&session).is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(
            hold_secs(&session),
            Some(10),
            "preload thread must have initialized model slot to base hold"
        );
        assert!(
            hellos.load(Ordering::SeqCst) >= 1,
            "preload hello must reach peer while recording"
        );

        session.cancel();
    }

    #[test]
    fn test_preload_not_kicked_on_mash_bounce() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _data = TempDataDir::new();
        mock_keystore::use_mock_keystore();
        let (session, hellos) = create_preload_test_session();
        let session = session.into_test_arc();
        crate::platform::test_injector().clear();

        *session.last_stop.lock().unwrap() = Some(Instant::now());
        crate::input::hotkey::PTT_KEY_DOWN.store(false, Ordering::Relaxed);

        session.start_ptt().expect("start ptt inside mash window");
        assert_eq!(session.current_mode(), VoiceMode::Idle);

        std::thread::sleep(Duration::from_millis(super::RESCUE_WAIT_MS + 50));

        assert_eq!(session.current_mode(), VoiceMode::Idle);
        assert_eq!(
            hellos.load(Ordering::SeqCst),
            0,
            "preload must never fire for bounces"
        );
        assert_eq!(hold_secs(&session), None);
    }
}
