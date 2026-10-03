use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use taurine_core::settings::Settings;

use super::{EditorKind, SettingKey, SettingKeyMeta, SettingsInteraction};
use crate::widgets::field::TextField;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfirmResetModalState {
    key: SettingKey,
    default_display_value: String,
    selected_yes: bool,
    pub(crate) error: Option<String>,
}

impl ConfirmResetModalState {
    pub(crate) fn new(key: SettingKey) -> Self {
        let defaults = Settings::default();
        Self {
            key,
            default_display_value: key.display_value(&defaults),
            selected_yes: true,
            error: None,
        }
    }

    pub(crate) const fn key(&self) -> SettingKey {
        self.key
    }

    pub(crate) fn default_display_value(&self) -> &str {
        &self.default_display_value
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) const fn selected_yes(&self) -> bool {
        self.selected_yes
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> SettingsInteraction {
        self.error = None;

        match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                self.selected_yes = true;
                SettingsInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
                self.selected_yes = false;
                SettingsInteraction::handled()
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if self.selected_yes {
                    SettingsInteraction::reset(self.key)
                } else {
                    SettingsInteraction::cancel()
                }
            }
            (KeyCode::Char('y'), KeyModifiers::NONE) | (KeyCode::Char('Y'), KeyModifiers::NONE) => {
                SettingsInteraction::reset(self.key)
            }
            (KeyCode::Char('n'), KeyModifiers::NONE)
            | (KeyCode::Char('N'), KeyModifiers::NONE)
            | (KeyCode::Esc, KeyModifiers::NONE) => SettingsInteraction::cancel(),
            _ => SettingsInteraction::handled(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputModalState {
    key: SettingKey,
    value: TextField,
    pub(crate) error: Option<String>,
}

impl InputModalState {
    pub(crate) fn new(key: SettingKey, value: String) -> Self {
        Self {
            key,
            value: TextField::new(value),
            error: None,
        }
    }

    pub(crate) const fn key(&self) -> SettingKey {
        self.key
    }

    pub(crate) fn field(&self) -> &TextField {
        &self.value
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> SettingsInteraction {
        self.error = None;

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => SettingsInteraction::cancel(),
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if self.key.editor_kind() == EditorKind::SingleCharInput
                    && self.value.len_chars() != 1
                {
                    self.error = Some(format!(
                        "{} must be exactly one character.",
                        self.key.display_name()
                    ));
                    return SettingsInteraction::handled();
                }
                let value = match self.key.editor_kind() {
                    EditorKind::OptionalTextInput if self.value.text().trim().is_empty() => None,
                    _ => Some(self.value.text().to_string()),
                };
                SettingsInteraction::save(self.key, value)
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.value.backspace();
                SettingsInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.value.delete_at();
                SettingsInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.value.move_left();
                SettingsInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.value.move_right();
                SettingsInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.value.move_home();
                SettingsInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.value.move_end();
                SettingsInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.value.insert(ch);
                SettingsInteraction::handled()
            }
            _ => SettingsInteraction::handled(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectModalState {
    key: SettingKey,
    options: Vec<String>,
    selected: usize,
    pub(crate) error: Option<String>,
}

impl SelectModalState {
    pub(crate) fn new(key: SettingKey, options: Vec<String>, current_value: String) -> Self {
        let selected = options
            .iter()
            .position(|option| option.eq_ignore_ascii_case(&current_value))
            .unwrap_or_default();

        Self {
            key,
            options,
            selected,
            error: None,
        }
    }

    pub(crate) const fn key(&self) -> SettingKey {
        self.key
    }

    pub(crate) fn options(&self) -> &[String] {
        &self.options
    }

    pub(crate) const fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> SettingsInteraction {
        self.error = None;

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => SettingsInteraction::cancel(),
            (KeyCode::Enter, KeyModifiers::NONE) => {
                SettingsInteraction::save(self.key, Some(self.options[self.selected].clone()))
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                self.selected = (self.selected + 1).min(self.options.len().saturating_sub(1));
                SettingsInteraction::handled()
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                self.selected = self.selected.saturating_sub(1);
                SettingsInteraction::handled()
            }
            _ => SettingsInteraction::handled(),
        }
    }
}
