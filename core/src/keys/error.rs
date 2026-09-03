use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyParseError {
    #[error("hotkey input cannot be empty")]
    EmptyInput,
    #[error("hotkey contains malformed separators")]
    MalformedSeparator,
    #[error("unknown key or modifier alias '{alias}'")]
    UnknownAlias { alias: String },
    #[error("duplicate modifier '{modifier}'")]
    DuplicateModifier { modifier: &'static str },
    #[error("conflicting modifier requirements '{first}' and '{second}'")]
    ConflictingModifiers {
        first: &'static str,
        second: &'static str,
    },
    #[error("hotkey is missing a base key")]
    MissingBaseKey,
    #[error("hotkey must include exactly one base key")]
    ModifierOnlyHotkey,
    #[error("hotkey contains multiple base keys: '{first}' and '{second}'")]
    MultipleBaseKeys { first: String, second: String },
    #[error("keypress alias is missing a main key")]
    MissingKeypressMainKey,
    #[error(
        "mouse button '{key}' requires at least one modifier key (e.g. 'ralt+{key}', 'ctrl+{key}')"
    )]
    MouseButtonRequiresModifier { key: String },
}
