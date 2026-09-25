// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

//! Serving loop for the isolated voice worker (`taurine --voice-daemon`).
//!
//! Holds the single configured recognizer plus per-request audio buffers.
//! The daemon streams chunks live during recording and issues one final
//! `transcribe`, so decode always sees the full utterance (batch accuracy)
//! while load and buffering overlap speech (streaming latency).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use taurine_core::error::{Error, Result};
use taurine_core::voice::{Transcriber, is_model_downloaded};

use super::capture::{
    MAX_BUFFER_SAMPLES, leading_silence_frames, normalize_snippet_rms,
    slice_utterance_with_postroll, slice_utterance_with_preroll, trailing_silence_frames,
};
use super::factory::create_transcriber;
use super::session::{MIN_SAMPLES_COUNT, MIN_SPEECH_RMS_ENERGY};
use super::worker_protocol as proto;
use super::worker_protocol::Header;

/// Hold ladder rungs: a fresh load holds 15s; genuine use inside the
/// window steps one rung up to the 120s cap. The daemon metadata mirror
/// in session.rs carries the same table; both must agree.
const HOLD_RUNGS_SECS: [u64; 4] = [15, 30, 60, 120];

fn base_hold() -> Duration {
    Duration::from_secs(HOLD_RUNGS_SECS[0])
}

fn next_rung(hold: Duration) -> Duration {
    match HOLD_RUNGS_SECS.iter().position(|r| *r == hold.as_secs()) {
        Some(i) => Duration::from_secs(HOLD_RUNGS_SECS[(i + 1).min(HOLD_RUNGS_SECS.len() - 1)]),
        None => base_hold(),
    }
}
/// Read quantum; each expiry sweeps the model TTL. Pipe silence never exits:
/// the worker lives for the daemon lifetime and is reclaimed only at daemon
/// shutdown (shutdown op), on disconnect, or on crash (daemon respawns).
const SWEEP_INTERVAL: Duration = Duration::from_secs(1);

struct Slot {
    transcriber: Box<dyn Transcriber>,
    model_id: String,
    last_used: Instant,
    hold: Duration,
}

impl Slot {
    /// Genuine use inside the current window steps one rung up (capped at
    /// 120s) and logs one debug line; use past the deadline restarts at
    /// base. Streaming appends pin `last_used` directly and never call this.
    fn note_use(&mut self) {
        if self.last_used.elapsed() <= self.hold {
            let next = next_rung(self.hold);
            if next != self.hold {
                self.hold = next;
                debug!(
                    "voice-daemon: extending voice model hold to {}s",
                    self.hold.as_secs()
                );
            }
        } else {
            self.hold = base_hold();
        }
        self.last_used = Instant::now();
    }
}

struct WorkerState {
    model: String,
    model_dir: PathBuf,
    slot: Option<Slot>,
    /// Model load running off the serve loop; awaited before first decode.
    loading: Option<(String, tokio::task::JoinHandle<Box<dyn Transcriber>>)>,
    buffers: HashMap<String, Vec<f32>>,
    warned_missing: bool,
    last_frame: Instant,
}

impl WorkerState {
    /// Fresh means the loaded model matches and activity is inside its rung.
    fn slot_fresh(&self) -> bool {
        match &self.slot {
            Some(slot) => slot.model_id == self.model && slot.last_used.elapsed() <= slot.hold,
            None => false,
        }
    }

    /// Dispatch a background load unless the slot is fresh or one is running.
    /// Skipped when model files are absent (transcribe reports it instead).
    fn kick_off_load(&mut self) {
        if self.slot_fresh() || self.loading.is_some() {
            return;
        }
        if !is_model_downloaded(&self.model, Some(&self.model_dir)) {
            return;
        }
        debug!("voice-daemon: loading voice model '{}'", self.model);
        let model = self.model.clone();
        let dir = self.model_dir.clone();
        let thread_model = model.clone();
        self.loading = Some((
            model,
            tokio::task::spawn_blocking(move || create_transcriber(&thread_model, Some(&dir))),
        ));
    }

