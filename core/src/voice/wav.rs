//! In-memory 16-bit PCM WAV audio encoder for cloud speech-to-text inference.

/// Standard RIFF header size for PCM audio.
pub const WAV_HEADER_SIZE: usize = 44;

/// Encodes raw 32-bit floating point audio samples into a standard 16-bit PCM WAV byte buffer in memory.
///
/// * `samples`: Slice of 32-bit floating point audio samples (normalized in range `[-1.0, 1.0]`).
/// * `sample_rate`: Sampling rate in Hz (e.g., 16000 for standard ASR).
/// * `channels`: Number of audio channels (e.g., 1 for mono).
///
/// Returns a byte vector containing a standard 44-byte RIFF/WAVE header followed by 16-bit little-endian PCM sample data.
pub fn encode_pcm16_wav(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let valid_channels = channels.max(1);
    let num_channels = valid_channels as u32;
    let bits_per_sample: u16 = 16;
    let bytes_per_sample = 2u32;
    let block_align = valid_channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * num_channels * bytes_per_sample;
    let data_len = (samples.len().saturating_mul(2)) as u32;
    let total_size = (WAV_HEADER_SIZE as u32).saturating_add(data_len);
    let file_size_minus_8 = 36u32.saturating_add(data_len);

    let mut buf = Vec::with_capacity(total_size as usize);

    // 0..4: RIFF chunk descriptor
    buf.extend_from_slice(b"RIFF");
    // 4..8: File size - 8
    buf.extend_from_slice(&file_size_minus_8.to_le_bytes());
    // 8..12: Format
    buf.extend_from_slice(b"WAVE");

    // 12..16: Subchunk1ID
    buf.extend_from_slice(b"fmt ");
    // 16..20: Subchunk1Size (16 for PCM)
    buf.extend_from_slice(&16u32.to_le_bytes());
    // 20..22: AudioFormat (1 = PCM)
    buf.extend_from_slice(&1u16.to_le_bytes());
    // 22..24: NumChannels
    buf.extend_from_slice(&valid_channels.to_le_bytes());
    // 24..28: SampleRate
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    // 28..32: ByteRate
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    // 32..34: BlockAlign
    buf.extend_from_slice(&block_align.to_le_bytes());
    // 34..36: BitsPerSample
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // 36..40: Subchunk2ID
    buf.extend_from_slice(b"data");
    // 40..44: Subchunk2Size
    buf.extend_from_slice(&data_len.to_le_bytes());

    // 44..: Data samples in 16-bit little-endian PCM
    for &sample in samples {
        let quantized = if sample.is_nan() {
            0i16
        } else if sample <= -1.0 {
            i16::MIN
        } else if sample >= 1.0 {
            i16::MAX
        } else if sample < 0.0 {
            (sample * 32768.0).round() as i16
        } else {
            (sample * 32767.0).round() as i16
        };
        buf.extend_from_slice(&quantized.to_le_bytes());
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_samples_produces_valid_44_byte_header() {
        let wav = encode_pcm16_wav(&[], 16000, 1);
        assert_eq!(wav.len(), 44, "Empty audio WAV must be exactly 44 bytes");

        // RIFF header
        assert_eq!(&wav[0..4], b"RIFF");
        let file_size_minus_8 = u32::from_le_bytes(wav[4..8].try_into().unwrap());
        assert_eq!(file_size_minus_8, 36);
        assert_eq!(&wav[8..12], b"WAVE");

        // fmt chunk
        assert_eq!(&wav[12..16], b"fmt ");
        let subchunk1_size = u32::from_le_bytes(wav[16..20].try_into().unwrap());
        assert_eq!(subchunk1_size, 16);
        let audio_format = u16::from_le_bytes(wav[20..22].try_into().unwrap());
        assert_eq!(audio_format, 1, "Audio format must be 1 (PCM)");
        let channels = u16::from_le_bytes(wav[22..24].try_into().unwrap());
        assert_eq!(channels, 1, "Channels must be 1 (Mono)");
        let sample_rate = u32::from_le_bytes(wav[24..28].try_into().unwrap());
        assert_eq!(sample_rate, 16000);
        let byte_rate = u32::from_le_bytes(wav[28..32].try_into().unwrap());
        assert_eq!(
            byte_rate, 32000,
            "Byte rate must be sample_rate * channels * 2"
        );
        let block_align = u16::from_le_bytes(wav[32..34].try_into().unwrap());
        assert_eq!(block_align, 2, "Block align must be channels * 2");
        let bits_per_sample = u16::from_le_bytes(wav[34..36].try_into().unwrap());
        assert_eq!(bits_per_sample, 16);

        // data chunk
        assert_eq!(&wav[36..40], b"data");
        let data_size = u32::from_le_bytes(wav[40..44].try_into().unwrap());
        assert_eq!(data_size, 0, "Empty audio must have data size 0");
    }

    #[test]
    fn test_sample_quantization_values() {
        let samples = vec![0.0, 1.0, -1.0, 0.5];
        let wav = encode_pcm16_wav(&samples, 16000, 1);
        assert_eq!(wav.len(), 44 + 4 * 2);

        let data = &wav[44..];
        let s0 = i16::from_le_bytes(data[0..2].try_into().unwrap());
        assert_eq!(s0, 0);

        let s1 = i16::from_le_bytes(data[2..4].try_into().unwrap());
        assert_eq!(s1, i16::MAX);

        let s2 = i16::from_le_bytes(data[4..6].try_into().unwrap());
        assert_eq!(s2, i16::MIN);

        let s3 = i16::from_le_bytes(data[6..8].try_into().unwrap());
        assert!((s3 - 16384).abs() <= 1);
    }

    #[test]
    fn test_clipping_and_out_of_bounds() {
        let samples = vec![2.5, -3.8];
        let wav = encode_pcm16_wav(&samples, 16000, 1);
        let data = &wav[44..];

        let s0 = i16::from_le_bytes(data[0..2].try_into().unwrap());
        assert_eq!(s0, i16::MAX, "Values > 1.0 must clamp to i16::MAX");

        let s1 = i16::from_le_bytes(data[2..4].try_into().unwrap());
        assert_eq!(s1, i16::MIN, "Values < -1.0 must clamp to i16::MIN");
    }

    #[test]
    fn test_nan_and_infinity_sanitization() {
        let samples = vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        let wav = encode_pcm16_wav(&samples, 16000, 1);
        let data = &wav[44..];

        let s0 = i16::from_le_bytes(data[0..2].try_into().unwrap());
        assert_eq!(s0, 0, "NaN must sanitize to 0");

        let s1 = i16::from_le_bytes(data[2..4].try_into().unwrap());
        assert_eq!(s1, i16::MAX, "+Inf must clamp to i16::MAX");

        let s2 = i16::from_le_bytes(data[4..6].try_into().unwrap());
        assert_eq!(s2, i16::MIN, "-Inf must clamp to i16::MIN");
    }

    #[test]
    fn test_stereo_and_different_sample_rates() {
        let samples = vec![0.1, -0.1, 0.2, -0.2];
        let wav = encode_pcm16_wav(&samples, 44100, 2);
        assert_eq!(wav.len(), 44 + 4 * 2);

        let sample_rate = u32::from_le_bytes(wav[24..28].try_into().unwrap());
        assert_eq!(sample_rate, 44100);

        let channels = u16::from_le_bytes(wav[22..24].try_into().unwrap());
        assert_eq!(channels, 2);

        let byte_rate = u32::from_le_bytes(wav[28..32].try_into().unwrap());
        assert_eq!(byte_rate, 44100 * 2 * 2);

        let block_align = u16::from_le_bytes(wav[32..34].try_into().unwrap());
        assert_eq!(block_align, 4);
    }
}
