// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

//! Isolated voice dictation worker (`taurine --voice-daemon`).
//!
//! Same binary, hidden internal role: the daemon spawns this instead of
//! loading sherpa-onnx in-process, so a recognizer crash can never take down
//! text expansion.

/// Run the voice worker until `shutdown`, disconnect, or 60s pipe idle.
pub fn run(pipe: Option<String>, version_token: Option<String>) -> taurine_core::error::Result<()> {
    super::worker::run(pipe, version_token)
}
