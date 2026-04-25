mod error;
mod hotkey;
mod key;

pub use error::KeyParseError;
pub use hotkey::{
    DangerousHotkey, Hotkey, HotkeyPlatform, KeyPress, TaurineReservedHotkey,
    conflicts_with_taurine_global_hotkey, danger_for_platform, hotkey_matches,
    hotkey_strings_overlap, hotkeys_overlap, normalize_hotkey, normalize_keypress_alias,
    parse_hotkey, parse_keypress_alias, taurine_pause_hotkey,
};
pub use key::{LogicalKey, Modifier, ModifierFamily, ModifierSide, ModifierState, Modifiers};
