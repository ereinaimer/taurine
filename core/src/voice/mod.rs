pub mod dictionary;
pub mod format;
pub mod gate;
pub mod models;
pub mod transcriber;

pub use dictionary::VoiceDictionary;
pub use format::format_transcript;
pub use gate::{GateDecision, GateWitnesses, evaluate_gate};
pub use models::{
    ModelCatalogEntry, compute_file_sha256, get_model_entry, is_model_downloaded, list_models,
    models_dir, resolve_model_alias, verify_file_sha256,
};
pub use transcriber::{Transcriber, Transcription};
