use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::keys::{HotkeyPlatform, Modifier, danger_for_platform, parse_hotkey};

use super::{SettingKey, SettingsInteraction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HotkeyCaptureModalState {
    key: SettingKey,
    current: String,
    captured: Option<String>,
    preview: Option<String>,
    pub(crate) error: Option<String>,
}

impl HotkeyCaptureModalState {
    pub(crate) fn new(key: SettingKey, current: String) -> Self {
        Self {
            key,
            current,
            captured: None,
            preview: None,
            error: None,
        }
    }

    pub(crate) const fn key(&self) -> SettingKey {
        self.key
    }

    pub(crate) fn current(&self) -> &str {
        &self.current
    }

    pub(crate) fn captured(&self) -> Option<&str> {
        self.captured.as_deref()
    }

    pub(crate) fn preview(&self) -> Option<&str> {
        self.preview.as_deref()
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> SettingsInteraction {
        self.error = None;

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => SettingsInteraction::cancel(),
            (KeyCode::Enter, KeyModifiers::NONE) => match self.captured.clone() {
                Some(value) => SettingsInteraction::save(self.key, Some(value)),
                None => {
                    self.error = Some("Press the keys to use first.".to_string());
                    SettingsInteraction::handled()
                }
            },
            _ => {
                self.capture_chord(&key);
                SettingsInteraction::handled()
            }
        }
    }

    fn capture_chord(&mut self, key: &KeyEvent) {
        let Some(base) = base_alias(key.code) else {
            // honey: lone modifiers/media keys carry no base; show held keys only.
            self.preview = held_preview(key.modifiers);
            return;
        };
        let mut parts: Vec<String> = held_modifiers(key.modifiers)
            .iter()
            .map(|modifier| modifier.canonical_name().to_string())
            .collect();
        parts.push(base);
        let hotkey = match parse_hotkey(&parts.join("+")) {
            Ok(hotkey) => hotkey,
            Err(error) => {
                self.error = Some(format!("Cannot use that combination ({error})."));
                return;
            }
        };
        if let Some(dangerous) = danger_for_platform(hotkey, current_platform()) {
            self.error = Some(format!(
                "Overlaps the {} shortcut.",
                dangerous.description()
            ));
            return;
        }
        self.preview = None;
        self.captured = Some(hotkey.canonical_string());
    }
}

fn held_modifiers(modifiers: KeyModifiers) -> Vec<Modifier> {
    let mut held = Vec::with_capacity(4);
    if modifiers.contains(KeyModifiers::SHIFT) {
        held.push(Modifier::Shift);
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        held.push(Modifier::Ctrl);
    }
    if modifiers.contains(KeyModifiers::ALT) {
        held.push(Modifier::Alt);
    }
    if modifiers.intersects(KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META) {
        held.push(Modifier::Meta);
    }
    held
}

fn held_preview(modifiers: KeyModifiers) -> Option<String> {
    let parts: Vec<String> = held_modifiers(modifiers)
        .iter()
        .map(|modifier| modifier.canonical_name().to_string())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(format!("{}+…", parts.join("+")))
    }
}

const fn current_platform() -> HotkeyPlatform {
    #[cfg(target_os = "macos")]
    {
        HotkeyPlatform::Mac
    }
    #[cfg(target_os = "windows")]
    {
        HotkeyPlatform::Windows
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        HotkeyPlatform::Linux
    }
}

