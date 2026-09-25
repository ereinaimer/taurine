use rodio::{Decoder, DeviceSinkBuilder, Player};
use std::io::Cursor;
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
    fn play(&mut self, data: &'static [u8], volume: u32) -> Result<(), String>;
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

/// Identity of the default output: device name plus its index in the current
/// output enumeration. Any topology change (unplug, reorder, swap between two
/// identical-named devices) changes the identity and forces a reopen, failing
/// safe toward reopen and never toward stale reuse.
#[derive(Clone, Debug, PartialEq, Eq)]
struct VoiceDeviceId {
    name: String,
    index: usize,
}

struct CachedVoiceSink {
    device_id: VoiceDeviceId,
    sink: Box<dyn VoiceSink + Send>,
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
    stream: rodio::MixerDeviceSink,
}

impl VoiceSink for RodioVoiceSink {
    // rodio/cpal exposes no stream liveness; a dead WASAPI endpoint surfaces
    // as a playback error, which the caller turns into drop-and-reopen.
    fn is_live(&self) -> bool {
        true
    }

    fn play(&mut self, data: &'static [u8], volume: u32) -> Result<(), String> {
        play_through_sink(self.stream.mixer(), data, volume)
    }
}

struct RodioVoiceBackend;

impl VoiceBackend for RodioVoiceBackend {
    fn default_device_id(&self) -> Option<VoiceDeviceId> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let name = host.default_output_device()?.name().ok()?;
        let mut seen: Option<usize> = None;
        for (index, device) in host.output_devices().ok()?.enumerate() {
            if device.name().ok().as_deref() != Some(name.as_str()) {
                continue;
            }
            if seen.is_some() {
                // Duplicate names are ambiguous: the default could be either
                // device, so the identity is unknowable and every cue reopens.
                return None;
            }
            seen = Some(index);
        }
        Some(VoiceDeviceId { name, index: seen? })
    }

    fn open_sink(&self) -> Option<Box<dyn VoiceSink + Send>> {
        let mut stream = DeviceSinkBuilder::open_default_sink().ok()?;
        stream.log_on_drop(false);
        Some(Box::new(RodioVoiceSink { stream }))
    }
}

static VOICE_SINK_CACHE: std::sync::Mutex<Option<CachedVoiceSink>> = std::sync::Mutex::new(None);

/// Recover from poisoning: one panicking cue thread must never silence all
/// future cues forever.
fn lock_voice_cache(
    cache: &std::sync::Mutex<Option<CachedVoiceSink>>,
) -> std::sync::MutexGuard<'_, Option<CachedVoiceSink>> {
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Warm-path cue playback. Reuses the cached sink while the default device is
/// unchanged and the sink is live; otherwise drops the cache and reopens the
/// current default exactly as the cold path. Silent on every failure: a missed
/// blip at worst, never routed to a superseded device.
fn play_cached_voice_cue(
    backend: &dyn VoiceBackend,
    cache: &std::sync::Mutex<Option<CachedVoiceSink>>,
    data: &'static [u8],
    volume: u32,
) {
    // Cheap identity query first, outside the lock: no stream opened.
    let current = backend.default_device_id();
    // Take (not borrow) under a short lock. The lock is never held across
    // device open, decode, play, or sleep, so overlapping cues never
    // serialize on it.
    // honey: concurrent cues may double-reopen; last store wins and both
    // entries are valid, so add a generation check only if cues overlap often.
    let cached = lock_voice_cache(cache).take();
    // Mismatch, dead sink, or playback error drops the entry and reopens below.
    if let Some(mut entry) = cached
        && current.as_ref().is_some_and(|id| id == &entry.device_id)
        && entry.sink.is_live()
        && entry.sink.play(data, volume).is_ok()
    {
        *lock_voice_cache(cache) = Some(entry);
        return;
    }
    if let Some(mut sink) = backend.open_sink()
        && sink.play(data, volume).is_ok()
    {
        // Cache only when the identity is unambiguous. Ambiguous (None) still
        // played through the fresh sink exactly as the pre-task cold path, but
        // stays uncached so the next cue reopens fresh too.
        if let Some(device_id) = current {
            *lock_voice_cache(cache) = Some(CachedVoiceSink { device_id, sink });
        }
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
            let state = self.state.lock().unwrap();
            let index = state.default?;
            let name = state.outputs.get(index)?.clone();
            // Mirror production: duplicate names are ambiguous, identity unknowable.
            if state.outputs.iter().filter(|n| *n == &name).count() > 1 {
                return None;
            }
            Some(VoiceDeviceId { name, index })
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
    fn voice_second_cue_with_unchanged_default_reuses_cached_sink() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 1, "unchanged default must not reopen");
        assert_eq!(state.plays, 2, "both cues must play");
    }

    #[test]
    fn voice_changed_default_identity_forces_reopen() {
        let (backend, cache, state) = fake_setup("dev-a");
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        {
            let mut state = state.lock().unwrap();
            state.outputs = vec!["dev-a".to_string(), "dev-b".to_string()];
            state.default = Some(1);
        }
        play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80);
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 2, "device change must reopen");
        assert_eq!(state.plays, 2, "both cues must play");
        drop(state);
        assert_eq!(
            cache.lock().unwrap().as_ref().map(|e| e.device_id.clone()),
            Some(VoiceDeviceId {
                name: "dev-b".to_string(),
                index: 1,
            }),
            "cache must track the new device"
        );
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
    fn voice_cache_lock_not_held_during_open_or_playback() {
        let (mut backend, cache, state) = fake_setup("dev-a");
        let (open_entered, open_entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (open_release_tx, open_release) = crossbeam_channel::bounded::<()>(1);
        let (play_entered, play_entered_rx) = crossbeam_channel::bounded::<()>(1);
        let (play_release_tx, play_release) = crossbeam_channel::bounded::<()>(1);
        backend.open_hooks = Some(Arc::new(CueHooks {
            entered: open_entered,
            release: open_release,
        }));
        backend.play_hooks = Some(Arc::new(CueHooks {
            entered: play_entered,
            release: play_release,
        }));

        let probe = |rx: &crossbeam_channel::Receiver<()>, what: &str| {
            rx.recv_timeout(std::time::Duration::from_secs(5))
                .expect(what);
            cache.try_lock().is_ok()
        };
        let mut unlocked = Vec::new();
        std::thread::scope(|s| {
            // Cold cue: device open rendezvous, then playback rendezvous.
            let handle = s.spawn(|| play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80));
            unlocked.push(probe(&open_entered_rx, "cue must reach device open"));
            open_release_tx.send(()).ok();
            unlocked.push(probe(&play_entered_rx, "cue must reach playback"));
            play_release_tx.send(()).ok();
            handle.join().expect("cue thread must finish");

            // Warm cue: cached sink replays, so only a playback rendezvous.
            let handle = s.spawn(|| play_cached_voice_cue(&backend, &cache, MINIMAL_COPY, 80));
            unlocked.push(probe(&play_entered_rx, "warm cue must reach playback"));
            play_release_tx.send(()).ok();
            handle.join().expect("cue thread must finish");
        });
        assert_eq!(
            unlocked,
            vec![true, true, true],
            "cache lock must never be held across device open or playback"
        );
        let state = state.lock().unwrap();
        assert_eq!(state.opens, 1, "warm cue must not reopen");
        assert_eq!(state.plays, 2, "both cues must play");
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
