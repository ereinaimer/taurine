pub mod dictionary;
pub mod downloader;
pub mod format;
pub mod gate;
pub mod memory;
pub mod models;
pub mod pattern;
pub mod pattern_matcher;
pub mod phonetic;
pub mod transcriber;

pub use dictionary::{VoiceDictionary, build_hotwords_payload};
pub use downloader::{
    ProgressCallback, StatusCallback, download_model, download_model_with_status,
    ensure_unified_hotwords_asset,
};
pub use format::format_transcript;
pub use gate::{
    GateDecision, GateWitnesses, RankedVoiceMatch, TriggerMatch, evaluate_gate,
    rank_voice_invocations, rank_voice_triggers, score_trigger,
};
pub use memory::{format_mb, model_disk_size_mb, process_rss_mb};
pub use models::{
    AUTO_UNIFIED_MIN_BYTES, MODEL_CATALOG, ModelCatalogEntry, compute_file_sha256, get_model_entry,
    get_system_ram_gb, is_model_downloaded, list_models, models_dir, resolve_auto_model,
    resolve_configured_model, resolve_model_alias, system_total_memory_bytes, verify_file_sha256,
};
pub use pattern::{VoicePattern, VoicePatternPart};
pub use pattern_matcher::match_voice_pattern;
pub use phonetic::{double_metaphone, primary_key};
pub use transcriber::{Transcriber, Transcription};
