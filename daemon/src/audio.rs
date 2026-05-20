use rodio::{Decoder, OutputStream, Sink};
use std::io::Cursor;
use tokio::sync::mpsc;
use tracing::{error, info};

pub fn init_audio_system() -> mpsc::Sender<bool> {
    let (tx, mut rx) = mpsc::channel::<bool>(4);

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
            let data: &[u8] = if is_paused {
                include_bytes!("../../assets/audio/pause.wav")
            } else {
                include_bytes!("../../assets/audio/resume.wav")
            };

            tracing::debug!(
                "Audio buffer first 4 bytes: {:02X} {:02X} {:02X} {:02X}",
                data[0],
                data[1],
                data[2],
                data[3]
            );

            let cursor = Cursor::new(data);
            match Decoder::new(cursor) {
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
