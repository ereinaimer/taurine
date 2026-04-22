use crate::engine::shell::ScriptMetadata;
use indexmap::IndexMap;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ArgMap {
    pub named: IndexMap<String, String>,
    pub positional: Vec<String>,
}

/// A single atomic action within an expansion sequence.
///
/// Expansions are a series of steps that the injector executes in order,
/// with implicit inter-step delays.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpansionStep {
    /// A text segment to be injected via clipboard paste.
    Text(String),
    /// A single key (or key combination) to simulate.
    /// The string is the raw alias (e.g. "tab", "ctrl+a", "left").
    KeyPress(String),
    /// An explicit pause in milliseconds.
    Delay(u64),
    /// A shell script to execute.
    Script(ScriptMetadata),
    /// A shell script to execute inline while preserving preceding injected text.
    InlineRun(ScriptMetadata),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinalExpansion {
    /// Ordered sequence of actions the injector must execute.
    pub steps: Vec<ExpansionStep>,
    /// Whether this expansion was a mathematical calculation.
    pub is_calculation: bool,
}

impl FinalExpansion {
    /// Convenience constructor for a simple text-only expansion.
    pub fn text(text: String) -> Self {
        Self {
            steps: vec![ExpansionStep::Text(text)],
            is_calculation: false,
        }
    }
}
