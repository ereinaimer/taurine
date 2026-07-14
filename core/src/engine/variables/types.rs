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
    /// An image to be injected via clipboard paste (raw RGBA bytes, width, height).
    Image(Vec<u8>, u32, u32),
    /// A single key (or key combination) to simulate.
    /// The string is the raw alias (e.g. "tab", "ctrl+a", "left").
    KeyPress(String),
    /// An explicit pause in milliseconds.
    Delay(u64),
    /// A shell script to execute.
    Script(ScriptMetadata),
    /// A shell script to execute inline while preserving preceding injected text.
    InlineRun(ScriptMetadata, Vec<String>),
    /// Simulates a mouse left-click.
    MouseClick,
    /// Simulates a mouse right-click.
    MouseRClick,
    /// Simulates a mouse middle-click.
    MouseMClick,
    /// Moves mouse to absolute coordinates (x, y).
    MouseMove(u16, u16),
    /// Scrolls mouse wheel vertically by delta.
    MouseScroll(i32),
    /// Holds the mouse left button down.
    MouseHold,
    /// Releases the mouse left button.
    MouseRelease,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinalExpansion {
    /// Ordered sequence of actions the injector must execute.
    pub steps: Vec<ExpansionStep>,
    /// Whether this expansion was a mathematical calculation.
    pub is_calculation: bool,
    /// When set, the expansion contains `| ai(...)` transformer markers.
    /// The daemon must resolve these asynchronously before injecting the final output.
    pub ai_transformer_template: Option<String>,
}

impl FinalExpansion {
    /// Convenience constructor for a simple text-only expansion.
    pub fn text(text: String) -> Self {
        Self {
            steps: vec![ExpansionStep::Text(text)],
            is_calculation: false,
            ai_transformer_template: None,
        }
    }
}
