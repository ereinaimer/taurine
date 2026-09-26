use rodio::{Decoder, DeviceSinkBuilder, Player};
use std::io::Cursor;
use std::time::{Duration, Instant};
use taurine_core::settings::{AudioTheme, get_cached_audio_theme, get_cached_audio_volume};
use tokio::sync::mpsc;
use tracing::{debug, warn};

const MINIMAL_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/minimal/pause.wav");
const MINIMAL_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/minimal/resume.wav");
const ARCADE_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/arcade/pause.wav");
const ARCADE_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/arcade/resume.wav");
const MECHANICAL_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/mechanical/pause.wav");
const MECHANICAL_RESUME: &[u8] =
    include_bytes!("../../../assets/audio/themes/mechanical/resume.wav");
const ORGANIC_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/organic/pause.wav");
const ORGANIC_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/organic/resume.wav");
const SCIFI_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/scifi/pause.wav");
const SCIFI_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/scifi/resume.wav");
const RUBBER_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/rubber/pause.wav");
const RUBBER_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/rubber/resume.wav");
const ZEN_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/zen/pause.wav");
const ZEN_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/zen/resume.wav");
const MINIMAL_COPY: &[u8] = include_bytes!("../../../assets/audio/themes/minimal/copy.wav");
const MINIMAL_PASTE: &[u8] = include_bytes!("../../../assets/audio/themes/minimal/paste.wav");
const ARCADE_COPY: &[u8] = include_bytes!("../../../assets/audio/themes/arcade/copy.wav");
const ARCADE_PASTE: &[u8] = include_bytes!("../../../assets/audio/themes/arcade/paste.wav");
const MECHANICAL_COPY: &[u8] = include_bytes!("../../../assets/audio/themes/mechanical/copy.wav");
const MECHANICAL_PASTE: &[u8] = include_bytes!("../../../assets/audio/themes/mechanical/paste.wav");
const ORGANIC_COPY: &[u8] = include_bytes!("../../../assets/audio/themes/organic/copy.wav");
const ORGANIC_PASTE: &[u8] = include_bytes!("../../../assets/audio/themes/organic/paste.wav");
const SCIFI_COPY: &[u8] = include_bytes!("../../../assets/audio/themes/scifi/copy.wav");
const SCIFI_PASTE: &[u8] = include_bytes!("../../../assets/audio/themes/scifi/paste.wav");
const RUBBER_COPY: &[u8] = include_bytes!("../../../assets/audio/themes/rubber/copy.wav");
const RUBBER_PASTE: &[u8] = include_bytes!("../../../assets/audio/themes/rubber/paste.wav");
const ZEN_COPY: &[u8] = include_bytes!("../../../assets/audio/themes/zen/copy.wav");
const ZEN_PASTE: &[u8] = include_bytes!("../../../assets/audio/themes/zen/paste.wav");

pub fn get_audio_data(theme: AudioTheme, is_paused: bool) -> &'static [u8] {
    match (theme, is_paused) {
        (AudioTheme::Minimal, true) => MINIMAL_PAUSE,
        (AudioTheme::Minimal, false) => MINIMAL_RESUME,
        (AudioTheme::Arcade, true) => ARCADE_PAUSE,
        (AudioTheme::Arcade, false) => ARCADE_RESUME,
        (AudioTheme::Mechanical, true) => MECHANICAL_PAUSE,
        (AudioTheme::Mechanical, false) => MECHANICAL_RESUME,
        (AudioTheme::Organic, true) => ORGANIC_PAUSE,
        (AudioTheme::Organic, false) => ORGANIC_RESUME,
        (AudioTheme::Scifi, true) => SCIFI_PAUSE,
        (AudioTheme::Scifi, false) => SCIFI_RESUME,
        (AudioTheme::Rubber, true) => RUBBER_PAUSE,
        (AudioTheme::Rubber, false) => RUBBER_RESUME,
        (AudioTheme::Zen, true) => ZEN_PAUSE,
        (AudioTheme::Zen, false) => ZEN_RESUME,
    }
}

