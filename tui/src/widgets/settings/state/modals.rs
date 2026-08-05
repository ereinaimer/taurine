use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::settings::Settings;

use super::{EditorKind, SettingKey, SettingsInteraction};

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
    value: String,
    cursor: usize,
    pub(crate) error: Option<String>,
}

impl InputModalState {
    pub(crate) fn new(key: SettingKey, value: String) -> Self {
        let cursor = value.chars().count();
        Self {
            key,
            value,
            cursor,
            error: None,
        }
    }

    pub(crate) const fn key(&self) -> SettingKey {
        self.key
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
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
                    && self.value.chars().count() != 1
                {
                    self.error = Some(format!(
                        "{} must be exactly one character.",
                        self.key.display_name()
                    ));
                    return SettingsInteraction::handled();
                }
                let value = match self.key.editor_kind() {
                    EditorKind::OptionalTextInput if self.value.trim().is_empty() => None,
                    _ => Some(self.value.clone()),
                };
                SettingsInteraction::save(self.key, value)
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.backspace();
                SettingsInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.cursor = self.cursor.saturating_sub(1);
                SettingsInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.cursor = (self.cursor + 1).min(self.value.chars().count());
                SettingsInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.cursor = 0;
                SettingsInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.cursor = self.value.chars().count();
                SettingsInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_char(ch);
                SettingsInteraction::handled()
            }
            _ => SettingsInteraction::handled(),
        }
    }

    fn insert_char(&mut self, ch: char) {
        let byte_index = char_index_to_byte_index(&self.value, self.cursor);
        self.value.insert(byte_index, ch);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let end = char_index_to_byte_index(&self.value, self.cursor);
        let start = char_index_to_byte_index(&self.value, self.cursor - 1);
        self.value.drain(start..end);
        self.cursor -= 1;
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

fn char_index_to_byte_index(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(value.len())
}
