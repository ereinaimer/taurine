#[cfg(target_os = "linux")]
use evdev::KeyCode;
#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType};
use taurine_core::keys::{Hotkey, KeyPress, Modifiers, hotkey_matches, parse_hotkey};

#[cfg(target_os = "linux")]
use crate::hotkey_evaluator::logical_key_from_evdev;
#[cfg(not(target_os = "linux"))]
use crate::hotkey_evaluator::logical_key_from_rdev;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    pub hotkey: Hotkey,
}

pub fn parse_pause_hotkey_setting(setting: &str) -> Option<HotkeySpec> {
    parse_hotkey(setting)
        .ok()
        .map(|hotkey| HotkeySpec { hotkey })
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
}
