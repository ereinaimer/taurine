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

    let cursor = Cursor::new(data);
    let decoder = Decoder::new(cursor).map_err(|e| format!("failed to decode audio: {e}"))?;
    let player = Player::connect_new(stream.mixer());
    player.set_volume(volume as f32 / 100.0);
    player.append(decoder);
    player.sleep_until_end();
    Ok(())
}

/// Plays a non-blocking audio cue for voice dictation events.
///
/// - `start = true`: copy sound signalling microphone is open (dictation started).
/// - `start = false`: paste sound signalling dictation stopped.
///
/// The stop cue fires when the user releases the PTT hotkey or toggles
/// hands-free off, before transcription/paste completes.
pub fn play_voice_cue(start: bool) {
    let volume = get_cached_audio_volume();
    if volume == 0 {
        return;
    }
    let theme = get_cached_audio_theme();
    let data = get_voice_audio_data(theme, start);

    std::thread::Builder::new()
        .name("tau-voice-cue".to_string())
        .spawn(move || {
            let mut stream = match DeviceSinkBuilder::open_default_sink() {
                Ok(s) => s,
                Err(_) => return,
            };
            stream.log_on_drop(false);
            let cursor = Cursor::new(data);
            if let Ok(decoder) = Decoder::new(cursor) {
                let player = Player::connect_new(stream.mixer());
                player.set_volume(volume as f32 / 100.0);
                player.append(decoder);
                player.sleep_until_end();
            }
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
