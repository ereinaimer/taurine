pub mod dictionary;
pub mod downloader;
pub mod format;
pub mod gate;
pub mod models;
pub mod transcriber;

pub use dictionary::VoiceDictionary;
pub use downloader::{
    ProgressCallback, StatusCallback, download_model, download_model_with_status,
};
pub use format::format_transcript;
pub use gate::{GateDecision, GateWitnesses, evaluate_gate};
pub use models::{
    MODEL_CATALOG, ModelCatalogEntry, UNIFIED_MIN_FREE_BYTES, available_memory_bytes,
    compute_file_sha256, get_model_entry, get_system_ram_gb, is_model_downloaded, list_models,
    models_dir, prune_deprecated_voice_models, quality_engine_allowed, resolve_model_alias,
    verify_file_sha256,
};
pub use transcriber::{Transcriber, Transcription};
