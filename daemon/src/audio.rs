use rodio::{Decoder, OutputStream, Sink};
use std::io::Cursor;
use tokio::sync::mpsc;
use tracing::{error, info};

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
        let (_stream, stream_handle) = match OutputStream::try_default() {
            Ok(res) => res,
            Err(e) => {
                error!("Failed to initialize audio stream: {}", e);
                return;
            }
        };

        info!("Audio system initialized");

        while let Some(is_paused) = rx.blocking_recv() {
            let data = if is_paused { PAUSE_WAV } else { RESUME_WAV };

            match load_wav_source(data) {
                Ok(decoder) => match Sink::try_new(&stream_handle) {
                    Ok(sink) => {
                        sink.append(decoder);
                        sink.sleep_until_end();
                    }
                    Err(e) => error!("Failed to create audio sink: {}", e),
                },
                Err(e) => error!("Failed to decode audio: {}", e),
            }
        }
    });

    tx
}
