use rodio::{Decoder, OutputStream, Sink};
use std::io::Cursor;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

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

    // Print buffer headers once at daemon startup
    tracing::debug!(
        "Pause audio buffer first 4 bytes: {:02X} {:02X} {:02X} {:02X}",
        PAUSE_WAV[0],
        PAUSE_WAV[1],
        PAUSE_WAV[2],
        PAUSE_WAV[3]
    );
    tracing::debug!(
        "Resume audio buffer first 4 bytes: {:02X} {:02X} {:02X} {:02X}",
        RESUME_WAV[0],
        RESUME_WAV[1],
        RESUME_WAV[2],
        RESUME_WAV[3]
    );

    std::thread::spawn(move || {
        info!("Audio worker thread started (per-trigger device acquisition)");

        while let Some(is_paused) = rx.blocking_recv() {
            let data = if is_paused { PAUSE_WAV } else { RESUME_WAV };

            // Acquire a fresh OutputStream on every trigger so we always bind
            // to the *current* default device. Cached streams silently play
            // into dead endpoints on Windows (WASAPI) after a device unplug
            // because rodio/cpal does not surface device-loss errors through
            // the Sink API.
            let (_stream, handle) = match OutputStream::try_default() {
                Ok(pair) => pair,
                Err(e) => {
                    warn!("Audio playback skipped: no audio device available: {}", e);
                    continue;
                }
            };

            match load_wav_source(data) {
                Ok(decoder) => match Sink::try_new(&handle) {
                    Ok(sink) => {
                        sink.append(decoder);
                        sink.sleep_until_end();
                    }
                    Err(e) => {
                        warn!("Audio playback failed: sink error: {}", e);
                    }
                },
                Err(e) => {
                    error!("Failed to decode audio: {}", e);
                }
            }
            // _stream and handle drop here, releasing the device cleanly.
        }
    });

    tx
}