    /// Await any in-flight load, then fall back to inline init if needed.
    async fn ensure_slot(&mut self) {
        if self.slot_fresh() {
            return;
        }
        if let Some((model, handle)) = self.loading.take()
            && let Ok(transcriber) = handle.await
            && model == self.model
        {
            self.slot = Some(Slot {
                transcriber,
                model_id: model,
                last_used: Instant::now(),
                hold: base_hold(),
            });
            return;
        }
        // Last resort when no background load ran (e.g. direct transcribe
        // without hello): block the loop rather than fail the utterance.
        if !is_model_downloaded(&self.model, Some(&self.model_dir)) {
            return;
        }
        debug!("voice-daemon: loading voice model '{}'", self.model);
        self.slot = Some(Slot {
            transcriber: create_transcriber(&self.model, Some(&self.model_dir)),
            model_id: self.model.clone(),
            last_used: Instant::now(),
            hold: base_hold(),
        });
    }

    fn sweep(&mut self) {
        if let Some(ref slot) = self.slot
            && slot.last_used.elapsed() > slot.hold
        {
            let hold_secs = slot.hold.as_secs();
            self.slot = None;
            debug!("voice-daemon: unloading idle voice model from RAM ({hold_secs}s hold expired)");
        }
    }
}

/// Serve one decoded request frame, returning response frames.
async fn handle_frame(
    state: &mut WorkerState,
    header: Header,
    body: Vec<u8>,
    own_version: &str,
    expected_token: &str,
) -> Vec<(Header, Vec<u8>)> {
    state.last_frame = Instant::now();
    let req_id = header.req_id.clone();
    let respond = |mut out: Header| {
        out.req_id = req_id.clone();
        (out, Vec::new())
    };
    match header.op.as_str() {
        proto::OP_HELLO => match proto::check_hello(&header, own_version, expected_token) {
            Ok(()) => {
                state.model = header.model.clone().unwrap_or_else(|| "auto".to_string());
                state.model =
                    taurine_core::voice::resolve_configured_model(&state.model).to_string();
                let mut ready = Header::op(proto::OP_READY);
                ready.version = Some(own_version.to_string());
                ready.model = Some(state.model.clone());
                info!("voice-daemon: serving model '{}'", state.model);
                // Overlap init with speech: the slot is warm by stop time.
                // A model-free warm handshake carries no model and loads nothing.
                if header
                    .model
                    .as_deref()
                    .is_some_and(|m| !m.trim().is_empty())
                {
                    state.kick_off_load();
                }
                vec![respond(ready)]
            }
            Err(e) => {
                let mut err = Header::op(proto::OP_ERROR);
                err.message = Some(e);
                vec![respond(err)]
            }
        },
        proto::OP_APPEND => {
            let id = req_id.clone().unwrap_or_default();
            // Streaming audio counts as activity: a long recording must not
            // evict its own model mid-session (same pin as the daemon side).
            if let Some(ref mut slot) = state.slot {
                slot.last_used = Instant::now();
            }
            match proto::decode_samples(&body) {
                Ok(samples) => {
                    let buf = state.buffers.entry(id).or_default();
                    buf.extend_from_slice(&samples);
                    if buf.len() > MAX_BUFFER_SAMPLES {
                        let overflow = buf.len() - MAX_BUFFER_SAMPLES;
                        buf.drain(..overflow);
                    }
                    let mut ack = Header::op(proto::OP_ACK);
                    ack.seq = header.seq;
                    vec![respond(ack)]
                }
                Err(e) => {
                    let mut err = Header::op(proto::OP_ERROR);
                    err.message = Some(e);
                    vec![respond(err)]
                }
            }
        }
        proto::OP_TRANSCRIBE => {
            let id = req_id.clone().unwrap_or_default();
            let samples = state.buffers.remove(&id).unwrap_or_default();
            // Cold-model wait accounting: when the stop event outruns the
            // overlapped load, the user feels it as dictation lag. Log it so
            // the hold policy can be tuned from evidence, not feel.
            let slot_at = Instant::now();
            state.ensure_slot().await;
            let slot_wait_ms = slot_at.elapsed().as_secs_f64() * 1000.0;
            if slot_wait_ms > 250.0 {
                debug!(
                    "voice-daemon: model slot wait {:.2}ms before decode (cold load at stop)",
                    slot_wait_ms
                );
            }
            vec![respond(transcribe_buffer(state, &samples))]
        }
        proto::OP_PING => vec![respond(Header::op(proto::OP_PONG))],
        proto::OP_UNLOAD => {
            if let Some((_, handle)) = state.loading.take() {
                handle.abort();
            }
            state.slot = None;
            vec![respond(Header::op(proto::OP_ACK))]
        }
        _ => {
            let mut err = Header::op(proto::OP_ERROR);
            err.message = Some(format!("unknown voice op '{}'", header.op));
            vec![respond(err)]
        }
    }
}