fn base_alias(code: KeyCode) -> Option<String> {
    match code {
        KeyCode::Char(' ') => Some("space".to_string()),
        KeyCode::Char(ch) => Some(ch.to_ascii_lowercase().to_string()),
        KeyCode::F(number) => Some(format!("f{number}")),
        KeyCode::Tab => Some("tab".to_string()),
        KeyCode::Delete => Some("delete".to_string()),
        KeyCode::Insert => Some("insert".to_string()),
        KeyCode::Up => Some("up".to_string()),
        KeyCode::Down => Some("down".to_string()),
        KeyCode::Left => Some("left".to_string()),
        KeyCode::Right => Some("right".to_string()),
        KeyCode::Home => Some("home".to_string()),
        KeyCode::End => Some("end".to_string()),
        KeyCode::PageUp => Some("pgup".to_string()),
        KeyCode::PageDown => Some("pgdown".to_string()),
        KeyCode::CapsLock => Some("capslock".to_string()),
        KeyCode::ScrollLock => Some("scrolllock".to_string()),
        KeyCode::NumLock => Some("numlock".to_string()),
        KeyCode::PrintScreen => Some("printscreen".to_string()),
        KeyCode::Pause => Some("pause".to_string()),
        // honey: bare Enter/Esc/Backspace are overlay controls; with held
        // modifiers they are capturable base keys instead.
        KeyCode::Enter => Some("enter".to_string()),
        KeyCode::Esc => Some("esc".to_string()),
        KeyCode::Backspace => Some("backspace".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::settings::state::SettingKeyMeta;
    use crossterm::event::ModifierKeyCode;

    fn test_state() -> HotkeyCaptureModalState {
        HotkeyCaptureModalState::new(SettingKey::PauseHotkey, "Alt + `".to_string())
    }

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn captures_ctrl_p_and_saves_canonical() {
        let mut state = test_state();
        let interaction = state.handle_key(press(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert!(interaction.pending_save().is_none());
        assert_eq!(state.captured(), Some("ctrl+p"));

        let interaction = state.handle_key(press(KeyCode::Enter, KeyModifiers::NONE));
        let pending = interaction.pending_save().expect("enter saves");
        assert_eq!(pending.key, SettingKey::PauseHotkey);
        assert_eq!(pending.value.as_deref(), Some("ctrl+p"));
    }

    #[test]
    fn captures_bare_function_key() {
        let mut state = test_state();
        state.handle_key(press(KeyCode::F(5), KeyModifiers::NONE));
        assert_eq!(state.captured(), Some("f5"));
    }

    #[test]
    fn shift_letter_keeps_shift_and_lowercases() {
        let mut state = test_state();
        state.handle_key(press(KeyCode::Char('P'), KeyModifiers::SHIFT));
        assert_eq!(state.captured(), Some("shift+p"));
    }

    #[test]
    fn ctrl_enter_captures_instead_of_confirming() {
        let mut state = test_state();
        state.handle_key(press(KeyCode::Char('p'), KeyModifiers::CONTROL));
        state.handle_key(press(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!(state.captured(), Some("ctrl+enter"));
    }

    #[test]
    fn lone_modifier_shows_preview_without_capture() {
        let mut state = test_state();
        state.handle_key(press(
            KeyCode::Modifier(ModifierKeyCode::LeftShift),
            KeyModifiers::SHIFT,
        ));
        assert_eq!(state.captured(), None);
        assert_eq!(state.preview(), Some("shift+…"));
    }

    #[test]
    fn dangerous_combo_is_rejected() {
        let mut state = test_state();
        state.handle_key(press(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(state.captured(), None);
        assert!(state.error().unwrap_or_default().contains("copy shortcut"));
    }

    #[test]
    fn enter_without_capture_shows_error() {
        let mut state = test_state();
        let interaction = state.handle_key(press(KeyCode::Enter, KeyModifiers::NONE));
        assert!(interaction.pending_save().is_none());
        assert!(state.error().is_some());
    }

    #[test]
    fn bare_backspace_captures_as_base_key() {
        let mut state = test_state();
        state.handle_key(press(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(state.captured(), Some("backspace"));
    }

    #[test]
    fn esc_cancels() {
        let mut state = test_state();
        state.handle_key(press(KeyCode::F(5), KeyModifiers::NONE));
        assert_eq!(state.captured(), Some("f5"));

        let interaction = state.handle_key(press(KeyCode::Esc, KeyModifiers::NONE));
        assert!(interaction.pending_save().is_none());
        assert!(interaction.should_close_modal());
    }

    #[test]
    fn current_value_is_prefilled_from_settings() {
        let settings = taurine_core::settings::Settings::default();
        let state = HotkeyCaptureModalState::new(
            SettingKey::PauseHotkey,
            SettingKey::PauseHotkey.edit_value(&settings),
        );
        assert_eq!(state.current(), "Alt + `");
        assert_eq!(state.captured(), None);
    }
}
