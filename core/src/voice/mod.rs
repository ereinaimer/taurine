pub mod dictionary;
pub mod downloader;
pub mod format;
pub mod gate;
pub mod models;
pub mod phonetic;
pub mod transcriber;

pub use dictionary::VoiceDictionary;
pub use downloader::{
    ProgressCallback, StatusCallback, download_model, download_model_with_status,
};
pub use format::format_transcript;
pub use gate::{
    GateDecision, GateWitnesses, TriggerMatch, evaluate_gate, rank_voice_triggers, score_trigger,
};
pub use models::{
    AUTO_UNIFIED_MIN_BYTES, MODEL_CATALOG, ModelCatalogEntry, compute_file_sha256, get_model_entry,
    get_system_ram_gb, is_model_downloaded, list_models, models_dir, resolve_auto_model,
    resolve_configured_model, resolve_model_alias, system_total_memory_bytes, verify_file_sha256,
};
pub use phonetic::{double_metaphone, primary_key};
pub use transcriber::{Transcriber, Transcription};
