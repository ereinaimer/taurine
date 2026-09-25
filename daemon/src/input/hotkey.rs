#[cfg(target_os = "linux")]
use evdev::KeyCode;
#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType};
use std::sync::atomic::AtomicBool;
use taurine_core::keys::{
    Hotkey, KeyPress, LogicalKey, Modifier, ModifierState, Modifiers, hotkey_matches, parse_hotkey,
};

#[cfg(target_os = "linux")]
use crate::input::hotkey_evaluator::logical_key_from_evdev;
#[cfg(not(target_os = "linux"))]
use crate::input::hotkey_evaluator::logical_key_from_rdev;

pub static PTT_KEY_DOWN: AtomicBool = AtomicBool::new(false);
pub static HANDSFREE_KEY_DOWN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    pub hotkey: Hotkey,
}

pub fn parse_pause_hotkey_setting(setting: &str) -> Option<HotkeySpec> {
    parse_hotkey(setting)
        .ok()
        .map(|hotkey| HotkeySpec { hotkey })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceHotkeySpec {
    /// A chord composed purely of modifiers, e.g. "win+lctrl" or "win+lctrl+lalt".
    ModifierChord(Modifiers),
    /// A standard hotkey with a non-modifier base key and optional modifiers, e.g. "ctrl+shift+v" or "f8".
    Standard(Hotkey),
}

impl VoiceHotkeySpec {
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Ok(hotkey) = parse_hotkey(trimmed) {
            return Some(Self::Standard(hotkey));
        }

        let tokens = taurine_core::keys::split_tokens(trimmed).ok()?;
        let mut modifiers = Modifiers::new();
        for token in tokens {
            let modifier = Modifier::from_alias(&token)?;
            modifiers.insert(modifier).ok()?;
        }

        if modifiers.is_empty() {
            None
        } else {
            Some(Self::ModifierChord(modifiers))
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn matches_press(&self, event: &Event, active_modifiers: Modifiers) -> bool {
        let EventType::KeyPress(key) = event.event_type else {
            return false;
        };
        let Some(logical) = logical_key_from_rdev(key) else {
            return false;
        };

        match self {
            Self::ModifierChord(req_mods) => match logical {
                LogicalKey::Modifier(m) => {
                    req_mods.family_state(m.family()) != ModifierState::Absent
                        && req_mods.matches_active(active_modifiers)
                }
                _ => false,
            },
            Self::Standard(hotkey) => {
                logical == hotkey.key && hotkey.modifiers.matches_active(active_modifiers)
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn matches_release(&self, event: &Event) -> bool {
        let EventType::KeyRelease(key) = event.event_type else {
            return false;
        };
        let Some(logical) = logical_key_from_rdev(key) else {
            return false;
        };

        match self {
            Self::ModifierChord(req_mods) => match logical {
                LogicalKey::Modifier(m) => {
                    req_mods.family_state(m.family()) != ModifierState::Absent
                }
                _ => false,
            },
            Self::Standard(hotkey) => {
                if logical == hotkey.key {
                    true
                } else if let LogicalKey::Modifier(m) = logical
                    && hotkey.modifiers.family_state(m.family()) != ModifierState::Absent
                {
                    true
                } else {
                    false
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub fn matches_press_evdev(
        &self,
        key: KeyCode,
        is_press: bool,
        active_modifiers: Modifiers,
    ) -> bool {
        if !is_press {
            return false;
        }
        let Some(logical) = logical_key_from_evdev(key) else {
            return false;
        };

        match self {
            Self::ModifierChord(req_mods) => match logical {
                LogicalKey::Modifier(m) => {
                    req_mods.family_state(m.family()) != ModifierState::Absent
                        && req_mods.matches_active(active_modifiers)
                }
                _ => false,
            },
            Self::Standard(hotkey) => {
                logical == hotkey.key && hotkey.modifiers.matches_active(active_modifiers)
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub fn matches_release_evdev(&self, key: KeyCode, is_release: bool) -> bool {
        if !is_release {
            return false;
        }
        let Some(logical) = logical_key_from_evdev(key) else {
            return false;
        };

        match self {
            Self::ModifierChord(req_mods) => match logical {
                LogicalKey::Modifier(m) => {
                    req_mods.family_state(m.family()) != ModifierState::Absent
                }
                _ => false,
            },
            Self::Standard(hotkey) => {
                if logical == hotkey.key {
                    true
                } else if let LogicalKey::Modifier(m) = logical
                    && hotkey.modifiers.family_state(m.family()) != ModifierState::Absent
                {
                    true
                } else {
                    false
                }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn is_pause_chord(event: &Event, modifiers: Modifiers, spec: &HotkeySpec) -> bool {
    let EventType::KeyPress(key) = event.event_type else {
        return false;
    };

    let Some(key) = logical_key_from_rdev(key) else {
        return false;
    };

    hotkey_matches(spec.hotkey, KeyPress { modifiers, key })
}

#[cfg(target_os = "linux")]
pub fn is_pause_chord_evdev(
    key: KeyCode,
    is_press: bool,
    modifiers: Modifiers,
    spec: &HotkeySpec,
) -> bool {
    if !is_press {
        return false;
    }

    let Some(key) = logical_key_from_evdev(key) else {
        return false;
    };

    hotkey_matches(spec.hotkey, KeyPress { modifiers, key })
}

#[cfg(test)]
#[cfg(not(target_os = "linux"))]
mod tests {
    use super::*;
    use rdev::{EventType, Key};
    use taurine_core::keys::{Modifier, Modifiers, parse_hotkey};

    fn ev(event_type: EventType) -> Event {
        Event {
            event_type,
            time: std::time::SystemTime::now(),
            name: None,
        }
    }

    fn modifiers_with(modifiers: &[Modifier]) -> Modifiers {
        let mut bitset = Modifiers::new();
        for modifier in modifiers {
            bitset.insert_active(*modifier);
        }
        bitset
    }

    #[test]
    fn parse_accepts_case_and_spacing_variants_without_implying_shift() {
        let expected = parse_hotkey("alt+`").expect("pause hotkey should parse");

        assert_eq!(
            parse_pause_hotkey_setting("Alt + `")
                .expect("default pause hotkey should parse")
                .hotkey,
            expected
        );
        assert_eq!(
            parse_pause_hotkey_setting("alt + `")
                .expect("lowercase pause hotkey should parse")
                .hotkey,
            expected
        );
        assert_eq!(
            parse_pause_hotkey_setting("Alt+`")
                .expect("compact pause hotkey should parse")
                .hotkey,
            expected
        );
    }

    #[test]
    fn matches_only_on_exact_keypress_with_required_modifiers() {
        let spec = parse_pause_hotkey_setting("Alt + `").unwrap();
        let press_bt = ev(EventType::KeyPress(Key::BackQuote));
        assert!(is_pause_chord(
            &press_bt,
            modifiers_with(&[Modifier::LeftAlt]),
            &spec
        ));
        assert!(!is_pause_chord(
            &press_bt,
            modifiers_with(&[Modifier::LeftCtrl]),
            &spec
        ));
        assert!(!is_pause_chord(
            &press_bt,
            modifiers_with(&[Modifier::LeftShift]),
            &spec
        ));
        assert!(!is_pause_chord(&press_bt, Modifiers::new(), &spec));
        assert!(!is_pause_chord(
            &press_bt,
            modifiers_with(&[Modifier::LeftAlt, Modifier::LeftShift]),
            &spec
        ));

        let release_bt = ev(EventType::KeyRelease(Key::BackQuote));
        assert!(!is_pause_chord(
            &release_bt,
            modifiers_with(&[Modifier::LeftAlt]),
            &spec
        ));
    }

    #[test]
    fn voice_hotkey_spec_parses_modifier_chords_and_standards() {
        let ptt = VoiceHotkeySpec::parse("win+lctrl").expect("modifier chord should parse");
        assert!(matches!(ptt, VoiceHotkeySpec::ModifierChord(_)));

        let hf =
            VoiceHotkeySpec::parse("win+lctrl+lalt").expect("3-key modifier chord should parse");
        assert!(matches!(hf, VoiceHotkeySpec::ModifierChord(_)));

        let std = VoiceHotkeySpec::parse("ctrl+shift+v").expect("standard hotkey should parse");
        assert!(matches!(std, VoiceHotkeySpec::Standard(_)));

        let fn_key = VoiceHotkeySpec::parse("f8").expect("function key should parse");
        assert!(matches!(fn_key, VoiceHotkeySpec::Standard(_)));

        assert!(VoiceHotkeySpec::parse("").is_none());
        assert!(VoiceHotkeySpec::parse("invalid_nonexistent_key_name").is_none());
    }

    #[test]
    fn voice_hotkey_spec_matches_modifier_chord_press_and_release() {
        let spec = VoiceHotkeySpec::parse("win+lctrl").unwrap();

        // Press LeftCtrl with LeftMeta and LeftCtrl active
        let press_lctrl = ev(EventType::KeyPress(Key::ControlLeft));
        let mods_meta_ctrl = modifiers_with(&[Modifier::LeftMeta, Modifier::LeftCtrl]);
        assert!(spec.matches_press(&press_lctrl, mods_meta_ctrl));

        // Press LeftCtrl without LeftMeta active
        let mods_ctrl_only = modifiers_with(&[Modifier::LeftCtrl]);
        assert!(!spec.matches_press(&press_lctrl, mods_ctrl_only));

        // Press non-modifier key A with both modifiers active
        let press_a = ev(EventType::KeyPress(Key::KeyA));
        assert!(!spec.matches_press(&press_a, mods_meta_ctrl));

        // Release LeftCtrl
        let release_lctrl = ev(EventType::KeyRelease(Key::ControlLeft));
        assert!(spec.matches_release(&release_lctrl));

        // Release LeftMeta
        let release_meta = ev(EventType::KeyRelease(Key::MetaLeft));
        assert!(spec.matches_release(&release_meta));

        // Release unrelated key
        let release_a = ev(EventType::KeyRelease(Key::KeyA));
        assert!(!spec.matches_release(&release_a));
    }

    #[test]
    fn voice_hotkey_spec_matches_standard_press_and_release() {
        let spec = VoiceHotkeySpec::parse("ctrl+shift+v").unwrap();

        let press_v = ev(EventType::KeyPress(Key::KeyV));
        let mods_ctrl_shift = modifiers_with(&[Modifier::LeftCtrl, Modifier::LeftShift]);
        assert!(spec.matches_press(&press_v, mods_ctrl_shift));

        let mods_ctrl_only = modifiers_with(&[Modifier::LeftCtrl]);
        assert!(!spec.matches_press(&press_v, mods_ctrl_only));

        let release_v = ev(EventType::KeyRelease(Key::KeyV));
        assert!(spec.matches_release(&release_v));

        let release_ctrl = ev(EventType::KeyRelease(Key::ControlLeft));
        assert!(spec.matches_release(&release_ctrl));

        let release_a = ev(EventType::KeyRelease(Key::KeyA));
        assert!(!spec.matches_release(&release_a));
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod linux_tests {
    use super::*;
    use taurine_core::keys::{Modifier, Modifiers};

    fn modifiers_with(modifiers: &[Modifier]) -> Modifiers {
        let mut bitset = Modifiers::new();
        for modifier in modifiers {
            bitset.insert_active(*modifier);
        }
        bitset
    }

    #[test]
    fn linux_pause_chord_matches_only_on_exact_keypress_with_required_modifiers() {
        let spec = parse_pause_hotkey_setting("Alt + `").unwrap();

        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            Modifiers::new(),
            &spec
        ));
        assert!(is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            modifiers_with(&[Modifier::LeftAlt]),
            &spec
        ));
        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            modifiers_with(&[Modifier::LeftCtrl]),
            &spec
        ));
        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            modifiers_with(&[Modifier::LeftShift]),
            &spec
        ));
        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            modifiers_with(&[Modifier::LeftAlt, Modifier::LeftShift]),
            &spec
        ));
        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            false,
            modifiers_with(&[Modifier::LeftAlt]),
            &spec
        ));
    }

    #[test]
    fn linux_voice_hotkey_spec_matches_modifier_chords_and_standards() {
        let spec = VoiceHotkeySpec::parse("win+lctrl").unwrap();

        let mods_meta_ctrl = modifiers_with(&[Modifier::LeftMeta, Modifier::LeftCtrl]);
        assert!(spec.matches_press_evdev(KeyCode::KEY_LEFTCTRL, true, mods_meta_ctrl));
        assert!(!spec.matches_press_evdev(KeyCode::KEY_LEFTCTRL, false, mods_meta_ctrl));
        assert!(!spec.matches_press_evdev(
            KeyCode::KEY_LEFTCTRL,
            true,
            modifiers_with(&[Modifier::LeftCtrl])
        ));

        assert!(spec.matches_release_evdev(KeyCode::KEY_LEFTCTRL, true));
        assert!(spec.matches_release_evdev(KeyCode::KEY_LEFTMETA, true));
        assert!(!spec.matches_release_evdev(KeyCode::KEY_A, true));
    }
}
