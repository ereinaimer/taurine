use super::error::KeyParseError;
use super::key::{LogicalKey, Modifier, ModifierInsertError, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub modifiers: Modifiers,
    pub key: LogicalKey,
}

impl KeyPress {
    pub fn canonical_string(self) -> String {
        canonical_string(self.modifiers, self.key)
    }

    pub const fn logical_key(self) -> LogicalKey {
        self.key
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
            insert_modifier(&mut modifiers, modifier)?;
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

    if key.is_mouse_button().is_some() && modifiers.is_empty() {
        return Err(KeyParseError::MouseButtonRequiresModifier {
            key: base_token.unwrap_or_else(|| key.canonical_name().into_owned()),
        });
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
        insert_modifier(&mut modifiers, modifier)?;
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

pub fn hotkey_matches(required: Hotkey, active: Hotkey) -> bool {
    required.key == active.key && required.modifiers.matches_active(active.modifiers)
}

pub fn hotkeys_overlap(left: Hotkey, right: Hotkey) -> bool {
    left.key == right.key && left.modifiers.overlaps(right.modifiers)
}

pub fn hotkey_strings_overlap(left: &str, right: &str) -> Result<bool, KeyParseError> {
    Ok(hotkeys_overlap(parse_hotkey(left)?, parse_hotkey(right)?))
}

pub fn danger_for_platform(hotkey: Hotkey, platform: HotkeyPlatform) -> Option<DangerousHotkey> {
    match platform {
        HotkeyPlatform::Windows | HotkeyPlatform::Linux => windows_linux_danger(hotkey),
        HotkeyPlatform::Mac => mac_danger(hotkey),
    }
}

pub fn conflicts_with_taurine_global_hotkey(hotkey: Hotkey) -> Option<TaurineReservedHotkey> {
    hotkeys_overlap(hotkey, taurine_pause_hotkey()).then_some(TaurineReservedHotkey::PauseToggle)
}

pub fn taurine_pause_hotkey() -> Hotkey {
    hotkey_with(&[Modifier::Alt], LogicalKey::Backquote)
}

fn insert_modifier(modifiers: &mut Modifiers, modifier: Modifier) -> Result<(), KeyParseError> {
    match modifiers.insert(modifier) {
        Ok(()) => Ok(()),
        Err(ModifierInsertError::Duplicate(existing)) => Err(KeyParseError::DuplicateModifier {
            modifier: existing.canonical_name(),
        }),
        Err(ModifierInsertError::Conflict { existing, incoming }) => {
            Err(KeyParseError::ConflictingModifiers {
                first: existing.canonical_name(),
                second: incoming.canonical_name(),
            })
        }
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
    danger_match(
        hotkey,
        DangerousHotkey::Copy,
        &[Modifier::Ctrl],
        LogicalKey::Letter('c'),
    )
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::Paste,
            &[Modifier::Ctrl],
            LogicalKey::Letter('v'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::Cut,
            &[Modifier::Ctrl],
            LogicalKey::Letter('x'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::Undo,
            &[Modifier::Ctrl],
            LogicalKey::Letter('z'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::SelectAll,
            &[Modifier::Ctrl],
            LogicalKey::Letter('a'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::Save,
            &[Modifier::Ctrl],
            LogicalKey::Letter('s'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::AppSwitcher,
            &[Modifier::Alt],
            LogicalKey::Tab,
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::CloseWindow,
            &[Modifier::Alt],
            LogicalKey::Function(4),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::SecureAttention,
            &[Modifier::Ctrl, Modifier::Alt],
            LogicalKey::Delete,
        )
    })
}

fn mac_danger(hotkey: Hotkey) -> Option<DangerousHotkey> {
    danger_match(
        hotkey,
        DangerousHotkey::Copy,
        &[Modifier::Meta],
        LogicalKey::Letter('c'),
    )
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::Paste,
            &[Modifier::Meta],
            LogicalKey::Letter('v'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::Cut,
            &[Modifier::Meta],
            LogicalKey::Letter('x'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::Undo,
            &[Modifier::Meta],
            LogicalKey::Letter('z'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::SelectAll,
            &[Modifier::Meta],
            LogicalKey::Letter('a'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::Save,
            &[Modifier::Meta],
            LogicalKey::Letter('s'),
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::AppSwitcher,
            &[Modifier::Meta],
            LogicalKey::Tab,
        )
    })
    .or_else(|| {
        danger_match(
            hotkey,
            DangerousHotkey::QuitApplication,
            &[Modifier::Meta],
            LogicalKey::Letter('q'),
        )
    })
}

fn danger_match(
    hotkey: Hotkey,
    dangerous: DangerousHotkey,
    modifiers: &[Modifier],
    key: LogicalKey,
) -> Option<DangerousHotkey> {
    hotkeys_overlap(hotkey, hotkey_with(modifiers, key)).then_some(dangerous)
}

fn hotkey_with(modifiers: &[Modifier], key: LogicalKey) -> Hotkey {
    let mut bitset = Modifiers::new();
    for modifier in modifiers {
        if bitset.insert(*modifier).is_err() {
            tracing::debug!("hotkey_with received a duplicate modifier; skipping it");
        }
    }
    Hotkey {
        modifiers: bitset,
        key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::ModifierFamily;

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
    fn normalizes_side_specific_modifier_aliases() {
        assert_eq!(normalize_hotkey("ralt+m").unwrap(), "ralt+m");
        assert_eq!(normalize_hotkey("lalt+m").unwrap(), "lalt+m");
        assert_eq!(normalize_hotkey("rightalt+m").unwrap(), "ralt+m");
        assert_eq!(normalize_hotkey("altgr+m").unwrap(), "ralt+m");
        assert_eq!(
            normalize_hotkey("leftcontrol+rightalt+k").unwrap(),
            "lctrl+ralt+k"
        );
        assert_eq!(
            normalize_hotkey("leftshift+rightmeta+space").unwrap(),
            "lshift+rmeta+space"
        );
    }

    #[test]
    fn rejects_duplicate_and_conflicting_modifiers() {
        assert_eq!(
            parse_hotkey("ctrl+control+k").unwrap_err(),
            KeyParseError::DuplicateModifier { modifier: "ctrl" }
        );
        assert_eq!(
            parse_hotkey("alt+ralt+m").unwrap_err(),
            KeyParseError::ConflictingModifiers {
                first: "alt",
                second: "ralt",
            }
        );
        assert_eq!(
            parse_hotkey("lalt+ralt+m").unwrap_err(),
            KeyParseError::ConflictingModifiers {
                first: "lalt",
                second: "ralt",
            }
        );
        assert_eq!(
            parse_hotkey("ctrl+lctrl+k").unwrap_err(),
            KeyParseError::ConflictingModifiers {
                first: "ctrl",
                second: "lctrl",
            }
        );
        assert_eq!(
            parse_hotkey("lctrl+rctrl+k").unwrap_err(),
            KeyParseError::ConflictingModifiers {
                first: "lctrl",
                second: "rctrl",
            }
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
    fn semantic_overlap_is_side_aware() {
        assert!(hotkey_strings_overlap("alt+m", "lalt+m").unwrap());
        assert!(hotkey_strings_overlap("alt+m", "ralt+m").unwrap());
        assert!(!hotkey_strings_overlap("lalt+m", "ralt+m").unwrap());
        assert!(hotkey_strings_overlap("ctrl+alt+m", "lctrl+ralt+m").unwrap());
        assert!(hotkey_strings_overlap("lctrl+alt+m", "lctrl+ralt+m").unwrap());
        assert!(!hotkey_strings_overlap("lctrl+alt+m", "rctrl+alt+m").unwrap());
        assert!(!hotkey_strings_overlap("ctrl+m", "ctrl+alt+m").unwrap());
    }

    #[test]
    fn runtime_matching_is_strict_about_sides_and_extra_families() {
        let generic_alt = parse_hotkey("alt+m").unwrap();
        let left_alt = parse_hotkey("lalt+m").unwrap();
        let right_alt = parse_hotkey("ralt+m").unwrap();
        let ctrl_right_alt = parse_hotkey("ctrl+ralt+m").unwrap();

        assert!(hotkey_matches(generic_alt, left_alt));
        assert!(hotkey_matches(generic_alt, right_alt));
        assert!(hotkey_matches(right_alt, right_alt));
        assert!(!hotkey_matches(right_alt, left_alt));
        assert!(!hotkey_matches(right_alt, ctrl_right_alt));
    }

    #[test]
    fn flags_platform_specific_dangerous_hotkeys_including_side_specific_variants() {
        let windows_copy = parse_hotkey("ctrl+c").unwrap();
        let windows_left_copy = parse_hotkey("lctrl+c").unwrap();
        let windows_right_switch = parse_hotkey("ralt+tab").unwrap();
        let mac_copy = parse_hotkey("cmd+c").unwrap();
        let mac_quit = parse_hotkey("meta+q").unwrap();

        assert_eq!(
            danger_for_platform(windows_copy, HotkeyPlatform::Windows),
            Some(DangerousHotkey::Copy)
        );
        assert_eq!(
            danger_for_platform(windows_left_copy, HotkeyPlatform::Linux),
            Some(DangerousHotkey::Copy)
        );
        assert_eq!(
            danger_for_platform(windows_right_switch, HotkeyPlatform::Linux),
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
    fn detects_taurine_pause_hotkey_conflicts_for_generic_and_side_specific_alt() {
        let pause = parse_hotkey("alt+`").unwrap();
        let left_pause = parse_hotkey("lalt+`").unwrap();
        let right_pause = parse_hotkey("ralt+`").unwrap();
        let alt_enter = parse_hotkey("alt+enter").unwrap();
        let alt_escape = parse_hotkey("alt+esc").unwrap();

        assert_eq!(
            conflicts_with_taurine_global_hotkey(pause),
            Some(TaurineReservedHotkey::PauseToggle)
        );
        assert_eq!(
            conflicts_with_taurine_global_hotkey(left_pause),
            Some(TaurineReservedHotkey::PauseToggle)
        );
        assert_eq!(
            conflicts_with_taurine_global_hotkey(right_pause),
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
        assert_eq!(normalize_keypress_alias("rightcommand").unwrap(), "rmeta");
    }

    #[test]
    fn modifier_helpers_preserve_family_order() {
        let hotkey = parse_hotkey("rctrl+lalt+k").unwrap();
        let ordered: Vec<ModifierFamily> =
            hotkey.modifiers.ordered().map(Modifier::family).collect();
        assert_eq!(ordered, vec![ModifierFamily::Ctrl, ModifierFamily::Alt]);
    }

    #[test]
    fn parses_and_normalizes_mouse_button_hotkeys() {
        assert_eq!(normalize_hotkey("ralt+mouse4").unwrap(), "ralt+mouse4");
        assert_eq!(normalize_hotkey("ctrl+mouse1").unwrap(), "ctrl+mouse1");
        assert_eq!(normalize_hotkey("shift+m5").unwrap(), "shift+mouse5");
        assert_eq!(normalize_hotkey("alt+middleclick").unwrap(), "alt+mouse3");
        assert_eq!(
            normalize_hotkey("ctrl+shift+mouse3").unwrap(),
            "ctrl+shift+mouse3"
        );

        let parsed = parse_hotkey("ralt+mouse4").unwrap();
        assert_eq!(
            parsed.key,
            LogicalKey::Mouse(crate::keys::MouseButton::Button4)
        );
        assert!(parsed.modifiers.contains(Modifier::RightAlt));
    }

    #[test]
    fn rejects_bare_mouse_buttons_without_modifiers() {
        assert_eq!(
            parse_hotkey("mouse4").unwrap_err(),
            KeyParseError::MouseButtonRequiresModifier {
                key: "mouse4".to_string(),
            }
        );
        assert_eq!(
            parse_hotkey("mouse1").unwrap_err(),
            KeyParseError::MouseButtonRequiresModifier {
                key: "mouse1".to_string(),
            }
        );
        assert_eq!(
            parse_hotkey("lclick").unwrap_err(),
            KeyParseError::MouseButtonRequiresModifier {
                key: "lclick".to_string(),
            }
        );
        assert_eq!(
            parse_hotkey("m5").unwrap_err(),
            KeyParseError::MouseButtonRequiresModifier {
                key: "m5".to_string(),
            }
        );
    }
}
