use crate::diagnostic::Diagnostic;

pub const ALL_KEY_CANDIDATES: &[&str] = &[
    "ctrl",
    "control",
    "lctrl",
    "rctrl",
    "shift",
    "lshift",
    "rshift",
    "alt",
    "opt",
    "option",
    "lalt",
    "ralt",
    "meta",
    "super",
    "cmd",
    "command",
    "win",
    "mod",
    "lmeta",
    "rmeta",
    "enter",
    "return",
    "esc",
    "escape",
    "tab",
    "space",
    "backspace",
    "delete",
    "del",
    "up",
    "down",
    "left",
    "right",
    "arrowup",
    "arrowdown",
    "arrowleft",
    "arrowright",
    "home",
    "end",
    "pgup",
    "pageup",
    "pgdown",
    "pagedown",
    "insert",
    "ins",
    "capslock",
    "numlock",
    "scrolllock",
    "printscreen",
    "prtsc",
    "pause",
    "mouse1",
    "mouse2",
    "mouse3",
    "mouse4",
    "mouse5",
    "lclick",
    "rclick",
    "mclick",
    "leftclick",
    "rightclick",
    "middleclick",
];

pub const COMMON_MODIFIERS: &[&str] = &["ctrl", "shift", "alt", "super", "meta"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyParseError {
    EmptyInput,
    MalformedSeparator,
    UnknownAlias {
        alias: String,
    },
    DuplicateModifier {
        modifier: &'static str,
    },
    ConflictingModifiers {
        first: &'static str,
        second: &'static str,
    },
    MissingBaseKey,
    ModifierOnlyHotkey,
    MultipleBaseKeys {
        first: String,
        second: String,
    },
    MissingKeypressMainKey,
    MouseButtonRequiresModifier {
        key: String,
    },
}

impl std::fmt::Display for KeyParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInput => {
                let diag = Diagnostic::problem("Hotkey input cannot be empty")
                    .help("Provide a key combination such as ctrl+shift+a or alt+space")
                    .example("ctrl+shift+a")
                    .render();
                write!(f, "{diag}")
            }
            Self::MalformedSeparator => {
                let diag = Diagnostic::problem("Hotkey contains malformed separators")
                    .help("Combine keys using single plus (+) separators")
                    .example("ctrl+shift+a")
                    .render();
                write!(f, "{diag}")
            }
            Self::UnknownAlias { alias } => {
                let diag =
                    Diagnostic::problem(format!("{alias} is not a recognized key or modifier"))
                        .suggest(alias, ALL_KEY_CANDIDATES)
                        .options("Valid modifiers", COMMON_MODIFIERS)
                        .example("super+shift+a")
                        .render();
                write!(f, "{diag}")
            }
            Self::DuplicateModifier { modifier } => {
                let diag = Diagnostic::problem(format!("Duplicate modifier {modifier}"))
                    .help("Each modifier family can only be specified once in a hotkey")
                    .render();
                write!(f, "{diag}")
            }
            Self::ConflictingModifiers { first, second } => {
                let diag = Diagnostic::problem(format!(
                    "Conflicting modifiers {first} and {second}"
                ))
                .help("Cannot combine conflicting sides or states of the same modifier family")
                .render();
                write!(f, "{diag}")
            }
            Self::MissingBaseKey => {
                let diag = Diagnostic::problem("Hotkey is missing a base key")
                    .help("Hotkeys require a base key (letter, number, or function key) alongside modifiers")
                    .example("ctrl+alt+t")
                    .render();
                write!(f, "{diag}")
            }
            Self::ModifierOnlyHotkey => {
                let diag = Diagnostic::problem(
                    "Hotkey must include a base key in addition to modifiers",
                )
                .help("Modifiers like ctrl, shift, or alt cannot trigger actions on their own")
                .example("ctrl+shift+a")
                .render();
                write!(f, "{diag}")
            }
            Self::MultipleBaseKeys { first, second } => {
                let diag = Diagnostic::problem(format!(
                    "Hotkey contains multiple base keys: {first} and {second}"
                ))
                .help("Hotkeys can only have one base key")
                .example(format!("ctrl+{first}"))
                .render();
                write!(f, "{diag}")
            }
            Self::MissingKeypressMainKey => {
                let diag = Diagnostic::problem("Keypress alias is missing a main key")
                    .help("Specify the main key to trigger")
                    .render();
                write!(f, "{diag}")
            }
            Self::MouseButtonRequiresModifier { key } => {
                let diag = Diagnostic::problem(format!(
                    "Mouse button {key} requires at least one modifier key"
                ))
                .help(
                    "To prevent capturing regular mouse clicks, pair mouse buttons with a modifier",
                )
                .example(format!("ralt+{key}"))
                .example(format!("ctrl+{key}"))
                .render();
                write!(f, "{diag}")
            }
        }
    }
}

impl std::error::Error for KeyParseError {}
