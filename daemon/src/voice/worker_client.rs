// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

//! Daemon-side client for the isolated voice worker.
//!
//! Owns the singleton worker lifecycle: spawn on first use, reuse while
//! healthy, respawn after crashes. Every request carries its own ID so a
//! stale response can never be mistaken for a live one.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::{debug, warn};

use super::worker_protocol as proto;
use super::worker_protocol::Header;

/// Combined stream bound so transports erase into one boxed type.
pub trait PipeStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> PipeStream for T {}

/// Boxed transport: real pipe in production, duplex stub in tests.
pub type DynStream = Box<dyn PipeStream + Send>;

/// Spawn function: pipe name + instance token in, live child out.
pub type SpawnFn = Arc<dyn Fn(&str, &str) -> io::Result<Child> + Send + Sync>;
/// Connect function: pipe name + hello fields in, handshaked stream out.
pub type ConnectFn = Arc<
    dyn Fn(
            String,
            String,
            String,
            String,
        ) -> Pin<Box<dyn Future<Output = Result<DynStream, String>> + Send>>
        + Send
        + Sync,
>;

/// Transcribed utterance returned by the worker.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerTranscript {
    /// Raw transcribed text (daemon applies dictionary and formatting).
    pub text: String,
    /// Recognizer confidence 0.0 to 1.0.
    pub confidence: f32,
    /// Audio duration seconds.
    pub duration_secs: f32,
}

/// Owns the singleton worker child and its pipe. All methods are sync;
/// IO runs on an internal current-thread runtime. Clone shares the worker.
///
/// Lifetime rule: never drop the last clone inside async context (dropping
/// the embedded runtime there panics). The daemon holds one static instance,
/// so this only constrains tests, which forget instead of dropping.
#[derive(Clone)]
pub struct WorkerClient {
    rt: Arc<tokio::runtime::Runtime>,
    inner: Arc<Mutex<Inner>>,
    spawner: SpawnFn,
    connector: ConnectFn,
}

struct Inner {
    child: Option<Child>,
    stream: Option<DynStream>,
    model: String,
    token: String,
}

fn default_spawner(pipe: &str, token: &str) -> io::Result<Child> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--voice-daemon")
        .arg("--voice-pipe")
        .arg(pipe)
        .arg("--voice-version-token")
        .arg(token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
}

fn pipe_identity() -> (String, String) {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    (
        format!("taurine-voice-{pid}-{nanos:x}{n:x}"),
        format!("{nanos:x}-{n:x}-{pid}"),
    )
}

async fn connect_pipe(
    pipe: String,
    model: String,
    version: String,
    token: String,
) -> Result<DynStream, String> {
    let stream: DynStream = {
        #[cfg(all(unix, not(target_os = "android")))]
        {
            let path = proto::socket_path(&pipe);
            Box::new(
                tokio::net::UnixStream::connect(&path)
                    .await
                    .map_err(|e| format!("voice worker connect failed: {e}"))?,
            )
        }
        #[cfg(target_os = "windows")]
        {
            use tokio::net::windows::named_pipe::ClientOptions;
            let full = proto::windows_pipe_path(&pipe);
            Box::new(
                ClientOptions::new()
                    .open(&full)
                    .map_err(|e| format!("voice worker connect failed: {e}"))?,
            )
        }
        #[cfg(target_os = "android")]
        {
            let _ = (pipe, model, version, token);
            return Err("voice worker is not supported on this platform".to_string());
        }
    };
    #[cfg(not(target_os = "android"))]
    {
        let mut ready = Header::op(proto::OP_HELLO);
        ready.version = Some(version);
        ready.model = Some(model);
        ready.token = Some(token);
        let mut stream = stream;
        let (resp, _) = transact(&mut stream, ready, &[], Duration::from_secs(5)).await?;
        if resp.op == proto::OP_READY {
            return Ok(stream);
        }
        let detail = resp.message.unwrap_or_else(|| resp.op.clone());
        Err(format!("voice worker handshake failed: {detail}"))
    }
}