/// Voice dictation cues, sourced from the uisfx `copy` (start) and `paste` (stop)
/// sounds. Kept separate from the pause/resume cues so dictation feedback
/// stays distinct from the global pause toggle.
pub fn get_voice_audio_data(theme: AudioTheme, start: bool) -> &'static [u8] {
    match (theme, start) {
        (AudioTheme::Minimal, true) => MINIMAL_COPY,
        (AudioTheme::Minimal, false) => MINIMAL_PASTE,
        (AudioTheme::Arcade, true) => ARCADE_COPY,
        (AudioTheme::Arcade, false) => ARCADE_PASTE,
        (AudioTheme::Mechanical, true) => MECHANICAL_COPY,
        (AudioTheme::Mechanical, false) => MECHANICAL_PASTE,
        (AudioTheme::Organic, true) => ORGANIC_COPY,
        (AudioTheme::Organic, false) => ORGANIC_PASTE,
        (AudioTheme::Scifi, true) => SCIFI_COPY,
        (AudioTheme::Scifi, false) => SCIFI_PASTE,
        (AudioTheme::Rubber, true) => RUBBER_COPY,
        (AudioTheme::Rubber, false) => RUBBER_PASTE,
        (AudioTheme::Zen, true) => ZEN_COPY,
        (AudioTheme::Zen, false) => ZEN_PASTE,
    }
}

pub fn create_channel() -> (mpsc::Sender<bool>, mpsc::Receiver<bool>) {
    mpsc::channel::<bool>(4)
}

/// Test seam for the warm voice-cue sink. The production backend opens the
/// real default output device; tests substitute fakes. Never touches audio
/// hardware unless a real backend is used.
trait VoiceSink: Send {
    fn is_live(&self) -> bool;
    /// Cut any previous sound, then start the new cue. Returns instantly;
    /// playback lifetime belongs to the sink (persistent) or the caller
    /// (one-shot, see wait_until_end). Never blocks.
    fn play(&mut self, data: &'static [u8], volume: u32) -> Result<(), String>;
    /// Block until the current sound ends. No-op for persistent sinks whose
    /// player outlives the call; one-shot sinks need it before drop cuts audio.
    fn wait_until_end(&self) {}
}

trait VoiceBackend {
    /// Cheap OS query for the current default output identity. No stream opened.
    /// Returns None when the identity is unknowable (duplicate names, missing
    /// device, OS query failure): the caller must still play, just uncached.
    fn default_device_id(&self) -> Option<VoiceDeviceId>;
    /// Fresh open of the current default, exactly the pre-task cold path. Must
    /// succeed whenever a default device exists, even when the identity above
    /// is ambiguous.
    fn open_sink(&self) -> Option<Box<dyn VoiceSink + Send>>;
}

/// Identity of the default output: device name plus its position among
/// same-named devices in the current output enumeration (0 for the common
/// unique-name case). Unrelated reorderings leave the identity unchanged so
/// the cache survives them; duplicate names are ambiguous and never cached.
/// Any real change (unplug, default switch) mismatches and forces a reopen,
/// failing safe toward reopen and never toward stale reuse.
#[derive(Clone, Debug, PartialEq, Eq)]
struct VoiceDeviceId {
    name: String,
    index: usize,
}

struct CachedVoiceSink {
    device_id: VoiceDeviceId,
    sink: Box<dyn VoiceSink + Send>,
    last_revalidated: Instant,
}

fn play_through_sink(
    sink: &rodio::mixer::Mixer,
    data: &'static [u8],
    volume: u32,
) -> Result<(), String> {
    let cursor = Cursor::new(data);
    let decoder = Decoder::new(cursor).map_err(|e| format!("failed to decode audio: {e}"))?;
    let player = Player::connect_new(sink);
    player.set_volume(volume as f32 / 100.0);
    player.append(decoder);
    player.sleep_until_end();
    Ok(())
}

struct RodioVoiceSink {
    // Never read, never remove: dropping the device sink releases the OS
    // audio stream and silences the player. It lives exactly as long as us.
    #[allow(dead_code)]
    stream: rodio::MixerDeviceSink,
    /// Persistent player: every cue stops the previous blip before starting
    /// the new one, so rapid taps interrupt instead of blending. The player
    /// outlives each cue call, so playback needs no blocking sleep.
    player: rodio::Player,
}

impl VoiceSink for RodioVoiceSink {
    // rodio/cpal exposes no stream liveness; a dead WASAPI endpoint surfaces
    // as a playback error, which the caller turns into drop-and-reopen.
    fn is_live(&self) -> bool {
        true
    }

    fn play(&mut self, data: &'static [u8], volume: u32) -> Result<(), String> {
        // Cut any still-playing blip first: cues interrupt, never blend.
        self.player.stop();
        let cursor = Cursor::new(data);
        let decoder = Decoder::new(cursor).map_err(|e| format!("failed to decode audio: {e}"))?;
        self.player.set_volume(volume as f32 / 100.0);
        self.player.append(decoder);
        Ok(())
    }

