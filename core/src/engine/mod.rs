pub mod ai_session;
pub mod buffer;
pub mod catalog;
pub mod conversion;
pub mod emoji;
pub mod evaluator;
pub mod math;
pub mod shell;
pub mod source;
pub mod state;
pub mod variables;

pub use shell::{ScriptBehavior, ScriptInterpreter, ScriptMetadata};
pub use source::SnippetSource;

pub use ai_session::{EngineMode, InlineAiSession};
pub use buffer::FastBuffer;
pub use catalog::{ActiveWindowInfo, ExpansionCatalog, HotkeyCatalog, RegexCatalog};
pub use evaluator::{
    CompletionRewrite, EngineEvent, Evaluator, ExpansionFollowUp, ExpansionResult,
};
pub use state::EngineState;
