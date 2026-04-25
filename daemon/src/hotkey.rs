#[cfg(target_os = "linux")]
use evdev::KeyCode;
#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType, Key};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyKey {
    BackQuote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    pub require_alt: bool,
    pub key: HotkeyKey,
}

pub fn parse_pause_hotkey_setting(setting: &str) -> Option<HotkeySpec> {
    // Strict, explicit mapping (business requirement is a single default chord).
    // We keep it strict so the CLI and daemon agree on the displayed value.
    if setting == "Alt + `" {
        // On rdev 0.5.x, the backtick key is represented as Key::BackQuote.
        return Some(HotkeySpec {
            require_alt: true,
            key: HotkeyKey::BackQuote,
        });
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn is_pause_chord(event: &Event, alt_down: bool, spec: &HotkeySpec) -> bool {
    if spec.require_alt && !alt_down {
        return false;
    }

    match event.event_type {
        EventType::KeyPress(k) => match spec.key {
            HotkeyKey::BackQuote => k == Key::BackQuote,
        },
        _ => false,
    }
}

#[cfg(target_os = "linux")]
pub fn is_pause_chord_evdev(
    key: KeyCode,
    is_press: bool,
    alt_down: bool,
    spec: &HotkeySpec,
) -> bool {
    if !is_press {
        return false;
    }

    if spec.require_alt && !alt_down {
        return false;
    }

    match spec.key {
        HotkeyKey::BackQuote => key == KeyCode::KEY_GRAVE,
    }
}

#[cfg(test)]
#[cfg(not(target_os = "linux"))]
mod tests {
    use super::*;
    use rdev::EventType;

    fn ev(event_type: EventType) -> Event {
        Event {
            event_type,
            time: std::time::SystemTime::now(),
            name: None,
        }
    }

    #[test]
    fn parse_strict_default() {
        let spec = parse_pause_hotkey_setting("Alt + `").expect("should parse");
        assert!(spec.require_alt);
        assert_eq!(spec.key, HotkeyKey::BackQuote);
        assert!(parse_pause_hotkey_setting("alt + `").is_none());
        assert!(parse_pause_hotkey_setting("Alt+`").is_none());
    }

    #[test]
    fn matches_only_on_keypress_with_alt_down() {
        let spec = parse_pause_hotkey_setting("Alt + `").unwrap();
        let mut alt_down = false;

        let press_bt = ev(EventType::KeyPress(Key::BackQuote));
        assert!(!is_pause_chord(&press_bt, alt_down, &spec));

        alt_down = true;
        assert!(is_pause_chord(&press_bt, alt_down, &spec));

        let release_bt = ev(EventType::KeyRelease(Key::BackQuote));
        assert!(!is_pause_chord(&release_bt, alt_down, &spec));
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod linux_tests {
    use super::*;

    #[test]
    fn linux_pause_chord_matches_only_on_keypress_with_alt_down() {
        let spec = parse_pause_hotkey_setting("Alt + `").unwrap();

        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            false,
            &spec
        ));
        assert!(is_pause_chord_evdev(KeyCode::KEY_GRAVE, true, true, &spec));
        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            false,
            true,
            &spec
        ));
    }
}
