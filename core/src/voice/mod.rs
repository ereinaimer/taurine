pub mod cloud;
pub mod devices;
pub mod dictionary;
pub mod format;
pub mod gate;
pub mod pattern;
pub mod pattern_matcher;
pub mod phonetic;
pub mod transcriber;
pub mod wav;

pub use cloud::{DEFAULT_GROQ_API_URL, DEFAULT_GROQ_MODEL, GroqVoiceClient, build_whisper_prompt};
pub use dictionary::{VoiceDictionary, build_hotwords_payload};
pub use format::format_transcript;
pub use gate::{
    RankedVoiceMatch, TriggerMatch, rank_voice_invocations, rank_voice_triggers, score_trigger,
};
pub use pattern::{VoicePattern, VoicePatternPart};
pub use pattern_matcher::match_voice_pattern;
pub use phonetic::{double_metaphone, primary_key};
pub use transcriber::{Transcriber, Transcription};
pub use wav::encode_pcm16_wav;
