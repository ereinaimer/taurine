use super::error::KeyParseError;
use super::key::{LogicalKey, Modifier, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub modifiers: Modifiers,
    pub key: LogicalKey,
}

impl KeyPress {
    pub fn canonical_string(self) -> String {
        canonical_string(self.modifiers, self.key)
    }
}

pub type Hotkey = KeyPress;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyPlatform {
    Windows,
    Linux,
    Mac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerousHotkey {
    Copy,
    Paste,
    Cut,
    Undo,
    SelectAll,
    Save,
    AppSwitcher,
    CloseWindow,
    SecureAttention,
    QuitApplication,
}

impl DangerousHotkey {
    pub const fn description(self) -> &'static str {
        match self {
            Self::Copy => "copy shortcut",
            Self::Paste => "paste shortcut",
            Self::Cut => "cut shortcut",
            Self::Undo => "undo shortcut",
            Self::SelectAll => "select-all shortcut",
            Self::Save => "save shortcut",
            Self::AppSwitcher => "application switcher",
            Self::CloseWindow => "close-window shortcut",
            Self::SecureAttention => "secure attention shortcut",
            Self::QuitApplication => "quit-application shortcut",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaurineReservedHotkey {
    PauseToggle,
}

impl TaurineReservedHotkey {
    pub const fn description(self) -> &'static str {
        match self {
            Self::PauseToggle => "Taurine pause hotkey",
        }
    }
}

pub fn parse_hotkey(input: &str) -> Result<Hotkey, KeyParseError> {
    let tokens = split_tokens(input)?;
    let mut modifiers = Modifiers::new();
    let mut base_key: Option<LogicalKey> = None;
    let mut base_token: Option<String> = None;

    for token in tokens {
        if let Some(modifier) = Modifier::from_alias(&token) {
            if !modifiers.insert(modifier) {
                return Err(KeyParseError::DuplicateModifier {
                    modifier: modifier.canonical_name(),
                });
            }
            continue;
        }

        let Some(key) = LogicalKey::from_alias(&token) else {
            return Err(KeyParseError::UnknownAlias { alias: token });
        };

        if let Some(existing_token) = base_token {
            return Err(KeyParseError::MultipleBaseKeys {
                first: existing_token,
                second: token,
            });
        }

        base_token = Some(token);
        base_key = Some(key);
    }

    let Some(key) = base_key else {
        return if modifiers.is_empty() {
            Err(KeyParseError::MissingBaseKey)
        } else {
            Err(KeyParseError::ModifierOnlyHotkey)
        };
    };

    if key.is_modifier_key().is_some() {
        return Err(KeyParseError::ModifierOnlyHotkey);
    }

    Ok(Hotkey { modifiers, key })
}

pub fn normalize_hotkey(input: &str) -> Result<String, KeyParseError> {
    Ok(parse_hotkey(input)?.canonical_string())
}

pub fn parse_keypress_alias(input: &str) -> Result<KeyPress, KeyParseError> {
    let tokens = split_tokens(input)?;
    let (main_key_alias, modifier_aliases) = tokens
        .split_last()
        .ok_or(KeyParseError::MissingKeypressMainKey)?;

    let mut modifiers = Modifiers::new();
    for alias in modifier_aliases {
        let Some(modifier) = Modifier::from_alias(alias) else {
            return Err(KeyParseError::UnknownAlias {
                alias: alias.clone(),
            });
        };
        if !modifiers.insert(modifier) {
            return Err(KeyParseError::DuplicateModifier {
                modifier: modifier.canonical_name(),
            });
        }
    }

    let Some(key) = LogicalKey::from_alias(main_key_alias) else {
        return Err(KeyParseError::UnknownAlias {
            alias: main_key_alias.clone(),
        });
    };

    Ok(KeyPress { modifiers, key })
}

pub fn normalize_keypress_alias(input: &str) -> Result<String, KeyParseError> {
    Ok(parse_keypress_alias(input)?.canonical_string())
}

pub fn danger_for_platform(hotkey: Hotkey, platform: HotkeyPlatform) -> Option<DangerousHotkey> {
    match platform {
        HotkeyPlatform::Windows | HotkeyPlatform::Linux => windows_linux_danger(hotkey),
        HotkeyPlatform::Mac => mac_danger(hotkey),
    }
}

pub fn conflicts_with_taurine_global_hotkey(hotkey: Hotkey) -> Option<TaurineReservedHotkey> {
    if hotkey == taurine_pause_hotkey() {
        Some(TaurineReservedHotkey::PauseToggle)
    } else {
        None
    }
}

pub fn taurine_pause_hotkey() -> Hotkey {
    let mut modifiers = Modifiers::new();
    let _ = modifiers.insert(Modifier::Alt);
    Hotkey {
        modifiers,
        key: LogicalKey::Backquote,
    }
}

fn split_tokens(input: &str) -> Result<Vec<String>, KeyParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(KeyParseError::EmptyInput);
    }

    trimmed
        .split('+')
        .map(|segment| {
            let token = segment.trim().to_ascii_lowercase();
            if token.is_empty() {
                Err(KeyParseError::MalformedSeparator)
            } else {
                Ok(token)
            }
        })
        .collect()
}

