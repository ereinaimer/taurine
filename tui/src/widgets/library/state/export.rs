use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::widgets::field::TextField;
use crate::widgets::library::actions::{LibraryInteraction, PendingLibraryExport};

use super::ButtonSelection;

pub(crate) const EXPORT_MODAL_FIELDS: [LibraryExportModalField; 3] = [
    LibraryExportModalField::Path,
    LibraryExportModalField::Password,
    LibraryExportModalField::ActionButton,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryExportModalField {
    Path,
    Password,
    ActionButton,
}

impl LibraryExportModalField {
    fn is_action_button(self) -> bool {
        matches!(self, Self::ActionButton)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryExportModalState {
    path: TextField,
    password: TextField,
    focus: LibraryExportModalField,
    error: Option<String>,
    button_selection: ButtonSelection,
}

impl LibraryExportModalState {
    pub fn new() -> taurine_core::Result<Self> {
        let path = taurine_core::exchange::resolve_export_path(None)?
            .to_string_lossy()
            .into_owned();

        Ok(Self {
            path: TextField::new(path),
            password: TextField::new(""),
            focus: LibraryExportModalField::Path,
            error: None,
            button_selection: ButtonSelection::Cancel,
        })
    }

    pub(crate) fn path(&self) -> &str {
        self.path.text()
    }

    pub(crate) fn path_field(&self) -> &TextField {
        &self.path
    }

    pub(crate) fn path_cursor(&self) -> usize {
        self.path.cursor()
    }

    pub(crate) fn password_cursor(&self) -> usize {
        self.password.cursor()
    }

    pub(crate) fn password_masked(&self) -> String {
        "*".repeat(self.password.len_chars())
    }

    pub(crate) fn password_display_value(&self) -> String {
        self.password_masked()
    }

    pub(crate) const fn focus(&self) -> LibraryExportModalField {
        self.focus
    }

    pub(crate) fn set_focus(&mut self, field: LibraryExportModalField) {
        self.focus = field;
    }

    pub(crate) fn focus_next(&mut self) {
        self.advance_focus(true);
    }

    pub(crate) fn focus_prev(&mut self) {
        self.advance_focus(false);
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) const fn button_selection(&self) -> ButtonSelection {
        self.button_selection
    }

    pub(crate) fn set_button_selection(&mut self, selection: ButtonSelection) {
        self.button_selection = selection;
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    fn visible_fields(&self) -> &'static [LibraryExportModalField] {
        &EXPORT_MODAL_FIELDS
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        self.error = None;

        if self.focus.is_action_button() {
            return self.handle_action_button_key(key);
        }

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => LibraryInteraction::close(),
            (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.advance_focus(true);
                LibraryInteraction::handled()
            }
            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.advance_focus(false);
                LibraryInteraction::handled()
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.advance_focus(true);
                LibraryInteraction::handled()
            }
            (KeyCode::BackTab, _) => {
                self.advance_focus(false);
                LibraryInteraction::handled()
            }
            _ => self.handle_focused_key(key),
        }
    }

    fn handle_action_button_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Left | KeyCode::Char('h'), KeyModifiers::NONE) => {
                self.button_selection = ButtonSelection::Cancel;
                LibraryInteraction::handled()
            }
            (KeyCode::Right | KeyCode::Char('l'), KeyModifiers::NONE) => {
                self.button_selection = ButtonSelection::Confirm;
                LibraryInteraction::handled()
            }
            (KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab, _) => {
                self.advance_focus(false);
                LibraryInteraction::handled()
            }
            (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => {
                match self.button_selection {
                    ButtonSelection::Cancel => LibraryInteraction::close(),
                    ButtonSelection::Confirm => match self.build_pending_export() {
                        Ok(pending_export) => LibraryInteraction::export(pending_export),
                        Err(error) => {
                            self.error = Some(error.to_string());
                            LibraryInteraction::handled()
                        }
                    },
                }
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn build_pending_export(&self) -> taurine_core::Result<PendingLibraryExport> {
        if self.path.text().trim().is_empty() {
            return Err(taurine_core::Error::Config(
                "Export path is required.".to_string(),
            ));
        }

        let password = if self.password.text().trim().is_empty() {
            None
        } else {
            taurine_core::exchange::validate_export_password(self.password.text())?;
            Some(self.password.text().to_string())
        };

        Ok(PendingLibraryExport {
            path: self.path.text().to_string(),
            password,
        })
    }

    fn handle_focused_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match self.focus {
            LibraryExportModalField::Path => self.handle_path_key(key),
            LibraryExportModalField::Password => self.handle_password_key(key),
            LibraryExportModalField::ActionButton => {
                unreachable!("ActionButton is handled before focused key dispatch")
            }
        }
    }

    fn handle_path_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.path.move_left();
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.path.move_right();
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.path.move_home();
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.path.move_end();
                LibraryInteraction::handled()
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.path.backspace();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.path.delete_at();
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.path.insert(ch);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_password_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.password.move_left();
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.password.move_right();
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.password.move_home();
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.password.move_end();
                LibraryInteraction::handled()
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.password.backspace();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.password.delete_at();
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.password.insert(ch);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn advance_focus(&mut self, forward: bool) {
        let fields = self.visible_fields();
        let current_index = fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);

        if forward {
            if self.focus == LibraryExportModalField::ActionButton {
                return;
            }
            self.focus = fields[(current_index + 1).min(fields.len() - 1)];
        } else {
            if current_index == 0 {
                return;
            }
            self.focus = fields[current_index - 1];
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryExportResultModalState {
    body: String,
}

impl LibraryExportResultModalState {
    pub(crate) fn new(path: &Path) -> Self {
        let body = format!(
            "Triggers are exported to: {} as an encrypted export.",
            path.display()
        );

        Self { body }
    }

    pub(crate) fn body(&self) -> &str {
        &self.body
    }

    pub(crate) fn set_error(&mut self, _error: String) {}
}
