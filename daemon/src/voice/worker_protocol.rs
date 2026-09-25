// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

//! Framing for the isolated voice worker pipe.
//!
//! Wire shape per frame: `u32le header_len` + `u32le body_len`, then the JSON
//! header, then the raw body (`f32le` 16kHz mono samples for chunks, empty
//! otherwise). Caps keep a malformed peer from ballooning allocations.

use serde::{Deserialize, Serialize};

/// Maximum JSON header bytes per frame.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Maximum body bytes per frame (a 200ms chunk is ~38KB; headroom included).
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Request ops, sent daemon to worker.
pub const OP_HELLO: &str = "hello";
/// Request ops, sent daemon to worker.
pub const OP_APPEND: &str = "append_chunk";
/// Request ops, sent daemon to worker.
pub const OP_TRANSCRIBE: &str = "transcribe";
/// Request ops, sent daemon to worker.
pub const OP_PING: &str = "ping";
/// Request ops, sent daemon to worker.
pub const OP_UNLOAD: &str = "unload";
/// Request ops, sent daemon to worker.
pub const OP_SHUTDOWN: &str = "shutdown";

/// Response ops, sent worker to daemon.
pub const OP_READY: &str = "ready";
/// Response ops, sent worker to daemon.
pub const OP_ACK: &str = "ack";
/// Response ops, sent worker to daemon.
pub const OP_RESULT: &str = "result";
/// Response ops, sent worker to daemon.
pub const OP_PONG: &str = "pong";
/// Response ops, sent worker to daemon.
pub const OP_ERROR: &str = "error";

/// Single JSON header covering requests and responses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// Operation name, one of the `OP_*` constants.
    pub op: String,
    /// Client-chosen request ID echoed in responses; absent for handshake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub req_id: Option<String>,
    /// Chunk sequence number for `append_chunk`/`ack`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// Binary version for `hello`/`ready`; stale workers fail the handshake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Per-spawn instance token binding a worker to its spawner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Resolved model ID for `hello`/`ready`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Transcribed text for `result`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Transcription confidence for `result`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Audio duration seconds for `result`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f32>,
    /// Human-readable error for `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Optional per-utterance hotwords for `transcribe`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotwords: Option<String>,
}

impl Header {
    /// Minimal header carrying only an op.
    pub fn op(op: impl Into<String>) -> Self {
        Self {
            op: op.into(),
            req_id: None,
            seq: None,
            version: None,
            token: None,
            model: None,
            text: None,
            confidence: None,
            duration_secs: None,
            message: None,
            hotwords: None,
        }
    }
}