fn canonical_string(modifiers: Modifiers, key: LogicalKey) -> String {
    let mut parts: Vec<String> = modifiers
        .ordered()
        .map(|modifier| modifier.canonical_name().to_string())
        .collect();
    parts.push(key.canonical_name().into_owned());
    parts.join("+")
}

fn windows_linux_danger(hotkey: Hotkey) -> Option<DangerousHotkey> {
    if is_ctrl_hotkey(hotkey, LogicalKey::Letter('c')) {
        return Some(DangerousHotkey::Copy);
    }
    if is_ctrl_hotkey(hotkey, LogicalKey::Letter('v')) {
        return Some(DangerousHotkey::Paste);
    }
    if is_ctrl_hotkey(hotkey, LogicalKey::Letter('x')) {
        return Some(DangerousHotkey::Cut);
    }
    if is_ctrl_hotkey(hotkey, LogicalKey::Letter('z')) {
        return Some(DangerousHotkey::Undo);
    }
    if is_ctrl_hotkey(hotkey, LogicalKey::Letter('a')) {
        return Some(DangerousHotkey::SelectAll);
    }
    if is_ctrl_hotkey(hotkey, LogicalKey::Letter('s')) {
        return Some(DangerousHotkey::Save);
    }
    if hotkey.modifiers == modifiers_with(&[Modifier::Alt]) && hotkey.key == LogicalKey::Tab {
        return Some(DangerousHotkey::AppSwitcher);
    }
    if hotkey.modifiers == modifiers_with(&[Modifier::Alt]) && hotkey.key == LogicalKey::Function(4)
    {
        return Some(DangerousHotkey::CloseWindow);
    }
    if hotkey.modifiers == modifiers_with(&[Modifier::Ctrl, Modifier::Alt])
        && hotkey.key == LogicalKey::Delete
    {
        return Some(DangerousHotkey::SecureAttention);
    }
    None
}

fn mac_danger(hotkey: Hotkey) -> Option<DangerousHotkey> {
    if is_meta_hotkey(hotkey, LogicalKey::Letter('c')) {
        return Some(DangerousHotkey::Copy);
    }
    if is_meta_hotkey(hotkey, LogicalKey::Letter('v')) {
        return Some(DangerousHotkey::Paste);
    }
    if is_meta_hotkey(hotkey, LogicalKey::Letter('x')) {
        return Some(DangerousHotkey::Cut);
    }
    if is_meta_hotkey(hotkey, LogicalKey::Letter('z')) {
        return Some(DangerousHotkey::Undo);
    }
    if is_meta_hotkey(hotkey, LogicalKey::Letter('a')) {
        return Some(DangerousHotkey::SelectAll);
    }
    if is_meta_hotkey(hotkey, LogicalKey::Letter('s')) {
        return Some(DangerousHotkey::Save);
    }
    if hotkey.modifiers == modifiers_with(&[Modifier::Meta]) && hotkey.key == LogicalKey::Tab {
        return Some(DangerousHotkey::AppSwitcher);
    }
    if hotkey.modifiers == modifiers_with(&[Modifier::Meta])
        && hotkey.key == LogicalKey::Letter('q')
    {
        return Some(DangerousHotkey::QuitApplication);
    }
    None
}

fn modifiers_with(modifiers: &[Modifier]) -> Modifiers {
    let mut bitset = Modifiers::new();
    for modifier in modifiers {
        let _ = bitset.insert(*modifier);
    }
    bitset
}

fn is_ctrl_hotkey(hotkey: Hotkey, key: LogicalKey) -> bool {
    hotkey.modifiers == modifiers_with(&[Modifier::Ctrl]) && hotkey.key == key
}

