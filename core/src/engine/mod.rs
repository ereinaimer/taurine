pub mod ai_session;
pub mod buffer;
pub mod catalog;
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
pub use catalog::ExpansionCatalog;
pub use evaluator::{EngineEvent, Evaluator, ExpansionResult};
pub use state::EngineState;