/// Gate, normalize, and decode one buffered utterance into a result header.
///
/// The slot must be warm before calling: the transcribe arm awaits
/// `ensure_slot`, so this stays synchronous and never blocks the loop.
fn transcribe_buffer(state: &mut WorkerState, samples: &[f32]) -> Header {
    let mut out = Header::op(proto::OP_RESULT);
    let leading_frames = leading_silence_frames(samples);
    let front_trimmed = slice_utterance_with_preroll(samples, leading_frames);
    let silence_frames = trailing_silence_frames(&front_trimmed);
    let trimmed = slice_utterance_with_postroll(&front_trimmed, silence_frames);
    if trimmed.len() < MIN_SAMPLES_COUNT {
        return out;
    }
    let energy: f32 = trimmed.iter().map(|s| s * s).sum::<f32>() / trimmed.len() as f32;
    if energy < MIN_SPEECH_RMS_ENERGY {
        return out;
    }
    if !is_model_downloaded(&state.model, Some(&state.model_dir)) {
        if !state.warned_missing {
            state.warned_missing = true;
            out.message = Some(format!(
                "voice model '{}' files are missing; dictation unavailable until download",
                state.model
            ));
        }
        return out;
    }
    let normed = normalize_snippet_rms(&trimmed);
    let Some(ref mut slot) = state.slot else {
        return out;
    };
    match slot.transcriber.transcribe(&normed, 16000) {
        Ok(t) => {
            slot.note_use();
            out.text = Some(t.text);
            out.confidence = Some(t.confidence);
            out.duration_secs = Some(t.duration_secs);
            out
        }
        Err(e) => {
            warn!("voice-daemon: transcription failed: {e}");
            out.message = Some(format!("voice transcription error: {e}"));
            out
        }
    }
}

/// Read one full frame with model sweeps. `Ok(None)` means peer disconnect.
/// Silence never exits; the worker lives for the daemon lifetime.
async fn read_frame<S>(stream: &mut S, state: &mut WorkerState) -> Result<Option<(Header, Vec<u8>)>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::new();
    loop {
        match tokio::time::timeout(SWEEP_INTERVAL, stream.read_buf(&mut buf)).await {
            Err(_) => {
                state.sweep();
                debug!(
                    idle_secs = state.last_frame.elapsed().as_secs(),
                    "voice-daemon: pipe sweep"
                );
                continue;
            }
            Ok(Err(e)) => return Err(Error::Io(e)),
            Ok(Ok(0)) if buf.is_empty() => return Ok(None),
            Ok(Ok(0)) => {
                return Err(Error::Engine("voice pipe closed mid-frame".to_string()));
            }
            Ok(Ok(_)) => {}
        }
        match proto::decode_frame(&mut buf).map_err(Error::Engine)? {
            Some(frame) => return Ok(Some(frame)),
            None => continue,
        }
    }
}