    fn wait_until_end(&self) {
        self.player.sleep_until_end();
    }
}

struct RodioVoiceBackend;

impl VoiceBackend for RodioVoiceBackend {
    fn default_device_id(&self) -> Option<VoiceDeviceId> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let name = host.default_output_device()?.name().ok()?;
        // Position among same-named devices. Unique names always yield 0
        // regardless of where unrelated endpoints sort around them, so
        // reorderings never invalidate the cache.
        let mut found = false;
        for device in host.output_devices().ok()? {
            if device.name().ok().as_deref() != Some(name.as_str()) {
                continue;
            }
            if found {
                // Duplicate names are ambiguous: the default could be any of
                // them, so the identity is unknowable and every cue reopens.
                return None;
            }
            found = true;
        }
        if !found {
            return None;
        }
        Some(VoiceDeviceId { name, index: 0 })
    }

    fn open_sink(&self) -> Option<Box<dyn VoiceSink + Send>> {
        let mut stream = DeviceSinkBuilder::open_default_sink().ok()?;
        stream.log_on_drop(false);
        let player = rodio::Player::connect_new(stream.mixer());
        Some(Box::new(RodioVoiceSink { stream, player }))
    }
}

static VOICE_SINK_CACHE: std::sync::Mutex<Option<CachedVoiceSink>> = std::sync::Mutex::new(None);

/// Minimum gap between output-identity revalidations. Enumeration stalls
/// for seconds while Windows re-enumerates (3.5s observed per query), so
/// back-to-back cues share one verdict instead of each paying the stall.
const REVALIDATE_THROTTLE: Duration = Duration::from_secs(2);

/// Recover from poisoning: one panicking cue thread must never silence all
/// future cues forever.
fn lock_voice_cache(
    cache: &std::sync::Mutex<Option<CachedVoiceSink>>,
) -> std::sync::MutexGuard<'_, Option<CachedVoiceSink>> {
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Warm-path cue playback. The cached entry stays cached: the cue plays
/// through it FIRST with no device query in front of it (stop, decode and
/// append run under a microsecond lock, never open, sleep or query), then
/// the default identity is revalidated beside playback unless the previous
/// verdict is under 2s old. Match keeps the entry; mismatch drops only the
/// entry this cue played with, so a newer concurrent store always survives,
/// then replays once on the fresh default. Nothing audible ever waits on OS
/// enumeration. A device change mid-flight costs at most one blip through
/// the stale sink (or a silent miss when unplugged) plus its replay.
/// Silent on every failure: a missed blip at worst.
fn play_cached_voice_cue(
    backend: &dyn VoiceBackend,
    cache: &std::sync::Mutex<Option<CachedVoiceSink>>,
    data: &'static [u8],
    volume: u32,
) {
    // Play through the cached entry first: stop, decode and append run under
    // a microsecond lock, never open, sleep or query. The entry stays cached
    // throughout, so overlapping cues share one sink and interrupt each
    // other instead of opening duplicates that blend.
    let warm_attempt = {
        let mut guard = lock_voice_cache(cache);
        match guard.as_mut() {
            Some(entry) if entry.sink.is_live() => {
                let play_at = Instant::now();
                let played = entry.sink.play(data, volume).is_ok();
                let play_ms = play_at.elapsed().as_secs_f64() * 1000.0;
                let throttled = entry.last_revalidated.elapsed() < REVALIDATE_THROTTLE;
                Some((played, entry.device_id.clone(), play_ms, throttled))
            }
            _ => {
                // No entry, or a dead one: drop it here so the fresh path
                // below starts clean instead of tripping on it every cue.
                *guard = None;
                drop(guard);
                debug!("voice cue no live sink, opening fresh");
                None
            }
        }
    };
    let mut revalidated: Option<(Option<VoiceDeviceId>, f64)> = None;
    if let Some((played, played_id, play_ms, throttled)) = warm_attempt {
        if played && throttled {
            debug!("voice cue warm (throttled revalidate)");
            return;
        }
        // Revalidate beside playback, lock-free: match keeps the entry,
        // mismatch drops only the entry this cue played with so a newer
        // concurrent store always survives.
        let query_at = Instant::now();
        let current = backend.default_device_id();
        let query_ms = query_at.elapsed().as_secs_f64() * 1000.0;
        if played && current.as_ref().is_some_and(|id| id == &played_id) {
            let mut guard = lock_voice_cache(cache);
            if let Some(entry) = guard.as_mut()
                && entry.device_id == played_id
            {
                entry.last_revalidated = Instant::now();
            }
            drop(guard);
            debug!(
                "voice cue warm play_ms={:.2} revalidate_ms={:.2}",
                play_ms, query_ms
            );
            return;
        }
        let mut guard = lock_voice_cache(cache);
        if guard.as_ref().is_some_and(|e| e.device_id == played_id) {
            *guard = None;
        }
        // A concurrent cue may have parked a fresh entry while this one
        // played: prefer it over opening yet another sink when it already
        // matches the current default.
        if let Some(entry) = guard.as_mut()
            && entry.sink.is_live()
            && current.as_ref().is_some_and(|id| id == &entry.device_id)
        {
            let raced = entry.sink.play(data, volume).is_ok();
            entry.last_revalidated = Instant::now();
            drop(guard);
            debug!(
                "voice cue raced play_ms={:.2} revalidate_ms={:.2}",
                play_ms, query_ms
            );
            if raced {
                return;
            }
        } else {
            drop(guard);
        }
        debug!(
            "voice cue stale play_ms={:.2} revalidate_ms={:.2}",
            play_ms, query_ms
        );
        // Fall through and replay on the fresh default below, whether the
        // stale blip sounded (it went to the departed endpoint, so only the
        // replay is heard) or failed. The old early-return on played-after-
        // mismatch stands deleted by this change. Reuse the revalidation
        // query so replay pays no second enumeration stall.
        revalidated = Some((current, query_ms));
    }
    let (current, query_ms) = revalidated.unwrap_or_else(|| {
        let query_at = Instant::now();
        let current = backend.default_device_id();
        let query_ms = query_at.elapsed().as_secs_f64() * 1000.0;
        (current, query_ms)
    });
    let open_at = Instant::now();
    let opened = backend.open_sink();
    let open_ms = open_at.elapsed().as_secs_f64() * 1000.0;
    let Some(mut sink) = opened else {
        debug!(
            "voice cue missed open_ms={:.2} query_ms={:.2}",
            open_ms, query_ms
        );
        return;
    };
    if sink.play(data, volume).is_err() {
        debug!(
            "voice cue missed open_ms={:.2} query_ms={:.2}",
            open_ms, query_ms
        );
        return;
    }
    // Cache only when the identity is unambiguous. Ambiguous (None) still
    // sounded through the fresh sink exactly as the cold path; it is never
    // cached, and the one-shot sink blocks to completion so drop cannot cut
    // the blip. Persistent cached sinks never wait: their player outlives
    // the call.
    if let Some(device_id) = current {
        *lock_voice_cache(cache) = Some(CachedVoiceSink {
            device_id,
            sink,
            last_revalidated: Instant::now(),
        });
        debug!(
            "voice cue cold open_ms={:.2} query_ms={:.2} cached=true",
            open_ms, query_ms
        );
    } else {
        sink.wait_until_end();
        debug!(
            "voice cue cold open_ms={:.2} query_ms={:.2} cached=false",
            open_ms, query_ms
        );
    }
}