/// One framed request/response round-trip with a total budget.
pub async fn transact<S>(
    stream: &mut S,
    mut header: Header,
    body: &[u8],
    timeout: Duration,
) -> Result<(Header, Vec<u8>), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    static REQ_COUNTER: AtomicU64 = AtomicU64::new(0);
    if header.req_id.is_none() {
        let id = format!("q{}", REQ_COUNTER.fetch_add(1, Ordering::Relaxed));
        header.req_id = Some(id);
    }
    let id = header.req_id.clone().unwrap_or_default();
    let bytes = proto::encode_frame(&header, body)?;
    let run = async {
        stream
            .write_all(&bytes)
            .await
            .map_err(|e| format!("voice pipe write failed: {e}"))?;
        let mut buf = Vec::new();
        loop {
            let mut chunk = [0u8; 4096];
            let n = stream
                .read(&mut chunk)
                .await
                .map_err(|e| format!("voice pipe read failed: {e}"))?;
            if n == 0 {
                return Err(if buf.is_empty() {
                    "voice worker closed the pipe".to_string()
                } else {
                    "voice pipe closed mid-frame".to_string()
                });
            }
            buf.extend_from_slice(&chunk[..n]);
            match proto::decode_frame(&mut buf)? {
                Some((resp, resp_body)) => {
                    if resp.req_id.as_deref() != Some(id.as_str()) {
                        return Err("voice worker answered the wrong request".to_string());
                    }
                    if resp.op == proto::OP_ERROR {
                        let detail = resp
                            .message
                            .clone()
                            .unwrap_or_else(|| "unknown error".to_string());
                        return Err(format!("voice worker error: {detail}"));
                    }
                    return Ok((resp, resp_body));
                }
                None => continue,
            }
        }
    };
    tokio::time::timeout(timeout, run)
        .await
        .map_err(|_| "voice worker timed out".to_string())?
}

/// Drive a future from sync code regardless of caller context.
///
/// Plain threads use the embedded current-thread runtime. Callers already
/// inside the daemon multi-thread runtime (config reload, pause coordinator)
/// must not nest `block_on`, which panics, so they yield the executor thread
/// with `block_in_place` instead.
fn block_on_client<F: Future>(rt: &tokio::runtime::Runtime, fut: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle)
            if matches!(
                handle.runtime_flavor(),
                tokio::runtime::RuntimeFlavor::MultiThread
            ) =>
        {
            tokio::task::block_in_place(|| handle.block_on(fut))
        }
        _ => rt.block_on(fut),
    }
}

