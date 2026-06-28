use rodio::{Decoder, DeviceSinkBuilder, Player};
use std::io::Cursor;
use tokio::sync::mpsc;
use tracing::{info, warn};

const PAUSE_WAV: &[u8] = include_bytes!("../../assets/audio/pause.wav");
const RESUME_WAV: &[u8] = include_bytes!("../../assets/audio/resume.wav");

/// Encapsulates the explicit WAV decoder initialization from a static buffer source.
fn load_wav_source(
    data: &'static [u8],
) -> Result<Decoder<Cursor<&'static [u8]>>, rodio::decoder::DecoderError> {
    let cursor = Cursor::new(data);
    Decoder::new(cursor)
}

pub fn init_audio_system() -> mpsc::Sender<bool> {
    let (tx, mut rx) = mpsc::channel::<bool>(4);

    std::thread::spawn(move || {
        info!("Audio worker thread started (per-trigger device acquisition)");

        while let Some(is_paused) = rx.blocking_recv() {
            let data = if is_paused { PAUSE_WAV } else { RESUME_WAV };

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

            match load_wav_source(data) {
                Ok(decoder) => {
                    let player = Player::connect_new(stream.mixer());
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

    tx
}