/// Open the default output sink once and park it in the cue cache so even
/// the first press of the process plays warm. No playback, no sound.
/// Best-effort and silent: a failure just leaves the first cue cold.
pub fn prewarm_voice_sink() {
    prewarm_voice_sink_with(&RodioVoiceBackend, &VOICE_SINK_CACHE);
}

/// Drop the parked cue sink without opening anything. Called on
/// device-change notifications so the next cue opens fresh on the
/// current default instead of sounding once more into the departed
/// endpoint. Never touches a live playback; the next cue re-caches.
pub fn drop_cached_voice_sink() {
    *lock_voice_cache(&VOICE_SINK_CACHE) = None;
    debug!("voice cue cache dropped on device change");
}

fn prewarm_voice_sink_with(
    backend: &dyn VoiceBackend,
    cache: &std::sync::Mutex<Option<CachedVoiceSink>>,
) {
    if lock_voice_cache(cache).is_some() {
        return;
    }
    let current = backend.default_device_id();
    if let Some(sink) = backend.open_sink()
        && let Some(device_id) = current
        && sink.is_live()
    {
        *lock_voice_cache(cache) = Some(CachedVoiceSink {
            device_id,
            sink,
            last_revalidated: Instant::now(),
        });
        debug!("voice cue sink pre-warmed");
    }
}

fn play_cue(is_paused: bool) -> Result<(), String> {
    let theme = get_cached_audio_theme();
    let volume = get_cached_audio_volume();
    let data = get_audio_data(theme, is_paused);

    // Acquire a fresh OutputStream/MixerDeviceSink on every trigger so we always bind
    // to the *current* default device. Cached streams silently play
    // into dead endpoints on Windows (WASAPI) after a device unplug
    // because rodio/cpal does not surface device-loss errors through
    // the Sink API.
    let mut stream = match DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            return Err(format!("no audio device available: {e}"));
        }
    };
    stream.log_on_drop(false);

    play_through_sink(stream.mixer(), data, volume)
}

