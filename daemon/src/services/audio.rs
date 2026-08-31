use rodio::{Decoder, DeviceSinkBuilder, Player};
use std::io::Cursor;
use taurine_core::settings::{AudioTheme, get_cached_audio_theme, get_cached_audio_volume};
use tokio::sync::mpsc;
use tracing::{debug, warn};

const MINIMAL_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/minimal/pause.wav");
const MINIMAL_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/minimal/resume.wav");
const SOFT_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/soft/pause.wav");
const SOFT_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/soft/resume.wav");
const GLASS_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/glass/pause.wav");
const GLASS_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/glass/resume.wav");
const ARCADE_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/arcade/pause.wav");
const ARCADE_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/arcade/resume.wav");
const MECHANICAL_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/mechanical/pause.wav");
const MECHANICAL_RESUME: &[u8] =
    include_bytes!("../../../assets/audio/themes/mechanical/resume.wav");
const ORGANIC_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/organic/pause.wav");
const ORGANIC_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/organic/resume.wav");
const DREAMY_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/dreamy/pause.wav");
const DREAMY_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/dreamy/resume.wav");
const SCIFI_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/scifi/pause.wav");
const SCIFI_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/scifi/resume.wav");
const RUBBER_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/rubber/pause.wav");
const RUBBER_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/rubber/resume.wav");
const CINEMATIC_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/cinematic/pause.wav");
const CINEMATIC_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/cinematic/resume.wav");
const STUDIO_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/studio/pause.wav");
const STUDIO_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/studio/resume.wav");
const ZEN_PAUSE: &[u8] = include_bytes!("../../../assets/audio/themes/zen/pause.wav");
const ZEN_RESUME: &[u8] = include_bytes!("../../../assets/audio/themes/zen/resume.wav");

pub fn get_audio_data(theme: AudioTheme, is_paused: bool) -> &'static [u8] {
    match (theme, is_paused) {
        (AudioTheme::Minimal, true) => MINIMAL_PAUSE,
        (AudioTheme::Minimal, false) => MINIMAL_RESUME,
        (AudioTheme::Soft, true) => SOFT_PAUSE,
        (AudioTheme::Soft, false) => SOFT_RESUME,
        (AudioTheme::Glass, true) => GLASS_PAUSE,
        (AudioTheme::Glass, false) => GLASS_RESUME,
        (AudioTheme::Arcade, true) => ARCADE_PAUSE,
        (AudioTheme::Arcade, false) => ARCADE_RESUME,
        (AudioTheme::Mechanical, true) => MECHANICAL_PAUSE,
        (AudioTheme::Mechanical, false) => MECHANICAL_RESUME,
        (AudioTheme::Organic, true) => ORGANIC_PAUSE,
        (AudioTheme::Organic, false) => ORGANIC_RESUME,
        (AudioTheme::Dreamy, true) => DREAMY_PAUSE,
        (AudioTheme::Dreamy, false) => DREAMY_RESUME,
        (AudioTheme::Scifi, true) => SCIFI_PAUSE,
        (AudioTheme::Scifi, false) => SCIFI_RESUME,
        (AudioTheme::Rubber, true) => RUBBER_PAUSE,
        (AudioTheme::Rubber, false) => RUBBER_RESUME,
        (AudioTheme::Cinematic, true) => CINEMATIC_PAUSE,
        (AudioTheme::Cinematic, false) => CINEMATIC_RESUME,
        (AudioTheme::Studio, true) => STUDIO_PAUSE,
        (AudioTheme::Studio, false) => STUDIO_RESUME,
        (AudioTheme::Zen, true) => ZEN_PAUSE,
        (AudioTheme::Zen, false) => ZEN_RESUME,
    }
}

pub fn create_channel() -> (mpsc::Sender<bool>, mpsc::Receiver<bool>) {
    mpsc::channel::<bool>(4)
}

pub fn start_worker(mut rx: mpsc::Receiver<bool>) {
    let spawn_result = std::thread::Builder::new()
        .name("tau-audio".to_string())
        .spawn(move || {
            debug!("Audio worker thread started (embedded audio themes)");

            while let Some(is_paused) = rx.blocking_recv() {
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
                        warn!("Audio playback skipped: no audio device available: {}", e);
                        continue;
                    }
                };
                stream.log_on_drop(false);

                let cursor = Cursor::new(data);
                match Decoder::new(cursor) {
                    Ok(decoder) => {
                        let player = Player::connect_new(stream.mixer());
                        player.set_volume(volume as f32 / 100.0);
                        player.append(decoder);
                        player.sleep_until_end();
                    }
                    Err(e) => {
                        warn!("Failed to decode audio: {}", e);
                    }
                }
                // stream drops here, releasing the device cleanly.
            }
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
        }
    }
}
