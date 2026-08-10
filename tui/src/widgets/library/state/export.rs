use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::widgets::library::actions::{
    LibraryInteraction, PendingLibraryExport, char_index_to_byte_index,
};

use super::ButtonSelection;

pub(crate) const LIBRARY_EXPORT_MODAL_FOOTER: &str = "↑/↓ Move   Tab Next";
pub(crate) const LIBRARY_EXPORT_RESULT_FOOTER: &str = "Enter Close   Esc Close";
pub(crate) const EXPORT_MODAL_FIELDS: [LibraryExportModalField; 7] = [
    LibraryExportModalField::Path,
    LibraryExportModalField::Encrypt,
    LibraryExportModalField::Password,
    LibraryExportModalField::IncludeSettings,
    LibraryExportModalField::IncludeSensitiveSettings,
    LibraryExportModalField::IncludeStats,
    LibraryExportModalField::ActionButton,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryExportModalField {
    Path,
    Encrypt,
    Password,
    IncludeSettings,
    IncludeSensitiveSettings,
    IncludeStats,
    ActionButton,
}

impl LibraryExportModalField {
    fn is_action_button(self) -> bool {
        matches!(self, Self::ActionButton)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryExportModalState {
    path: String,
    path_cursor: usize,
    encrypt: bool,
    password: String,
    password_cursor: usize,
    include_settings: bool,
    include_sensitive_settings: bool,
    include_stats: bool,
    focus: LibraryExportModalField,
    error: Option<String>,
    button_selection: ButtonSelection,
}

impl LibraryExportModalState {
    pub fn new() -> taurine_core::Result<Self> {
        let path = taurine_core::exchange::resolve_export_path(None)?
            .to_string_lossy()
            .into_owned();
        let path_cursor = path.chars().count();

        Ok(Self {
            path,
            path_cursor,
            encrypt: true,
            password: String::new(),
            password_cursor: 0,
            include_settings: false,
            include_sensitive_settings: false,
            include_stats: false,
            focus: LibraryExportModalField::Path,
            error: None,
            button_selection: ButtonSelection::Cancel,
        })
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn path_cursor(&self) -> usize {
        self.path_cursor
    }

    pub(crate) const fn encrypt(&self) -> bool {
        self.encrypt
    }

    pub(crate) fn password_masked(&self) -> String {
        "*".repeat(self.password.chars().count())
    }

    pub(crate) fn password(&self) -> &str {
        &self.password
    }

    pub(crate) fn password_display_value(&self) -> String {
        self.password_masked()
    }

    pub(crate) const fn password_cursor(&self) -> usize {
        self.password_cursor
    }

    pub(crate) const fn include_settings(&self) -> bool {
        self.include_settings
    }

    pub(crate) const fn include_sensitive_settings(&self) -> bool {
        self.include_sensitive_settings
    }

    pub(crate) const fn include_stats(&self) -> bool {
        self.include_stats
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

    fn should_skip_field(&self, field: LibraryExportModalField) -> bool {
        !self.encrypt
            && matches!(
                field,
                LibraryExportModalField::Password
                    | LibraryExportModalField::IncludeSensitiveSettings
            )
    }

    pub(crate) fn footer_text(&self) -> &'static str {
        LIBRARY_EXPORT_MODAL_FOOTER
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
        if self.path.trim().is_empty() {
            return Err(taurine_core::Error::Config(
                "Export path is required.".to_string(),
            ));
        }

        if self.encrypt && self.password.is_empty() {
            return Err(taurine_core::Error::Config(
                "Encryption password is required.".to_string(),
            ));
        }

        if self.encrypt && self.password.len() < 8 {
            return Err(taurine_core::Error::Config(
                "Encryption password must be at least 8 characters long".to_string(),
            ));
        }

        Ok(PendingLibraryExport {
            path: self.path.clone(),
            encrypt: self.encrypt,
            password: self.encrypt.then(|| self.password.clone()),
            include_settings: self.include_settings,
            include_sensitive_settings: self.include_sensitive_settings,
            include_stats: self.include_stats,
        })
    }

    fn handle_focused_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match self.focus {
            LibraryExportModalField::Path => self.handle_path_key(key),
            LibraryExportModalField::Encrypt => self.handle_encrypt_key(key),
            LibraryExportModalField::Password => self.handle_password_key(key),
            LibraryExportModalField::IncludeSettings => self.handle_include_settings_key(key),
            LibraryExportModalField::IncludeSensitiveSettings => {
                self.handle_include_sensitive_settings_key(key)
            }
            LibraryExportModalField::IncludeStats => self.handle_include_stats_key(key),
            LibraryExportModalField::ActionButton => {
                unreachable!("ActionButton is handled before focused key dispatch")
            }
        }
    }

    fn handle_path_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.path_cursor = self.path_cursor.saturating_sub(1);
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.path_cursor = (self.path_cursor + 1).min(self.path.chars().count());
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.path_cursor = 0;
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.path_cursor = self.path.chars().count();
                LibraryInteraction::handled()
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.delete_path_backward();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.delete_path_forward();
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_path_char(ch);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_encrypt_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                self.encrypt = !self.encrypt;
                if !self.encrypt {
                    self.password.clear();
                    self.password_cursor = 0;
                    self.include_sensitive_settings = false;
                }
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_password_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.password_cursor = self.password_cursor.saturating_sub(1);
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.password_cursor =
                    (self.password_cursor + 1).min(self.password.chars().count());
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.password_cursor = 0;
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.password_cursor = self.password.chars().count();
                LibraryInteraction::handled()
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.delete_password_backward();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.delete_password_forward();
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_password_char(ch);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_include_settings_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                self.include_settings = !self.include_settings;
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_include_sensitive_settings_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) if self.encrypt => {
                self.include_sensitive_settings = !self.include_sensitive_settings;
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_include_stats_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                self.include_stats = !self.include_stats;
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
            let mut next = current_index + 1;
            while next < fields.len() && self.should_skip_field(fields[next]) {
                next += 1;
            }
            self.focus = fields[next.min(fields.len() - 1)];
        } else {
            if current_index == 0 {
                return;
            }
            let mut prev = current_index - 1;
            while prev > 0 && self.should_skip_field(fields[prev]) {
                prev -= 1;
            }
            self.focus = fields[prev];
        }
    }

    fn insert_path_char(&mut self, ch: char) {
        let byte_index = char_index_to_byte_index(&self.path, self.path_cursor);
        self.path.insert(byte_index, ch);
        self.path_cursor += 1;
    }

    fn delete_path_backward(&mut self) {
        if self.path_cursor == 0 {
            return;
        }

        let end = char_index_to_byte_index(&self.path, self.path_cursor);
        let start = char_index_to_byte_index(&self.path, self.path_cursor - 1);
        self.path.replace_range(start..end, "");
        self.path_cursor -= 1;
    }

    fn delete_path_forward(&mut self) {
        if self.path_cursor >= self.path.chars().count() {
            return;
        }

        let start = char_index_to_byte_index(&self.path, self.path_cursor);
        let end = char_index_to_byte_index(&self.path, self.path_cursor + 1);
        self.path.replace_range(start..end, "");
    }

    fn insert_password_char(&mut self, ch: char) {
        let byte_index = char_index_to_byte_index(&self.password, self.password_cursor);
        self.password.insert(byte_index, ch);
        self.password_cursor += 1;
    }

    fn delete_password_backward(&mut self) {
        if self.password_cursor == 0 {
            return;
        }

        let end = char_index_to_byte_index(&self.password, self.password_cursor);
        let start = char_index_to_byte_index(&self.password, self.password_cursor - 1);
        self.password.replace_range(start..end, "");
        self.password_cursor -= 1;
    }

    fn delete_password_forward(&mut self) {
        if self.password_cursor >= self.password.chars().count() {
            return;
        }

        let start = char_index_to_byte_index(&self.password, self.password_cursor);
        let end = char_index_to_byte_index(&self.password, self.password_cursor + 1);
        self.password.replace_range(start..end, "");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryExportResultModalState {
    body: String,
}

impl LibraryExportResultModalState {
    pub(crate) fn new(
        path: &Path,
        encrypt: bool,
        include_settings: bool,
        include_stats: bool,
    ) -> Self {
        let subject = match (include_settings, include_stats) {
            (false, false) => "Triggers".to_string(),
            (true, false) => "Triggers and Settings".to_string(),
            (false, true) => "Triggers and Stats".to_string(),
            (true, true) => "Triggers, Settings and Stats".to_string(),
        };

        let body = match (include_settings, include_stats, encrypt) {
            (false, false, false) => format!("{} are exported to: {}", subject, path.display()),
            (false, false, true) => format!(
                "{} are exported to: {} as an encrypted export.",
                subject,
                path.display()
            ),
            (_, _, true) => format!(
                "{} were exported to: {} with encryption.",
                subject,
                path.display()
            ),
            (_, _, false) => format!("{} were exported to: {}", subject, path.display()),
        };

        Self { body }
    }

    pub(crate) fn body(&self) -> &str {
        &self.body
    }

    pub(crate) const fn footer_text(&self) -> &'static str {
        LIBRARY_EXPORT_RESULT_FOOTER
    }

    pub(crate) fn set_error(&mut self, _error: String) {}
}