fn is_meta_hotkey(hotkey: Hotkey, key: LogicalKey) -> bool {
    hotkey.modifiers == modifiers_with(&[Modifier::Meta]) && hotkey.key == key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_ctrl_shift_g_variants() {
        assert_eq!(
            normalize_hotkey("Ctrl + ShIFT + G").unwrap(),
            "ctrl+shift+g"
        );
        assert_eq!(normalize_hotkey("Shift+Ctrl+G").unwrap(), "ctrl+shift+g");
        assert_eq!(
            normalize_hotkey("control + shift + g").unwrap(),
            "ctrl+shift+g"
        );
    }

    #[test]
    fn normalizes_modifier_order_and_key_aliases() {
        assert_eq!(
            normalize_hotkey("meta+alt+ctrl+shift+g").unwrap(),
            "ctrl+shift+alt+meta+g"
        );
        assert_eq!(normalize_hotkey("Command + Return").unwrap(), "meta+enter");
        assert_eq!(normalize_hotkey("ArrowUp").unwrap(), "up");
        assert_eq!(normalize_hotkey("DEL").unwrap(), "delete");
    }

    #[test]
    fn rejects_duplicate_modifiers() {
        assert_eq!(
            parse_hotkey("ctrl+control+k").unwrap_err(),
            KeyParseError::DuplicateModifier { modifier: "ctrl" }
        );
    }

    #[test]
    fn rejects_modifier_only_hotkeys() {
        assert_eq!(
            parse_hotkey("ctrl+shift").unwrap_err(),
            KeyParseError::ModifierOnlyHotkey
        );
    }

    #[test]
    fn rejects_multiple_base_keys() {
        assert_eq!(
            parse_hotkey("ctrl+k+p").unwrap_err(),
            KeyParseError::MultipleBaseKeys {
                first: "k".to_string(),
                second: "p".to_string(),
            }
        );
    }

    #[test]
    fn rejects_unknown_aliases_and_malformed_separators() {
        assert_eq!(
            parse_hotkey("ctrl+hyper").unwrap_err(),
            KeyParseError::UnknownAlias {
                alias: "hyper".to_string(),
            }
        );
        assert_eq!(
            parse_hotkey("ctrl++g").unwrap_err(),
            KeyParseError::MalformedSeparator
        );
        assert_eq!(parse_hotkey("   ").unwrap_err(), KeyParseError::EmptyInput);
    }

    #[test]
    fn keeps_top_row_and_numpad_digits_distinct() {
        assert_eq!(normalize_hotkey("ctrl+1").unwrap(), "ctrl+1");
        assert_eq!(normalize_hotkey("ctrl+num1").unwrap(), "ctrl+num1");
        assert_ne!(
            parse_hotkey("ctrl+1").unwrap(),
            parse_hotkey("ctrl+num1").unwrap()
        );
    }

    #[test]
    fn flags_platform_specific_dangerous_hotkeys() {
        let windows_copy = parse_hotkey("ctrl+c").unwrap();
        let windows_switch = parse_hotkey("alt+tab").unwrap();
        let mac_copy = parse_hotkey("cmd+c").unwrap();
        let mac_quit = parse_hotkey("meta+q").unwrap();

        assert_eq!(
            danger_for_platform(windows_copy, HotkeyPlatform::Windows),
            Some(DangerousHotkey::Copy)
        );
        assert_eq!(
            danger_for_platform(windows_switch, HotkeyPlatform::Linux),
            Some(DangerousHotkey::AppSwitcher)
        );
        assert_eq!(
            danger_for_platform(mac_copy, HotkeyPlatform::Mac),
            Some(DangerousHotkey::Copy)
        );
        assert_eq!(
            danger_for_platform(mac_quit, HotkeyPlatform::Mac),
            Some(DangerousHotkey::QuitApplication)
        );
        assert_eq!(danger_for_platform(mac_copy, HotkeyPlatform::Windows), None);
    }

    #[test]
    fn detects_taurine_pause_hotkey_conflicts_only_for_alt_backtick() {
        let pause = parse_hotkey("alt+`").unwrap();
        let alt_enter = parse_hotkey("alt+enter").unwrap();
        let alt_escape = parse_hotkey("alt+esc").unwrap();

        assert_eq!(
            conflicts_with_taurine_global_hotkey(pause),
            Some(TaurineReservedHotkey::PauseToggle)
        );
        assert_eq!(conflicts_with_taurine_global_hotkey(alt_enter), None);
        assert_eq!(conflicts_with_taurine_global_hotkey(alt_escape), None);
    }

    #[test]
    fn keypress_alias_parser_keeps_modifier_main_key_support() {
        assert_eq!(normalize_keypress_alias("Shift+Tab").unwrap(), "shift+tab");
        assert_eq!(
            normalize_keypress_alias("ctrl+shift").unwrap(),
            "ctrl+shift"
        );
        assert_eq!(normalize_keypress_alias("mod").unwrap(), "meta");
    }
}