/// Plays a non-blocking audio cue for voice dictation events.
///
/// - `start = true`: copy sound signalling microphone is open (dictation started).
/// - `start = false`: paste sound signalling dictation stopped.
///
/// The stop cue fires when the user releases the PTT hotkey or toggles
/// hands-free off, before transcription/paste completes.
pub fn play_voice_cue(start: bool) {
    // Hermetic tests never play sound on the host speakers.
    if cfg!(test) {
        let _ = start;
        return;
    }
    let volume = get_cached_audio_volume();
    if volume == 0 {
        return;
    }
    let theme = get_cached_audio_theme();
    let data = get_voice_audio_data(theme, start);

    std::thread::Builder::new()
        .name("tau-voice-cue".to_string())
        .spawn(move || {
            play_cached_voice_cue(&RodioVoiceBackend, &VOICE_SINK_CACHE, data, volume);
        })
        .ok();
}

/// Plays a non-blocking pause/resume cue for the global pause toggle.
///
/// Audio-first: called synchronously at the top of the chord arm, before the
/// toggle and before any channel send. Each press spawns its own `tau-pause-cue`
/// thread running the existing blocking `play_cue`, so rapid presses overlap
/// (each tap plays fully) instead of cutting each other.
pub fn play_pause_cue(is_paused: bool) {
    // Hermetic tests never play sound on the host speakers.
    if cfg!(test) {
        let _ = is_paused;
        return;
    }
    let volume = get_cached_audio_volume();
    if volume == 0 {
        return;
    }

    std::thread::Builder::new()
        .name("tau-pause-cue".to_string())
        .spawn(move || {
            let _ = play_cue(is_paused);
        })
        .ok();
}

/// Plays mic-open cue when PTT is pressed down or dictation begins.
pub fn play_voice_start_cue() {
    play_voice_cue(true);
}

/// Plays mic-close cue when the user stops dictation (PTT release,
/// hands-free toggle off, or Escape), before transcription runs.
pub fn play_voice_stop_cue() {
    play_voice_cue(false);
}