/// Serve a single connected daemon until shutdown or disconnect.
async fn serve_stream<S>(
    stream: &mut S,
    model_dir: PathBuf,
    own_version: String,
    expected_token: String,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let mut state = WorkerState {
        model: "auto".to_string(),
        model_dir,
        slot: None,
        loading: None,
        buffers: HashMap::new(),
        warned_missing: false,
        last_frame: Instant::now(),
    };
    // First frame must be the handshake.
    let (hello_header, hello_body) = match read_frame(stream, &mut state).await? {
        Some(frame) => frame,
        None => return Ok(()),
    };
    for (resp, body) in handle_frame(
        &mut state,
        hello_header,
        hello_body,
        &own_version,
        &expected_token,
    )
    .await
    {
        let bytes = proto::encode_frame(&resp, &body).map_err(Error::Engine)?;
        stream.write_all(&bytes).await.map_err(Error::Io)?;
        if resp.op == proto::OP_ERROR {
            return Ok(());
        }
    }
    loop {
        let (header, body) = match read_frame(stream, &mut state).await? {
            Some(frame) => frame,
            None => return Ok(()),
        };
        if header.op == proto::OP_SHUTDOWN {
            return Ok(());
        }
        for (resp, body) in
            handle_frame(&mut state, header, body, &own_version, &expected_token).await
        {
            let bytes = proto::encode_frame(&resp, &body).map_err(Error::Engine)?;
            stream.write_all(&bytes).await.map_err(Error::Io)?;
        }
        state.sweep();
    }
}

/// Entry point for `taurine --voice-daemon`.
pub fn run(pipe: Option<String>, version_token: Option<String>) -> Result<()> {
    let pipe = pipe
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| Error::Config("voice worker requires --voice-pipe".to_string()))?;
    let expected_token = version_token.unwrap_or_default();
    let model_dir = taurine_core::system::paths::ensure_data_dir().join("models");
    let own_version = env!("CARGO_PKG_VERSION").to_string();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Io)?;
    rt.block_on(serve(pipe, model_dir, own_version, expected_token))
}

#[cfg(all(unix, not(target_os = "android")))]
async fn serve(
    pipe: String,
    model_dir: PathBuf,
    own_version: String,
    expected_token: String,
) -> Result<()> {
    use tokio::net::UnixListener;
    let path = proto::socket_path(&pipe);
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(Error::Io)?;
    // SAFETY: fd comes from a live listener owned by this task; fchmod
    // on a fd cannot race path replacement the way a path chmod can.
    let rc = unsafe { libc::fchmod(std::os::unix::io::AsRawFd::as_raw_fd(&listener), 0o600) };
    if rc != 0 {
        return Err(Error::Io(std::io::Error::last_os_error()));
    }
    let (mut stream, _) = listener.accept().await.map_err(Error::Io)?;
    let res = serve_stream(&mut stream, model_dir, own_version, expected_token).await;
    let _ = std::fs::remove_file(&path);
    res
}

#[cfg(target_os = "windows")]
async fn serve(
    pipe: String,
    model_dir: PathBuf,
    own_version: String,
    expected_token: String,
) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let full = proto::windows_pipe_path(&pipe);
    // Prefer the same-user DACL helper when available; fall back to a
    // local-only instance so a missing helper never blocks dictation.
    let mut server =
        match crate::platform::windows::pipe_security::PipeSecurity::current_user_only() {
            Ok(security) => security.create_server(&full, true).map_err(Error::Io)?,
            Err(e) => {
                warn!("voice-daemon: same-user pipe DACL unavailable ({e}); using local-only pipe");
                ServerOptions::new()
                    .first_pipe_instance(true)
                    .reject_remote_clients(true)
                    .create(&full)
                    .map_err(Error::Io)?
            }
        };
    server.connect().await.map_err(Error::Io)?;
    serve_stream(&mut server, model_dir, own_version, expected_token).await
}