/// Recover a poisoned client mutex. A panic mid-call must degrade to one
/// failed request, never a permanently bricked voice path.
fn lock_client(inner: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl WorkerClient {
    /// Production client spawning `current_exe --voice-daemon`.
    pub fn new() -> io::Result<Self> {
        Self::with_hooks(
            Arc::new(default_spawner),
            Arc::new(|pipe, model, version, token| {
                Box::pin(connect_pipe(pipe, model, version, token))
                    as Pin<Box<dyn Future<Output = Result<DynStream, String>> + Send>>
            }),
        )
    }

    /// Client with injectable spawn/connect (tests).
    pub fn with_hooks(spawner: SpawnFn, connector: ConnectFn) -> io::Result<Self> {
        Ok(Self {
            rt: Arc::new(
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?,
            ),
            inner: Arc::new(Mutex::new(Inner {
                child: None,
                stream: None,
                model: String::new(),
                token: String::new(),
            })),
            spawner,
            connector,
        })
    }

    /// Pre-spawn a model-free worker: spawn plus handshake only, no model load.
    ///
    /// Idempotent and silent: a live connection is reused, failures degrade
    /// to the lazy first-press path. The worker lives for the daemon
    /// lifetime; the first real `ensure_worker` binds it without respawning.
    pub fn warm_up(&self) {
        let mut inner = lock_client(&self.inner);
        if inner.stream.is_some() {
            return;
        }
        if inner.child.is_some() {
            self.drop_locked(&mut inner);
        }
        match self.spawn_and_connect("") {
            Ok((child, stream, token)) => {
                inner.child = Some(child);
                inner.stream = Some(stream);
                inner.model.clear();
                inner.token = token;
                debug!("voice worker warmed without model");
            }
            Err(e) => warn!("voice worker warm-up failed, lazy fallback: {e}"),
        }
    }

    /// Spawn the process and complete the pipe handshake for `model`.
    fn spawn_and_connect(&self, model: &str) -> Result<(Child, DynStream, String), String> {
        let (pipe, token) = pipe_identity();
        let mut child =
            (self.spawner)(&pipe, &token).map_err(|e| format!("voice worker spawn failed: {e}"))?;
        let version = env!("CARGO_PKG_VERSION").to_string();
        let connector = Arc::clone(&self.connector);
        let model = model.to_string();
        let stream = block_on_client(&self.rt, async {
            let mut last = String::new();
            for wait_ms in [50, 200, 800] {
                match connector(pipe.clone(), model.clone(), version.clone(), token.clone()).await {
                    Ok(s) => return Ok(s),
                    Err(e) => {
                        last = e;
                        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                    }
                }
            }
            Err(last)
        })
        .inspect_err(|_| {
            let _ = child.kill();
        })?;
        Ok((child, stream, token))
    }

    /// Ensure a healthy worker serving `model`, spawning or switching as needed.
    pub fn ensure_worker(&self, model: &str) -> Result<(), String> {
        let mut inner = lock_client(&self.inner);
        if inner.stream.is_some() && inner.model == model {
            let stream = inner
                .stream
                .as_mut()
                .ok_or("voice worker is not running".to_string())?;
            match block_on_client(
                &self.rt,
                transact(
                    stream,
                    Header::op(proto::OP_PING),
                    &[],
                    Duration::from_secs(2),
                ),
            ) {
                Ok(_) => return Ok(()),
                // A timeout means busy (decoding is synchronous), not dead:
                // keep the worker; a real death surfaces on the next op.
                Err(e) if e.contains("timed out") => return Ok(()),
                Err(_) => self.drop_locked(&mut inner),
            }
        }
        if inner.stream.is_some() && inner.model.is_empty() {
            // Warmed but unbound: bind the model over the live pipe.
            let token = inner.token.clone();
            let mut hello = Header::op(proto::OP_HELLO);
            hello.version = Some(env!("CARGO_PKG_VERSION").to_string());
            hello.model = Some(model.to_string());
            hello.token = Some(token);
            let bound = match inner.stream.as_mut() {
                Some(stream) => block_on_client(
                    &self.rt,
                    transact(stream, hello, &[], Duration::from_secs(5)),
                ),
                None => Err("voice worker is not running".to_string()),
            };
            match bound {
                Ok((resp, _)) if resp.op == proto::OP_READY => {
                    inner.model = model.to_string();
                    return Ok(());
                }
                Ok((resp, _)) => {
                    let detail = resp.message.unwrap_or_else(|| resp.op.clone());
                    self.drop_locked(&mut inner);
                    return Err(format!("voice worker handshake failed: {detail}"));
                }
                Err(_) => self.drop_locked(&mut inner),
            }
        }
        if inner.stream.is_some() || inner.child.is_some() {
            self.drop_locked(&mut inner);
        }
        let (child, stream, token) = self.spawn_and_connect(model)?;
        inner.child = Some(child);
        inner.stream = Some(stream);
        inner.model = model.to_string();
        inner.token = token;
        Ok(())
    }

    /// Stream one chunk into the worker buffer keyed by `req_id`.
    pub fn append(&self, req_id: &str, seq: u64, samples: &[f32]) -> Result<(), String> {
        let mut inner = lock_client(&self.inner);
        let stream = inner
            .stream
            .as_mut()
            .ok_or("voice worker is not running".to_string())?;
        let mut header = Header::op(proto::OP_APPEND);
        header.req_id = Some(req_id.to_string());
        header.seq = Some(seq);
        let body = proto::encode_samples(samples);
        if block_on_client(
            &self.rt,
            transact(stream, header, &body, Duration::from_secs(5)),
        )
        .is_err()
        {
            self.drop_locked(&mut inner);
            return Err("voice worker dropped the audio stream".to_string());
        }
        Ok(())
    }

    /// Finish the session keyed by `req_id` and decode everything buffered.
    pub fn transcribe(
        &self,
        req_id: &str,
        timeout: Duration,
        hotwords: Option<&str>,
    ) -> Result<WorkerTranscript, String> {
        let mut inner = lock_client(&self.inner);
        let stream = inner
            .stream
            .as_mut()
            .ok_or("voice worker is not running".to_string())?;
        let mut header = Header::op(proto::OP_TRANSCRIBE);
        header.req_id = Some(req_id.to_string());
        header.hotwords = hotwords.map(str::to_string);
        match block_on_client(&self.rt, transact(stream, header, &[], timeout)) {
            Ok((resp, _)) => Ok(WorkerTranscript {
                text: resp.text.unwrap_or_default(),
                confidence: resp.confidence.unwrap_or(0.0),
                duration_secs: resp.duration_secs.unwrap_or(0.0),
            }),
            Err(e) => {
                self.drop_locked(&mut inner);
                Err(e)
            }
        }
    }

    /// Best-effort immediate model unload; never fails the caller.
    pub fn unload_model(&self) {
        let mut inner = lock_client(&self.inner);
        if let Some(stream) = inner.stream.as_mut()
            && let Err(e) = block_on_client(
                &self.rt,
                transact(
                    stream,
                    Header::op(proto::OP_UNLOAD),
                    &[],
                    Duration::from_secs(2),
                ),
            )
        {
            // The connection stays: a slow worker is busy, not dead.
            debug!("voice worker unload unanswered: {e}");
        }
    }

    /// Best-effort full shutdown of the worker process.
    pub fn shutdown(&self) {
        let mut inner = lock_client(&self.inner);
        if let Some(stream) = inner.stream.as_mut() {
            let _ = block_on_client(
                &self.rt,
                transact(
                    stream,
                    Header::op(proto::OP_SHUTDOWN),
                    &[],
                    Duration::from_secs(2),
                ),
            );
        }
        self.drop_locked(&mut inner);
    }

    fn drop_locked(&self, inner: &mut Inner) {
        inner.stream = None;
        inner.model.clear();
        inner.token.clear();
        if let Some(mut child) = inner.child.take() {
            match child.try_wait() {
                Ok(Some(status)) => warn!("voice worker exited: {status}"),
                Ok(None) => {
                    let _ = child.kill();
                }
                Err(e) => {
                    warn!("voice worker wait failed: {e}");
                    let _ = child.kill();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// In-test peer: answers hello/ping/append/transcribe over duplex.
    async fn stub_peer(mut peer: tokio::io::DuplexStream, text: String) {
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
                let mut resp;
                match header.op.as_str() {
                    proto::OP_HELLO => {
                        resp = Header::op(proto::OP_READY);
                        resp.version = header.version.clone();
                        resp.model = header.model.clone();
                    }
                    proto::OP_PING => resp = Header::op(proto::OP_PONG),
                    proto::OP_APPEND => {
                        resp = Header::op(proto::OP_ACK);
                        resp.seq = header.seq;
                    }
                    proto::OP_TRANSCRIBE => {
                        resp = Header::op(proto::OP_RESULT);
                        if let Some(ref hw) = header.hotwords {
                            resp.text = Some(format!("{text} [{hw}]"));
                        } else {
                            resp.text = Some(text.clone());
                        }
                        resp.confidence = Some(0.9);
                        resp.duration_secs = Some(1.0);
                    }
                    proto::OP_UNLOAD | proto::OP_SHUTDOWN => resp = Header::op(proto::OP_ACK),
                    _ => {
                        resp = Header::op(proto::OP_ERROR);
                        resp.message = Some("unknown op".to_string());
                    }
                }
                resp.req_id = header.req_id.clone();
                let bytes = proto::encode_frame(&resp, &[]).expect("encode");
                if peer.write_all(&bytes).await.is_err() {
                    return;
                }
            }
        }
    }

    fn stub_client(spawns: Arc<AtomicUsize>, text: &str) -> WorkerClient {
        let text = text.to_string();
        let connector: ConnectFn = Arc::new(move |_, model, version, _| {
            let text = text.clone();
            Box::pin(async move {
                let (client, peer) = tokio::io::duplex(256 * 1024);
                tokio::spawn(stub_peer(peer, text));
                // Handshake inline so the client starts from a ready peer.
                let mut ready = Header::op(proto::OP_HELLO);
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
        let spawner: SpawnFn = Arc::new(move |_, _| {
            spawns.fetch_add(1, Ordering::SeqCst);
            // Never actually spawned in tests; the duplex peer above speaks.
            // Return a real short-lived child so kill/wait paths stay honest.
            // Windowless on Windows: hermetic tests must never flash conhost.
            #[cfg(all(unix, not(target_os = "android")))]
            let child = Command::new("true").spawn()?;
            #[cfg(target_os = "windows")]
            let child = {
                use std::os::windows::process::CommandExt;
                let mut cmd = Command::new("cmd");
                cmd.arg("/C").arg("exit").arg("0");
                cmd.creation_flags(0x0800_0000);
                cmd.spawn()?
            };
            #[cfg(target_os = "android")]
            let child = Command::new("true").spawn()?;
            Ok(child)
        });
        WorkerClient::with_hooks(spawner, connector).expect("test client")
    }

    #[test]
    fn test_client_round_trip() {
        let client = stub_client(Arc::new(AtomicUsize::new(0)), "hello stub");
        client
            .ensure_worker("parakeet-tdt-ctc-110m")
            .expect("ensure");
        client.append("r1", 0, &[0.1; 1600]).expect("append");
        let t = client
            .transcribe("r1", Duration::from_secs(5), None)
            .expect("transcribe");
        assert_eq!(t.text, "hello stub");
        assert_eq!(t.confidence, 0.9);
    }

    #[test]
    fn test_client_transcribe_with_hotwords() {
        let client = stub_client(Arc::new(AtomicUsize::new(0)), "hello");
        client
            .ensure_worker("parakeet-tdt-ctc-110m")
            .expect("ensure");
        client.append("r1", 0, &[0.1; 1600]).expect("append");
        let t = client
            .transcribe("r1", Duration::from_secs(5), Some("movies folder/Taurine"))
            .expect("transcribe");
        assert_eq!(t.text, "hello [movies folder/Taurine]");
        assert_eq!(t.confidence, 0.9);
    }

    #[test]
    fn test_concurrent_ensure_spawns_once() {
        let spawns = Arc::new(AtomicUsize::new(0));
        let client = Arc::new(stub_client(Arc::clone(&spawns), "x"));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let client = Arc::clone(&client);
            handles.push(std::thread::spawn(move || {
                client
                    .ensure_worker("parakeet-tdt-ctc-110m")
                    .expect("ensure")
            }));
        }
        for h in handles {
            h.join().expect("thread");
        }
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_voice_calls_from_async_task_do_not_panic() {
        // Reproduces the 2026-09-23 outage: pause and reload call into the
        // client from Tokio tasks, where nested block_on panics.
        let client = stub_client(Arc::new(AtomicUsize::new(0)), "x");
        client
            .ensure_worker("parakeet-tdt-ctc-110m")
            .expect("ensure");
        client.unload_model();
        client.shutdown();
        // The embedded runtime cannot be dropped inside async context;
        // production holds one static client, tests forget instead.
        std::mem::forget(client);
    }

    #[test]
    fn test_poisoned_client_recovers() {
        let client = stub_client(Arc::new(AtomicUsize::new(0)), "x");
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = client.inner.lock().unwrap();
            panic!("simulated outage panic while holding the guard");
        }));
        // A poisoned guard degrades to at most one failed request, never a
        // permanently bricked voice path.
        client
            .ensure_worker("parakeet-tdt-ctc-110m")
            .expect("ensure after poison");
    }

    #[test]
    fn test_slow_pong_keeps_worker() {
        // A worker stuck in a long decode answers ping late; that must not
        // look like death (previously this killed in-flight transcription).
        let connector: ConnectFn = Arc::new(|_, model, version, _| {
            Box::pin(async move {
                let (client, peer) = tokio::io::duplex(256 * 1024);
                tokio::spawn(async move {
                    stub_slow_peer(peer, std::time::Duration::from_secs(3)).await;
                });
                let mut ready = Header::op(proto::OP_HELLO);
                ready.version = Some(version);
                ready.model = Some(model);
                let mut client: DynStream = Box::new(client);
                // Handshake against the live peer before the slowdown starts.
                let (resp, _) = transact(&mut client, ready, &[], Duration::from_secs(5)).await?;
                if resp.op != proto::OP_READY {
                    return Err("stub handshake failed".to_string());
                }
                Ok(client)
            }) as Pin<Box<dyn Future<Output = Result<DynStream, String>> + Send>>
        });
        let spawns = Arc::new(AtomicUsize::new(0));
        let spawner: SpawnFn = Arc::new({
            let spawns = Arc::clone(&spawns);
            move |_, _| {
                spawns.fetch_add(1, Ordering::SeqCst);
                #[cfg(all(unix, not(target_os = "android")))]
                let child = Command::new("true").spawn()?;
                #[cfg(target_os = "windows")]
                let child = Command::new("cmd").arg("/C").arg("exit").arg("0").spawn()?;
                #[cfg(target_os = "android")]
                let child = Command::new("true").spawn()?;
                Ok(child)
            }
        });
        let client = WorkerClient::with_hooks(spawner, connector).expect("test client");
        client
            .ensure_worker("parakeet-tdt-ctc-110m")
            .expect("ensure");
        // Ping takes 3s against the 2s budget yet the worker is kept.
        client
            .ensure_worker("parakeet-tdt-ctc-110m")
            .expect("slow ping keeps worker");
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
    }

    /// Peer answering everything slowly (first response delayed past ping budget).
    async fn stub_slow_peer(mut peer: tokio::io::DuplexStream, delay: std::time::Duration) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = Vec::new();
        let mut delayed = false;
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
                if !delayed {
                    delayed = true;
                    tokio::time::sleep(delay).await;
                }
                let mut resp = match header.op.as_str() {
                    proto::OP_HELLO => {
                        let mut ready = Header::op(proto::OP_READY);
                        ready.version = header.version.clone();
                        ready.model = header.model.clone();
                        ready
                    }
                    proto::OP_PING => Header::op(proto::OP_PONG),
                    _ => Header::op(proto::OP_ACK),
                };
                resp.req_id = header.req_id.clone();
                let bytes = proto::encode_frame(&resp, &[]).expect("encode");
                if peer.write_all(&bytes).await.is_err() {
                    return;
                }
            }
        }
    }

    #[test]
    fn test_warm_up_then_ensure_spawns_once() {
        let spawns = Arc::new(AtomicUsize::new(0));
        let client = stub_client(Arc::clone(&spawns), "warm hello");
        client.warm_up();
        client.warm_up();
        client
            .ensure_worker("parakeet-tdt-ctc-110m")
            .expect("ensure after warm");
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        client
            .ensure_worker("parakeet-tdt-ctc-110m")
            .expect("ping reuse");
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
    }

    /// True while pid is still a live process. Unix waitpid with WNOHANG
    /// reaps a killed zombie (nonzero return) and reports 0 only when the
    /// child is still running, so an orphan reads alive and a kill reads dead.
    #[cfg(unix)]
    fn child_is_alive(pid: u32) -> bool {
        let mut status = 0;
        // SAFETY: waitpid targets only our own spawned child pid; WNOHANG
        // never blocks and reaps at most that pid, never an unrelated child.
        let r = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
        r == 0
    }

    /// True while pid is still a live process. tasklist probes the live
    /// process table, so a killed child reads dead once termination lands;
    /// the bounded poll absorbs TerminateProcess lag without flaking green.
    #[cfg(windows)]
    fn child_is_alive(pid: u32) -> bool {
        for _ in 0..20 {
            let out = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .output();
            match out {
                Ok(o) => {
                    if !String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")) {
                        return false;
                    }
                }
                Err(_) => return false,
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        true
    }

    // Live process-spawn test: spawns real long-lived sleepers plus
    // tasklist/waitpid pid polling and sleeps, so it never runs by default.
    // Opt in on a throwaway machine with
    // TAURINE_ALLOW_HOST_INPUT=1 cargo test -p taurine_daemon -- --ignored.
    #[test]
    #[ignore]
    fn test_handshake_failure_kills_spawned_child() {
        if !crate::platform::host_tests_allowed() {
            return;
        }
        // Orphan regression: a connector/handshake failure after a successful
        // spawn must kill the child. Both warm_up (silent) and ensure_worker
        // (loud) share spawn_and_connect, so both paths are covered here with
        // a long-lived sleeper whose recorded pid must read dead afterwards.
        let spawns = Arc::new(AtomicUsize::new(0));
        let pids = Arc::new(Mutex::new(Vec::<u32>::new()));
        let spawner: SpawnFn = Arc::new({
            let spawns = Arc::clone(&spawns);
            let pids = Arc::clone(&pids);
            move |_, _| {
                spawns.fetch_add(1, Ordering::SeqCst);
                #[cfg(all(unix, not(target_os = "android")))]
                let child = Command::new("sleep").arg("30").spawn()?;
                #[cfg(target_os = "windows")]
                let child = {
                    use std::os::windows::process::CommandExt;
                    let mut cmd = Command::new("ping");
                    cmd.arg("-n").arg("30").arg("127.0.0.1");
                    cmd.stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    cmd.creation_flags(0x0800_0000);
                    cmd.spawn()?
                };
                #[cfg(target_os = "android")]
                let child = Command::new("sleep").arg("30").spawn()?;
                pids.lock().unwrap().push(child.id());
                Ok(child)
            }
        });
        let failing: ConnectFn = Arc::new(|_, _, _, _| {
            Box::pin(async move { Err("stub handshake failed".to_string()) })
                as Pin<Box<dyn Future<Output = Result<DynStream, String>> + Send>>
        });
        let client = WorkerClient::with_hooks(spawner, failing).expect("test client");
        client.warm_up();
        assert!(client.ensure_worker("parakeet-tdt-ctc-110m").is_err());
        assert_eq!(spawns.load(Ordering::SeqCst), 2);
        let pids = pids.lock().unwrap().clone();
        assert_eq!(pids.len(), 2);
        for pid in pids {
            assert!(
                !child_is_alive(pid),
                "handshake failure orphaned worker pid {pid}"
            );
        }
    }

    #[test]
    fn test_dead_worker_errors_then_respawns() {
        let spawns = Arc::new(AtomicUsize::new(0));
        // First connector hands out a peer that immediately closes, so the
        // inline handshake fails exactly like a dead worker would.
        let dead: ConnectFn = Arc::new(|_, model, version, _| {
            Box::pin(async move {
                let (mut client, peer) = tokio::io::duplex(64 * 1024);
                drop(peer);
                let mut hello = Header::op(proto::OP_HELLO);
                hello.version = Some(version);
                hello.model = Some(model);
                let _: (Header, Vec<u8>) =
                    transact(&mut client, hello, &[], Duration::from_secs(2)).await?;
                Ok(Box::new(client) as DynStream)
            }) as Pin<Box<dyn Future<Output = Result<DynStream, String>> + Send>>
        });
        let spawner: SpawnFn = Arc::new({
            let spawns = Arc::clone(&spawns);
            move |_, _| {
                spawns.fetch_add(1, Ordering::SeqCst);
                #[cfg(all(unix, not(target_os = "android")))]
                let child = Command::new("true").spawn()?;
                #[cfg(target_os = "windows")]
                let child = Command::new("cmd").arg("/C").arg("exit").arg("0").spawn()?;
                #[cfg(target_os = "android")]
                let child = Command::new("true").spawn()?;
                Ok(child)
            }
        });
        let client = WorkerClient::with_hooks(spawner, dead).expect("test client");
        assert!(client.ensure_worker("parakeet-tdt-ctc-110m").is_err());
        assert!(
            client
                .transcribe("r1", Duration::from_secs(2), None)
                .is_err()
        );
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        // The next press respawns instead of reusing the corpse.
        assert!(client.ensure_worker("parakeet-tdt-ctc-110m").is_err());
        assert_eq!(spawns.load(Ordering::SeqCst), 2);
    }
}
