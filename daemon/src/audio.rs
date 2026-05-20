use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
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
        let mut stream_pair: Option<(OutputStream, OutputStreamHandle)> = None;
        info!("Audio worker thread started (dynamic stream acquisition)");

        while let Some(is_paused) = rx.blocking_recv() {
            let data = if is_paused { PAUSE_WAV } else { RESUME_WAV };

            for attempt in 0..2 {
                if stream_pair.is_none() {
                    match OutputStream::try_default() {
                        Ok(res) => {
                            stream_pair = Some(res);
                        }
                        Err(e) => {
                            if attempt == 0 {
                                warn!(
                                    "Audio playback skipped: failed to acquire default audio stream: {}",
                                    e
                                );
                            } else {
                                warn!(
                                    "Audio retry failed: could not re-acquire audio stream: {}",
                                    e
                                );
                            }
                            break;
                        }
                    }
                }

                let mut play_success = false;

                if let Some((_, handle)) = &stream_pair {
                    match load_wav_source(data) {
                        Ok(decoder) => match Sink::try_new(handle) {
                            Ok(sink) => {
                                sink.append(decoder);
                                sink.sleep_until_end();
                                play_success = true;
                            }
                            Err(e) => {
                                if attempt == 0 {
                                    warn!(
                                        "Audio device lost or sink error ({}). Attempting to heal.",
                                        e
                                    );
                                }
                            }
                        },
                        Err(e) => {
                            error!("Failed to decode audio: {}", e);
                            break;
                        }
                    }
                }

                if play_success {
                    break;
                } else {
                    stream_pair = None;
                }
            }
        }
    });

    tx
}
