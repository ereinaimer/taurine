/// Operating modes for Taurine voice dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceMode {
    /// No active voice recording or transcription.
    Idle,
    /// Push-to-talk: records while hotkey is held down.
    PushToTalk,
    /// Hands-free: records continuously until Escape or hotkey toggle.
    HandsFree,
    /// Processing audio and transcribing / injecting.
    Processing,
}

impl VoiceMode {
    /// Return true if the voice system is actively recording or transcribing.
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Idle)
    }
}
