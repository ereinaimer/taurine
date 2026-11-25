mod error;
mod hotkey;
mod key;

pub use error::KeyParseError;
pub use hotkey::{
    DangerousHotkey, Hotkey, HotkeyPlatform, KeyPress, NiteReservedHotkey,
    conflicts_with_nite_global_hotkey, danger_for_platform, hotkey_matches, hotkey_strings_overlap,
    hotkeys_overlap, nite_pause_hotkey, normalize_hotkey, normalize_keypress_alias, parse_hotkey,
    parse_keypress_alias, split_tokens,
};
pub use key::{
    LogicalKey, Modifier, ModifierFamily, ModifierInsertError, ModifierSide, ModifierState,
    Modifiers, MouseButton,
};