/// Encode one frame. Fails on oversized parts, never truncates.
pub fn encode_frame(header: &Header, body: &[u8]) -> Result<Vec<u8>, String> {
    let header_bytes =
        serde_json::to_vec(header).map_err(|e| format!("voice frame header encode failed: {e}"))?;
    if header_bytes.len() > MAX_HEADER_BYTES {
        return Err(format!(
            "voice frame header too large: {} bytes",
            header_bytes.len()
        ));
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(format!("voice frame body too large: {} bytes", body.len()));
    }
    let mut out = Vec::with_capacity(8 + header_bytes.len() + body.len());
    out.extend_from_slice(&(header_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&header_bytes);
    out.extend_from_slice(body);
    Ok(out)
}

/// Decode one frame from the front of `buf`, consuming it. Returns `Ok(None)`
/// when fewer than a full frame is buffered.
pub fn decode_frame(buf: &mut Vec<u8>) -> Result<Option<(Header, Vec<u8>)>, String> {
    if buf.len() < 8 {
        return Ok(None);
    }
    let header_len = u32::from_le_bytes(
        buf[0..4]
            .try_into()
            .map_err(|_| "voice frame short read".to_string())?,
    ) as usize;
    let body_len = u32::from_le_bytes(
        buf[4..8]
            .try_into()
            .map_err(|_| "voice frame short read".to_string())?,
    ) as usize;
    if header_len > MAX_HEADER_BYTES {
        return Err(format!("voice frame header too large: {header_len} bytes"));
    }
    if body_len > MAX_BODY_BYTES {
        return Err(format!("voice frame body too large: {body_len} bytes"));
    }
    if buf.len() < 8 + header_len + body_len {
        return Ok(None);
    }
    let header: Header = serde_json::from_slice(&buf[8..8 + header_len])
        .map_err(|e| format!("voice frame header decode failed: {e}"))?;
    let body = buf[8 + header_len..8 + header_len + body_len].to_vec();
    buf.drain(..8 + header_len + body_len);
    Ok(Some((header, body)))
}

/// Encode `f32` samples as little-endian bytes for chunk bodies.
pub fn encode_samples(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Decode a chunk body back to samples. Fails on ragged tails.
pub fn decode_samples(body: &[u8]) -> Result<Vec<f32>, String> {
    if !body.len().is_multiple_of(4) {
        return Err(format!(
            "voice chunk body not f32-aligned: {} bytes",
            body.len()
        ));
    }
    let (chunks, _) = body.as_chunks::<4>();
    Ok(chunks.iter().map(|c| f32::from_le_bytes(*c)).collect())
}

/// Unix socket path for a worker pipe name. Shared with the daemon client
/// so both ends resolve the same file.
pub fn socket_path(pipe: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{pipe}.sock"))
}

/// Windows named-pipe path for a worker pipe name.
pub fn windows_pipe_path(pipe: &str) -> String {
    format!(r"\\.\pipe\{pipe}")
}

/// Accept a `hello` header iff it names this binary version and the
/// expected instance token. Pure for testability.
pub fn check_hello(header: &Header, own_version: &str, expected_token: &str) -> Result<(), String> {
    if header.op != OP_HELLO {
        return Err(format!("expected hello, got '{}'", header.op));
    }
    if header.version.as_deref() != Some(own_version) {
        return Err(format!(
            "voice worker version mismatch: worker is {own_version}, daemon sent {}",
            header.version.as_deref().unwrap_or("<none>")
        ));
    }
    if header.token.as_deref() != Some(expected_token) {
        return Err("voice worker token mismatch: stale or foreign worker".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_round_trip_with_body() {
        let mut header = Header::op(OP_APPEND);
        header.req_id = Some("req-1".to_string());
        header.seq = Some(7);
        let samples = vec![0.25f32, -0.5, 0.0];
        let body = encode_samples(&samples);

        let bytes = encode_frame(&header, &body).expect("encode");
        let mut buf = bytes;
        let (got_header, got_body) = decode_frame(&mut buf).expect("decode").expect("full frame");
        assert_eq!(got_header, header);
        assert_eq!(decode_samples(&got_body).expect("samples"), samples);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_frame_partial_buffer_waits() {
        let bytes = encode_frame(&Header::op(OP_PING), &[]).expect("encode");
        let mut buf = bytes[..5].to_vec();
        assert!(decode_frame(&mut buf).expect("decode").is_none());
    }

    #[test]
    fn test_frame_rejects_oversize_and_ragged() {
        assert!(encode_frame(&Header::op(OP_APPEND), &vec![0u8; MAX_BODY_BYTES + 1]).is_err());
        assert!(decode_samples(&[0u8; 5]).is_err());
        let mut bad_len = vec![0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0];
        assert!(decode_frame(&mut bad_len).is_err());
    }

    #[test]
    fn test_hello_version_gate() {
        let mut good = Header::op(OP_HELLO);
        good.version = Some("9.9.9".to_string());
        good.token = Some("tok".to_string());
        assert!(check_hello(&good, "9.9.9", "tok").is_ok());

        let mut stale = Header::op(OP_HELLO);
        stale.version = Some("0".to_string());
        stale.token = Some("tok".to_string());
        assert!(check_hello(&stale, "9.9.9", "tok").is_err());

        let mut foreign = Header::op(OP_HELLO);
        foreign.version = Some("9.9.9".to_string());
        foreign.token = Some("other".to_string());
        assert!(check_hello(&foreign, "9.9.9", "tok").is_err());

        assert!(check_hello(&Header::op(OP_PING), "9.9.9", "tok").is_err());
    }

    #[test]
    fn test_transcribe_header_with_hotwords_round_trip() {
        let mut header = Header::op(OP_TRANSCRIBE);
        header.req_id = Some("req-hw".to_string());
        header.hotwords = Some("movies folder/Taurine/Kubernetes".to_string());

        let bytes = encode_frame(&header, &[]).expect("encode");
        let mut buf = bytes;
        let (got_header, got_body) = decode_frame(&mut buf).expect("decode").expect("full frame");
        assert_eq!(got_header, header);
        assert_eq!(
            got_header.hotwords.as_deref(),
            Some("movies folder/Taurine/Kubernetes")
        );
        assert!(got_body.is_empty());
    }
}