#[cfg(target_os = "android")]
async fn serve(_pipe: String, _model_dir: PathBuf, _own: String, _token: String) -> Result<()> {
    Err(Error::Config(
        "voice worker not supported on android".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> WorkerState {
        WorkerState {
            model: "parakeet-tdt-ctc-110m".to_string(),
            model_dir: std::env::temp_dir().join("taurine-worker-test-models"),
            slot: None,
            loading: None,
            buffers: HashMap::new(),
            warned_missing: false,
            last_frame: Instant::now(),
        }
    }

    struct LenRecorderTranscriber {
        seen_len: std::sync::Arc<std::sync::Mutex<usize>>,
    }
    impl taurine_core::voice::Transcriber for LenRecorderTranscriber {
        fn name(&self) -> &str {
            "len-recorder"
        }
        fn transcribe(
            &mut self,
            audio: &[f32],
            _sample_rate: u32,
        ) -> taurine_core::error::Result<taurine_core::voice::Transcription> {
            *self.seen_len.lock().unwrap_or_else(|p| p.into_inner()) = audio.len();
            Ok(taurine_core::voice::Transcription::new("", 0.0, 0.0))
        }
    }

    fn recorded_state() -> (WorkerState, std::sync::Arc<std::sync::Mutex<usize>>) {
        // Stub model files so the downloaded-model gate passes hermetically.
        let dir = std::env::temp_dir().join("taurine-worker-trim-models");
        let sub = dir.join("parakeet-110m");
        let _ = std::fs::create_dir_all(&sub);
        let _ = std::fs::write(sub.join("model.int8.onnx"), b"not a model");
        let _ = std::fs::write(sub.join("tokens.txt"), b"a b");
        let seen = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let mut state = test_state();
        state.model_dir = dir;
        state.slot = Some(Slot {
            transcriber: Box::new(LenRecorderTranscriber {
                seen_len: seen.clone(),
            }),
            model_id: state.model.clone(),
            last_used: Instant::now(),
            hold: Duration::from_secs(15),
        });
        (state, seen)
    }

    #[test]
    fn test_transcribe_trims_leading_silence_with_preroll() {
        let (mut state, seen) = recorded_state();
        // 12 silent frames + 10 speech frames: decoder must see speech plus
        // exactly 8 frames (4096 samples) of pre-roll cushion.
        let mut samples = vec![0.0f32; 12 * 512];
        samples.extend(vec![0.5f32; 5120]);
        let out = transcribe_buffer(&mut state, &samples);
        assert!(out.text.is_some());
        assert_eq!(
            *seen.lock().unwrap_or_else(|p| p.into_inner()),
            5120 + 8 * 512
        );
        let _ = std::fs::remove_dir_all(&state.model_dir);
    }

    #[test]
    fn test_transcribe_all_silence_hits_gate_without_model() {
        let mut state = test_state();
        let out = transcribe_buffer(&mut state, &vec![0.0f32; 16000]);
        assert_eq!(out.op, proto::OP_RESULT);
        assert!(out.text.is_none());
        assert!(state.slot.is_none(), "silence must not load the model");
    }

    #[test]
    fn test_transcribe_rejects_silence_without_model() {
        let mut state = test_state();
        let out = transcribe_buffer(&mut state, &vec![0.0f32; 16000]);
        assert_eq!(out.op, proto::OP_RESULT);
        assert!(out.text.is_none());
        assert!(state.slot.is_none(), "silence must not load the model");
    }

    #[test]
    fn test_transcribe_rejects_short_audio_without_model() {
        let mut state = test_state();
        let out = transcribe_buffer(&mut state, &vec![0.5f32; 500]);
        assert!(out.text.is_none());
        assert!(state.slot.is_none(), "short audio must not load the model");
    }

    fn hello_header(model: &str) -> Header {
        let mut hello = Header::op(proto::OP_HELLO);
        hello.req_id = Some("h1".to_string());
        hello.version = Some(env!("CARGO_PKG_VERSION").to_string());
        hello.model = Some(model.to_string());
        hello.token = Some("tok".to_string());
        hello
    }

    #[tokio::test]
    async fn test_hello_dispatches_eager_load() {
        // Present (tiny, invalid) model files: hello must start a
        // background load instead of waiting for first decode.
        let dir = std::env::temp_dir().join("taurine-worker-eager-models");
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("parakeet-110m");
        std::fs::create_dir_all(&sub).expect("temp model dir");
        std::fs::write(sub.join("model.int8.onnx"), b"not a model").expect("write");
        std::fs::write(sub.join("tokens.txt"), b"a b").expect("write");
        let mut state = WorkerState {
            model: "auto".to_string(),
            model_dir: dir.clone(),
            slot: None,
            loading: None,
            buffers: HashMap::new(),
            warned_missing: false,
            last_frame: Instant::now(),
        };
        let version = env!("CARGO_PKG_VERSION").to_string();
        let out = handle_frame(
            &mut state,
            hello_header("parakeet-tdt-ctc-110m"),
            Vec::new(),
            &version,
            "tok",
        )
        .await;
        assert_eq!(out.len(), 1);
        assert!(state.loading.is_some(), "hello must kick off the load");
        if let Some((_, handle)) = state.loading.take() {
            handle.abort();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_hello_without_files_loads_nothing() {
        let dir = std::env::temp_dir().join("taurine-worker-nomodel-models");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mut state = WorkerState {
            model: "auto".to_string(),
            model_dir: dir.clone(),
            slot: None,
            loading: None,
            buffers: HashMap::new(),
            warned_missing: false,
            last_frame: Instant::now(),
        };
        let version = env!("CARGO_PKG_VERSION").to_string();
        handle_frame(
            &mut state,
            hello_header("parakeet-tdt-ctc-110m"),
            Vec::new(),
            &version,
            "tok",
        )
        .await;
        assert!(
            state.loading.is_none(),
            "missing files must not spawn a load"
        );
        assert!(state.slot.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn round_trip(
        client: &mut tokio::io::DuplexStream,
        header: Header,
        body: &[u8],
    ) -> (Header, Vec<u8>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut out = header;
        if out.req_id.is_none() {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            out.req_id = Some(format!(
                "e{}",
                N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        }
        let bytes = proto::encode_frame(&out, body).expect("encode");
        client.write_all(&bytes).await.expect("write");
        let mut buf = Vec::new();
        loop {
            let mut chunk = [0u8; 4096];
            let n = client.read(&mut chunk).await.expect("read");
            assert!(n > 0, "worker closed the pipe mid-test");
            buf.extend_from_slice(&chunk[..n]);
            if let Some(frame) = proto::decode_frame(&mut buf).expect("decode") {
                assert_eq!(frame.0.req_id, out.req_id);
                return frame;
            }
        }
    }

    struct DummyTranscriber;
    impl taurine_core::voice::Transcriber for DummyTranscriber {
        fn name(&self) -> &str {
            "dummy"
        }
        fn transcribe(
            &mut self,
            _audio: &[f32],
            _sample_rate: u32,
        ) -> taurine_core::error::Result<taurine_core::voice::Transcription> {
            Ok(taurine_core::voice::Transcription::new("", 0.0, 0.0))
        }
    }

    fn slotted_state() -> WorkerState {
        let mut state = test_state();
        state.slot = Some(Slot {
            transcriber: Box::new(DummyTranscriber),
            model_id: state.model.clone(),
            last_used: Instant::now(),
            hold: Duration::from_secs(15),
        });
        state
    }

    fn runged_state(hold_secs: u64, last_used_ago: Duration) -> WorkerState {
        let mut state = test_state();
        state.slot = Some(Slot {
            transcriber: Box::new(DummyTranscriber),
            model_id: state.model.clone(),
            last_used: Instant::now() - last_used_ago,
            hold: Duration::from_secs(hold_secs),
        });
        state
    }

    #[test]
    fn test_ladder_escalates_one_rung_per_use() {
        let mut state = runged_state(15, Duration::from_secs(10));
        for expected in [30u64, 60, 120] {
            state.slot.as_mut().expect("slot").note_use();
            assert_eq!(
                state.slot.as_ref().expect("slot").hold,
                Duration::from_secs(expected)
            );
            state.slot.as_mut().expect("slot").last_used = Instant::now() - Duration::from_secs(10);
        }
    }

    #[test]
    fn test_ladder_caps_at_120s() {
        let mut state = runged_state(120, Duration::from_secs(10));
        for _ in 0..3 {
            state.slot.as_mut().expect("slot").note_use();
            state.slot.as_mut().expect("slot").last_used = Instant::now() - Duration::from_secs(10);
        }
        assert_eq!(
            state.slot.as_ref().expect("slot").hold,
            Duration::from_secs(120),
            "repeated use never exceeds the cap"
        );
        state.slot.as_mut().expect("slot").last_used = Instant::now() - Duration::from_secs(100);
        state.sweep();
        assert!(state.slot.is_some(), "100s fits the 120s cap rung");
        state.slot.as_mut().expect("slot").last_used = Instant::now() - Duration::from_secs(121);
        state.sweep();
        assert!(state.slot.is_none(), "silence past the cap evicts");
    }

    #[test]
    fn test_ladder_stale_use_restarts_at_base() {
        let mut state = runged_state(60, Duration::from_secs(61));
        state.slot.as_mut().expect("slot").note_use();
        assert_eq!(
            state.slot.as_ref().expect("slot").hold,
            Duration::from_secs(15)
        );
    }

    #[test]
    fn test_long_recording_pin_then_use_steps_up() {
        // Worker mirror of the daemon long-recording case: streaming appends
        // pin last_used without stepping, so the transcribe success steps one
        // rung up instead of resetting to base.
        let mut state = runged_state(15, Duration::from_secs(20));
        state.slot.as_mut().expect("slot").last_used = Instant::now();
        state.slot.as_mut().expect("slot").note_use();
        assert_eq!(
            state.slot.as_ref().expect("slot").hold,
            Duration::from_secs(30)
        );
    }

    #[tokio::test]
    async fn test_ladder_restarts_at_base_after_eviction() {
        let mut state = runged_state(60, Duration::from_secs(61));
        state.sweep();
        assert!(
            state.slot.is_none(),
            "silence past the rung deadline evicts"
        );
        let model = state.model.clone();
        state.loading = Some((
            model,
            tokio::task::spawn_blocking(|| Box::new(DummyTranscriber) as Box<dyn Transcriber>),
        ));
        state.ensure_slot().await;
        assert_eq!(
            state.slot.as_ref().expect("slot").hold,
            Duration::from_secs(15),
            "next load restarts at base"
        );
    }

    #[tokio::test]
    async fn test_ladder_resets_on_model_switch() {
        let mut state = runged_state(120, Duration::from_secs(5));
        state.model = "other-model".to_string();
        state.loading = Some((
            state.model.clone(),
            tokio::task::spawn_blocking(|| Box::new(DummyTranscriber) as Box<dyn Transcriber>),
        ));
        state.ensure_slot().await;
        let slot = state.slot.as_ref().expect("slot");
        assert_eq!(slot.model_id, "other-model");
        assert_eq!(slot.hold, Duration::from_secs(15));
    }

    #[tokio::test]
    async fn test_streaming_audio_pins_the_model() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut client_stream, mut server_stream) = tokio::io::duplex(256 * 1024);
        let mut state = slotted_state();
        // 10s past activity is warm on the 15s base rung: sweep retains.
        state.slot.as_mut().expect("slot").last_used = Instant::now() - Duration::from_secs(10);
        state.sweep();
        assert!(state.slot.is_some(), "10s past activity is warm");
        // Backdate past the TTL, then stream: activity must refresh.
        state.slot.as_mut().expect("slot").last_used = Instant::now() - Duration::from_secs(90);
        let mut append = hello_header("parakeet-tdt-ctc-110m");
        append.op = proto::OP_APPEND.to_string();
        append.req_id = Some("s9".to_string());
        append.seq = Some(0);
        let body = proto::encode_samples(&vec![0.5f32; 1600]);
        let bytes = proto::encode_frame(&append, &body).expect("encode");
        client_stream.write_all(&bytes).await.expect("write");
        // Drive one server iteration manually: read, handle, respond.
        let mut buf = Vec::new();
        let mut chunk = [0u8; 65536];
        let n = server_stream.read(&mut chunk).await.expect("read");
        buf.extend_from_slice(&chunk[..n]);
        let (header, body) = proto::decode_frame(&mut buf)
            .expect("decode")
            .expect("frame");
        let version = env!("CARGO_PKG_VERSION").to_string();
        let out = handle_frame(&mut state, header, body, &version, "tok").await;
        assert_eq!(out.len(), 1);
        let bytes = proto::encode_frame(&out[0].0, &out[0].1).expect("encode");
        server_stream.write_all(&bytes).await.expect("write");
        // The stale slot survived because audio streamed through it.
        assert!(state.slot.is_some());
        state.sweep();
        assert!(state.slot.is_some(), "streamed audio pins past TTL");

        // Silence afterwards lets it expire.
        state.slot.as_mut().expect("slot").last_used = Instant::now() - Duration::from_secs(90);
        state.sweep();
        assert!(state.slot.is_none(), "90s past activity is evicted");
    }

    #[tokio::test]
    async fn test_long_silence_never_exits_worker() {
        use tokio::io::AsyncWriteExt;
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        let mut state = test_state();
        state.last_frame = Instant::now() - Duration::from_secs(90);
        let mut ping = Header::op(proto::OP_PING);
        ping.req_id = Some("p-long".to_string());
        let bytes = proto::encode_frame(&ping, &[]).expect("encode");
        client.write_all(&bytes).await.expect("write");
        let frame = read_frame(&mut server, &mut state)
            .await
            .expect("stays alive across long silence")
            .expect("frame");
        assert_eq!(frame.0.op, proto::OP_PING);
    }

    #[tokio::test]
    async fn test_serve_stream_end_to_end_without_model_files() {
        let (client_stream, server_stream) = tokio::io::duplex(256 * 1024);
        let mut client_stream = client_stream;
        let dir = std::env::temp_dir().join("taurine-worker-e2e-models");
        let _ = std::fs::remove_dir_all(&dir);
        let version = env!("CARGO_PKG_VERSION").to_string();
        let server = tokio::spawn(async move {
            let mut server_stream = server_stream;
            serve_stream(&mut server_stream, dir, version, "tok".to_string()).await
        });

        let mut hello = Header::op(proto::OP_HELLO);
        hello.version = Some(env!("CARGO_PKG_VERSION").to_string());
        hello.model = Some("parakeet-tdt-ctc-110m".to_string());
        hello.token = Some("tok".to_string());
        let (ready, _) = round_trip(&mut client_stream, hello, &[]).await;
        assert_eq!(ready.op, proto::OP_READY);

        let mut append = Header::op(proto::OP_APPEND);
        append.req_id = Some("s1".to_string());
        append.seq = Some(0);
        let loud = proto::encode_samples(&vec![0.5f32; 16000]);
        let (ack, _) = round_trip(&mut client_stream, append, &loud).await;
        assert_eq!(ack.op, proto::OP_ACK);

        let mut decode = Header::op(proto::OP_TRANSCRIBE);
        decode.req_id = Some("s1".to_string());
        let (result, _) = round_trip(&mut client_stream, decode, &[]).await;
        assert_eq!(result.op, proto::OP_RESULT);
        // No model files under temp: empty result plus a one-time notice.
        assert!(result.text.is_none());
        assert!(
            result
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("missing")
        );

        let (pong, _) = round_trip(&mut client_stream, Header::op(proto::OP_PING), &[]).await;
        assert_eq!(pong.op, proto::OP_PONG);

        let mut bye = Header::op(proto::OP_SHUTDOWN);
        bye.req_id = Some("bye".to_string());
        let bytes = proto::encode_frame(&bye, &[]).expect("encode");
        use tokio::io::AsyncWriteExt;
        client_stream.write_all(&bytes).await.expect("write");
        drop(client_stream);
        server.await.expect("server task").expect("clean shutdown");
    }
}