pub fn start_worker(mut rx: mpsc::Receiver<bool>) {
    let spawn_result = std::thread::Builder::new()
        .name("tau-audio".to_string())
        .spawn(move || {
            let _ = crate::platform::panic::catch_worker_panic(
                "tau-audio",
                std::panic::AssertUnwindSafe(move || {
                    debug!("Audio worker thread started (embedded audio themes)");
                    let mut backoff =
                        crate::platform::circuit_breaker::ExponentialBackoff::default();

                    while let Some(is_paused) = rx.blocking_recv() {
                        let res = crate::platform::panic::catch_worker_panic(
                            "tau-audio-cue",
                            std::panic::AssertUnwindSafe(|| play_cue(is_paused)),
                        );

                        match res {
                            Ok(Ok(())) => {
                                backoff.record_success();
                            }
                            Ok(Err(err)) => {
                                warn!("Audio playback skipped: {}", err);
                                backoff.wait();
                            }
                            Err(panic_err) => {
                                warn!(
                                    error = %panic_err,
                                    "Audio cue playback panicked; backing off"
                                );
                                backoff.wait();
                            }
                        }
                    }
                }),
            );
        });
    if let Err(error) = spawn_result {
        warn!(error = %error, "Failed to spawn audio worker thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    /// Rendezvous probe: lets a test observe the cue thread mid-open or
    /// mid-play so it can assert the cache lock is free at that moment.
    struct CueHooks {
        entered: crossbeam_channel::Sender<()>,
        release: crossbeam_channel::Receiver<()>,
    }

    impl CueHooks {
        fn rendezvous(&self) {
            self.entered.send(()).ok();
            let _ = self
                .release
                .recv_timeout(std::time::Duration::from_secs(10));
        }
    }

    #[derive(Default)]
    struct FakeState {
        outputs: Vec<String>,
        default: Option<usize>,
        opens: usize,
        plays: usize,
        queries: usize,
        /// Previous blips cut short by a newer cue (interrupt semantics).
        cuts: usize,
        sounding: bool,
        dead: bool,
        failing: bool,
    }

    struct FakeSink {
        state: Arc<StdMutex<FakeState>>,
        play_hooks: Option<Arc<CueHooks>>,
    }

    impl VoiceSink for FakeSink {
        fn is_live(&self) -> bool {
            !self.state.lock().unwrap().dead
        }

        fn play(&mut self, _data: &'static [u8], _volume: u32) -> Result<(), String> {
            {
                let mut state = self.state.lock().unwrap();
                if state.failing {
                    return Err("fake playback failure".to_string());
                }
                // A persistent sink cuts its previous blip when a new cue
                // arrives; the fake mirrors that contract.
                if state.sounding {
                    state.cuts += 1;
                }
                state.sounding = true;
                state.plays += 1;
            }
            if let Some(hooks) = &self.play_hooks {
                hooks.rendezvous();
            }
            Ok(())
        }
    }

    struct FakeBackend {
        state: Arc<StdMutex<FakeState>>,
        open_hooks: Option<Arc<CueHooks>>,
        play_hooks: Option<Arc<CueHooks>>,
    }

    impl VoiceBackend for FakeBackend {
        fn default_device_id(&self) -> Option<VoiceDeviceId> {
            self.state.lock().unwrap().queries += 1;
            let state = self.state.lock().unwrap();
            let index = state.default?;
            let name = state.outputs.get(index)?.clone();
            // Mirror production: duplicate names are ambiguous, identity
            // unknowable. Otherwise the identity is the name plus its
            // position among same-named devices (0 for unique names), so
            // unrelated reorderings never invalidate the cache.
            let same: Vec<usize> = state
                .outputs
                .iter()
                .enumerate()
                .filter(|(_, n)| *n == &name)
                .map(|(i, _)| i)
                .collect();
            if same.len() > 1 {
                return None;
            }
            let position = same.iter().position(|&i| i == index)?;
            Some(VoiceDeviceId {
                name,
                index: position,
            })
        }

        fn open_sink(&self) -> Option<Box<dyn VoiceSink + Send>> {
            // Fresh open of the current default regardless of ambiguity,
            // exactly the pre-task cold path. Fails only with no default.
            let open_hooks = self.open_hooks.clone();
            let play_hooks = self.play_hooks.clone();
            {
                let mut state = self.state.lock().unwrap();
                state.outputs.get(state.default?)?;
                state.opens += 1;
            }
            if let Some(hooks) = &open_hooks {
                hooks.rendezvous();
            }
            Some(Box::new(FakeSink {
                state: Arc::clone(&self.state),
                play_hooks,
            }))
        }
    }

    fn fake_setup(
        initial: &str,
    ) -> (
        FakeBackend,
        StdMutex<Option<CachedVoiceSink>>,
        Arc<StdMutex<FakeState>>,
    ) {
        fake_setup_with(&[initial], 0)
    }

    fn fake_setup_with(
        outputs: &[&str],
        default: usize,
    ) -> (
        FakeBackend,
        StdMutex<Option<CachedVoiceSink>>,
        Arc<StdMutex<FakeState>>,
    ) {
        let state = Arc::new(StdMutex::new(FakeState {
            outputs: outputs.iter().map(|s| (*s).to_string()).collect(),
            default: Some(default),
            ..Default::default()
        }));
        let backend = FakeBackend {
            state: Arc::clone(&state),
            open_hooks: None,
            play_hooks: None,
        };
        (backend, StdMutex::new(None), state)
    }

    #[test]
    fn voice_prewarm_parks_sink_so_first_cue_needs_no_open() {
        let (backend, cache, state) = fake_setup("dev-a");
        prewarm_voice_sink_with(&backend, &cache);
        assert!(cache.lock().unwrap().is_some(), "prewarm must park a sink");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 1, "first cue must reuse the prewarmed sink");
        assert_eq!(state.plays, 1, "cue must play");
    }

    #[test]
    fn voice_second_cue_cuts_first_instead_of_blending() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 1, "second cue must reuse the warm sink");
        assert_eq!(state.plays, 2, "both cues must play");
        assert_eq!(state.cuts, 1, "second cue must cut the first, not blend");
    }

    #[test]
    fn voice_mismatch_replays_once_on_new_default() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        {
            let mut state = state.lock().unwrap();
            state.outputs = vec!["dev-a".to_string(), "dev-b".to_string()];
            state.default = Some(1);
        }
        // Age the entry past the throttle so this cue revalidates instead of
        // taking the throttled warm path (simulates time passing; hermetic, no sleep).
        {
            let mut guard = cache.lock().unwrap();
            if let Some(entry) = guard.as_mut() {
                entry.last_revalidated =
                    std::time::Instant::now() - std::time::Duration::from_secs(5);
            }
        }
        // The switched cue must sound on the NEW device in the same call,
        // not play silently into the departed endpoint and heal next time.
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 2, "mismatch must reopen immediately");
        assert_eq!(
            state.plays, 3,
            "stale play plus fresh replay must both sound"
        );
        drop(state);
        assert_eq!(
            cache.lock().unwrap().as_ref().map(|e| e.device_id.clone()),
            Some(VoiceDeviceId {
                name: "dev-b".to_string(),
                index: 0
            }),
            "cache must track the new device"
        );
    }

    #[test]
    fn voice_revalidation_throttles_during_flux() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 1, "throttled cues must reuse the warm sink");
        assert_eq!(state.plays, 2, "both cues must play");
        assert_eq!(
            state.queries, 1,
            "second immediate cue must skip revalidation"
        );
    }

    #[test]
    fn voice_drop_cache_forces_fresh_open() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        assert!(cache.lock().unwrap().is_some());
        // Park a sink in the GLOBAL cache via the fake backend, then drop it.
        let sink = backend.open_sink().expect("fake open must succeed");
        *VOICE_SINK_CACHE.lock().unwrap() = Some(CachedVoiceSink {
            device_id: VoiceDeviceId {
                name: "dev-a".to_string(),
                index: 0,
            },
            last_revalidated: std::time::Instant::now(),
            sink,
        });
        drop_cached_voice_sink();
        assert!(
            VOICE_SINK_CACHE.lock().unwrap().is_none(),
            "drop must empty the cache"
        );
        drop(state);
    }

    #[test]
    fn voice_mismatch_drop_preserves_newer_concurrent_entry() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        // A concurrent cue stores a newer entry for dev-b while this cue's
        // stale drop runs: the drop must only remove the entry it played.
        let (b_backend, _, b_state) = fake_setup_with(&["dev-a", "dev-b"], 1);
        let b_sink = b_backend.open_sink().expect("concurrent open must succeed");
        let newer = CachedVoiceSink {
            device_id: VoiceDeviceId {
                name: "dev-b".to_string(),
                index: 0,
            },
            // Aged: this cue must revalidate, not throttle.
            last_revalidated: std::time::Instant::now() - std::time::Duration::from_secs(5),
            sink: b_sink,
        };
        *cache.lock().unwrap() = Some(newer);
        // Default still dev-a: the cue plays through dev-b, drops exactly the
        // entry it played, then reopens fresh on dev-a and replays there.
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        assert_eq!(
            state.lock().unwrap().opens,
            2,
            "stale cue must reopen on the current default"
        );
        assert_eq!(
            b_state.lock().unwrap().plays,
            1,
            "stale play still sounds once"
        );
        drop(state);
        assert_eq!(
            cache.lock().unwrap().as_ref().map(|e| e.device_id.clone()),
            Some(VoiceDeviceId {
                name: "dev-a".to_string(),
                index: 0,
            }),
            "cache must end on the current default"
        );
    }

    #[test]
    fn voice_second_cue_with_unchanged_default_reuses_cached_sink() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 1, "unchanged default must not reopen");
        assert_eq!(state.plays, 2, "both cues must play");
    }

    #[test]
    fn voice_changed_default_identity_reopens_and_replays() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        {
            let mut state = state.lock().unwrap();
            state.outputs = vec!["dev-a".to_string(), "dev-b".to_string()];
            state.default = Some(1);
        }
        // Age the entry past the throttle so this cue revalidates instead of
        // taking the throttled warm path (simulates time passing; hermetic, no sleep).
        {
            let mut guard = cache.lock().unwrap();
            if let Some(entry) = guard.as_mut() {
                entry.last_revalidated =
                    std::time::Instant::now() - std::time::Duration::from_secs(5);
            }
        }
        // The changed cue reopens immediately on the new default and replays.
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        {
            let state = state.lock().unwrap();
            assert_eq!(state.opens, 2, "changed default must reopen immediately");
            assert_eq!(
                state.plays, 3,
                "stale play plus fresh replay must both sound"
            );
        }
        assert_eq!(
            cache.lock().unwrap().as_ref().map(|e| e.device_id.clone()),
            Some(VoiceDeviceId {
                name: "dev-b".to_string(),
                index: 0,
            }),
            "cache must track the new device"
        );
        // Next cue stays warm against the new default.
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 2, "warm cue must not reopen");
        assert_eq!(state.plays, 4, "all cues must play");
        drop(state);
    }

    #[test]
    fn voice_unrelated_reorder_never_invalidates_cache() {
        let (backend, cache, state) = fake_setup_with(&["dev-a", "dev-b", "dev-c"], 0);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        {
            // Unrelated endpoints reorder around a unique default: the
            // identity (name plus same-name position) is unchanged.
            let mut state = state.lock().unwrap();
            state.outputs = vec![
                "dev-c".to_string(),
                "dev-b".to_string(),
                "dev-a".to_string(),
            ];
            state.default = Some(2);
        }
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 1, "reorder must not reopen");
        assert_eq!(state.plays, 3, "all cues must play");
    }

    #[test]
    fn voice_dead_sink_falls_back_to_reopen() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        state.lock().unwrap().dead = true;
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 2, "dead sink must reopen");
        assert_eq!(state.plays, 2, "cue must still play after reopen");
    }

    #[test]
    fn voice_playback_failure_is_swallowed_without_surfacing() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        state.lock().unwrap().failing = true;
        // Must not panic, and must attempt reopen exactly as today.
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 2, "failed playback must reopen");
        assert_eq!(state.plays, 1, "only the first cue played");
        drop(state);
        assert!(
            cache.lock().unwrap().is_none(),
            "poisoned sink must not stay cached"
        );
    }

    #[test]
    fn voice_duplicate_names_fall_back_to_uncached_playback_then_resume_caching() {
        let (backend, cache, state) = fake_setup_with(&["Speakers", "Speakers"], 0);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        {
            let state = state.lock().unwrap();
            assert_eq!(state.opens, 2, "ambiguous identity must reopen per cue");
            assert_eq!(state.plays, 2, "cue must still play when names collide");
        }
        assert!(
            cache.lock().unwrap().is_none(),
            "ambiguous identity must not populate the cache"
        );
        {
            let mut state = state.lock().unwrap();
            state.outputs = vec!["Speakers".to_string(), "Headphones".to_string()];
        }
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 3, "unique identity must resume warm reuse");
        assert_eq!(state.plays, 4, "all cues must play");
    }

    #[test]
    fn voice_same_name_different_index_forces_reopen() {
        let (backend, cache, state) = fake_setup_with(&["Speakers", "Speakers"], 0);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        state.lock().unwrap().default = Some(1);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(
            state.opens, 2,
            "default moving between identical names must reopen"
        );
        assert_eq!(state.plays, 2, "both cues must play");
        drop(state);
        assert!(
            cache.lock().unwrap().is_none(),
            "ambiguous identity must never populate the cache"
        );
    }

    #[test]
    fn voice_poisoned_cache_lock_still_plays_cue() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = cache.lock().unwrap();
            panic!("intentional cache-lock poison");
        }));
        assert!(cache.is_poisoned(), "setup must poison the cache lock");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 1, "poison recovery must reuse the warm sink");
        assert_eq!(state.plays, 2, "cue must still play after lock poison");
    }

    #[test]
    fn voice_cache_lock_free_during_device_open() {
        let (mut backend, cache, state) = fake_setup("dev-a");
        let (open_entered, open_entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (open_release_tx, open_release) = crossbeam_channel::bounded::<()>(1);
        backend.open_hooks = Some(Arc::new(CueHooks {
            entered: open_entered,
            release: open_release,
        }));

        std::thread::scope(|s| {
            // Cold cue blocks inside device open: the cache lock must stay
            // free so overlapping cues never serialize on a slow open.
            let handle = s.spawn(|| play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80));
            open_entered_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("cue must reach device open");
            assert!(
                cache.try_lock().is_ok(),
                "cache lock must stay free across device open"
            );
            open_release_tx.send(()).ok();
            handle.join().expect("cue thread must finish");
        });
        // Warm cue replays through the shared entry: one open total, the
        // second play cutting (not blending with) the first.
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 1, "warm cue must not reopen");
        assert_eq!(state.plays, 2, "both cues must play");
        assert_eq!(state.cuts, 1, "warm cue must cut, not blend");
    }

    #[test]
    fn test_all_embedded_audio_themes_are_valid_wav() {
        for &theme in AudioTheme::all() {
            let pause_bytes = get_audio_data(theme, true);
            assert!(
                !pause_bytes.is_empty(),
                "Theme {:?} pause cue is empty",
                theme
            );
            assert_eq!(
                &pause_bytes[0..4],
                b"RIFF",
                "Theme {:?} pause cue is not a valid WAV",
                theme
            );

            let resume_bytes = get_audio_data(theme, false);
            assert!(
                !resume_bytes.is_empty(),
                "Theme {:?} resume cue is empty",
                theme
            );
            assert_eq!(
                &resume_bytes[0..4],
                b"RIFF",
                "Theme {:?} resume cue is not a valid WAV",
                theme
            );

            let copy_bytes = get_voice_audio_data(theme, true);
            assert!(
                !copy_bytes.is_empty(),
                "Theme {:?} voice start (copy) cue is empty",
                theme
            );
            assert_eq!(
                &copy_bytes[0..4],
                b"RIFF",
                "Theme {:?} voice start (copy) cue is not a valid WAV",
                theme
            );

            let paste_bytes = get_voice_audio_data(theme, false);
            assert!(
                !paste_bytes.is_empty(),
                "Theme {:?} voice stop (paste) cue is empty",
                theme
            );
            assert_eq!(
                &paste_bytes[0..4],
                b"RIFF",
                "Theme {:?} voice stop (paste) cue is not a valid WAV",
                theme
            );
        }
    }
}
