use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::exchange::ImportStatsMode;

use crate::widgets::library::actions::{
    LibraryImportConflictMode, LibraryImportOutcome, LibraryInteraction,
    PendingLibraryImportPrepare, PreparedLibraryImport, char_index_to_byte_index,
};

use super::trigger::LibrarySelectState;
use super::{ButtonSelection, LibraryImportModalField};

pub(crate) const LIBRARY_IMPORT_MODAL_FOOTER: &str = "↑/↓ Move   Tab Next";
pub(crate) const LIBRARY_IMPORT_RESULT_FOOTER: &str = "Enter Close   Esc Close";
pub(crate) const LIBRARY_IMPORT_RUN_VARIABLES_FOOTER: &str = "y Continue   n Cancel   Esc Cancel";
pub(crate) const IMPORT_MODAL_FIELDS: [LibraryImportModalField; 7] = [
    LibraryImportModalField::Path,
    LibraryImportModalField::Password,
    LibraryImportModalField::IncludeSettings,
    LibraryImportModalField::IncludeSensitiveSettings,
    LibraryImportModalField::StatsMode,
    LibraryImportModalField::ConflictMode,
    LibraryImportModalField::ActionButton,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryImportModalState {
    path: String,
    path_cursor: usize,
    password: String,
    password_cursor: usize,
    include_settings: bool,
    include_sensitive_settings: bool,
    stats_mode: ImportStatsMode,
    conflict_mode: LibraryImportConflictMode,
    focus: LibraryImportModalField,
    error: Option<String>,
    selector: Option<LibrarySelectState>,
    button_selection: ButtonSelection,
    file_is_encrypted: Option<bool>,
}

impl LibraryImportModalState {
    pub fn new() -> Self {
        Self {
            path: String::new(),
            path_cursor: 0,
            password: String::new(),
            password_cursor: 0,
            include_settings: false,
            include_sensitive_settings: false,
            stats_mode: ImportStatsMode::Ignore,
            conflict_mode: LibraryImportConflictMode::Skip,
            focus: LibraryImportModalField::Path,
            error: None,
            selector: None,
            button_selection: ButtonSelection::Cancel,
            file_is_encrypted: None,
        }
    }

    pub(crate) fn with_path(path: impl Into<String>) -> Self {
        let path = path.into();
        let path_cursor = path.chars().count();
        let mut state = Self {
            path,
            path_cursor,
            password: String::new(),
            password_cursor: 0,
            include_settings: false,
            include_sensitive_settings: false,
            stats_mode: ImportStatsMode::Ignore,
            conflict_mode: LibraryImportConflictMode::Skip,
            focus: LibraryImportModalField::IncludeSettings,
            error: None,
            selector: None,
            button_selection: ButtonSelection::Cancel,
            file_is_encrypted: None,
        };
        state.detect_file_encryption();
        state
    }

    pub(crate) fn is_encrypted(&self) -> Option<bool> {
        self.file_is_encrypted
    }

    fn detect_file_encryption(&mut self) {
        let path = self.path.trim();
        if path.is_empty() {
            self.file_is_encrypted = None;
            return;
        }
        self.file_is_encrypted = std::fs::File::open(path).ok().and_then(|mut f| {
            use std::io::Read;
            let mut header = [0u8; 4];
            f.read_exact(&mut header).ok()?;
            match &header {
                b"TAUP" => Some(false),
                b"TAU1" => Some(true),
                _ => None,
            }
        });
        if self.file_is_encrypted == Some(false) && self.focus == LibraryImportModalField::Password
        {
            self.advance_focus(true);
        }
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn path_cursor(&self) -> usize {
        self.path_cursor
    }

    pub(crate) fn password(&self) -> &str {
        &self.password
    }

    pub(crate) fn password_display_value(&self) -> String {
        "*".repeat(self.password.chars().count())
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

    #[cfg(test)]
    pub(crate) const fn stats_mode(&self) -> ImportStatsMode {
        self.stats_mode
    }

    #[cfg(test)]
    pub(crate) const fn conflict_mode(&self) -> LibraryImportConflictMode {
        self.conflict_mode
    }

    pub(crate) const fn focus(&self) -> LibraryImportModalField {
        self.focus
    }

    pub(crate) fn set_focus(&mut self, field: LibraryImportModalField) {
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

    pub(crate) fn footer_text(&self) -> &'static str {
        if self.selector.is_some() {
            "j/k Move   ↑/↓ Move   Enter Save   Esc Cancel"
        } else {
            LIBRARY_IMPORT_MODAL_FOOTER
        }
    }

    pub(crate) fn selector(&self) -> Option<&LibrarySelectState> {
        self.selector.as_ref()
    }

    pub(crate) const fn stats_mode_label(&self) -> &'static str {
        match self.stats_mode {
            ImportStatsMode::Ignore => "ignore",
            ImportStatsMode::Merge => "merge",
            ImportStatsMode::Overwrite => "overwrite",
        }
    }

    pub(crate) const fn conflict_mode_label(&self) -> &'static str {
        self.conflict_mode.label()
    }

    fn visible_fields(&self) -> &'static [LibraryImportModalField] {
        &IMPORT_MODAL_FIELDS
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        if self.selector.is_some() {
            return self.handle_selector_key(key);
        }

        self.error = None;

        if self.focus == LibraryImportModalField::ActionButton {
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
                    ButtonSelection::Confirm => match self.build_pending_prepare() {
                        Ok(pending_prepare) => LibraryInteraction::prepare_import(pending_prepare),
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

    fn build_pending_prepare(&self) -> taurine_core::Result<PendingLibraryImportPrepare> {
        if self.path.trim().is_empty() {
            return Err(taurine_core::Error::Config(
                "Import path is required.".to_string(),
            ));
        }

        if self.file_is_encrypted == Some(true) && self.password.is_empty() {
            return Err(taurine_core::Error::Config(
                "Password is required for encrypted file.".to_string(),
            ));
        }

        let password = match self.file_is_encrypted {
            Some(false) => None,
            _ => (!self.password.is_empty()).then(|| self.password.clone()),
        };

        Ok(PendingLibraryImportPrepare {
            path: self.path.clone(),
            password,
            options: taurine_core::exchange::ImportOptions {
                include_settings: self.include_settings,
                stats_mode: self.stats_mode,
                include_sensitive_settings: self.include_sensitive_settings,
            },
            conflict_mode: self.conflict_mode,
            return_to_modal: self.clone(),
        })
    }

    fn handle_focused_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match self.focus {
            LibraryImportModalField::Path => self.handle_path_key(key),
            LibraryImportModalField::Password => self.handle_password_key(key),
            LibraryImportModalField::IncludeSettings => self.handle_include_settings_key(key),
            LibraryImportModalField::IncludeSensitiveSettings => {
                self.handle_include_sensitive_settings_key(key)
            }
            LibraryImportModalField::StatsMode => self.handle_stats_mode_key(key),
            LibraryImportModalField::ConflictMode => self.handle_conflict_mode_key(key),
            LibraryImportModalField::ActionButton => {
                unreachable!("ActionButton is handled before focused key dispatch")
            }
        }
    }

    fn handle_selector_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        let Some(mut selector) = self.selector.take() else {
            return LibraryInteraction::handled();
        };

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.selector = None;
                LibraryInteraction::handled()
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                selector.selected =
                    (selector.selected + 1).min(selector.options.len().saturating_sub(1));
                self.selector = Some(selector);
                LibraryInteraction::handled()
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                selector.selected = selector.selected.saturating_sub(1);
                self.selector = Some(selector);
                LibraryInteraction::handled()
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                match self.focus {
                    LibraryImportModalField::StatsMode => {
                        self.stats_mode = match selector.selected {
                            0 => ImportStatsMode::Ignore,
                            1 => ImportStatsMode::Merge,
                            _ => ImportStatsMode::Overwrite,
                        };
                    }
                    LibraryImportModalField::ConflictMode => {
                        self.conflict_mode = match selector.selected {
                            0 => LibraryImportConflictMode::Skip,
                            _ => LibraryImportConflictMode::Overwrite,
                        };
                    }
                    _ => {}
                }
                LibraryInteraction::handled()
            }
            _ => {
                self.selector = Some(selector);
                LibraryInteraction::handled()
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
            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                self.include_sensitive_settings = !self.include_sensitive_settings;
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_stats_mode_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                self.selector = Some(LibrarySelectState {
                    title: "Select Stats Mode",
                    options: vec![
                        "ignore".to_string(),
                        "merge".to_string(),
                        "overwrite".to_string(),
                    ],
                    selected: match self.stats_mode {
                        ImportStatsMode::Ignore => 0,
                        ImportStatsMode::Merge => 1,
                        ImportStatsMode::Overwrite => 2,
                    },
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_conflict_mode_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                self.selector = Some(LibrarySelectState {
                    title: "Select Conflict Mode",
                    options: LibraryImportConflictMode::ALL
                        .iter()
                        .map(|mode| mode.label().to_string())
                        .collect(),
                    selected: match self.conflict_mode {
                        LibraryImportConflictMode::Skip => 0,
                        LibraryImportConflictMode::Overwrite => 1,
                    },
                });
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
            if self.focus == LibraryImportModalField::ActionButton {
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

    fn should_skip_field(&self, field: LibraryImportModalField) -> bool {
        field == LibraryImportModalField::Password && self.file_is_encrypted == Some(false)
    }

    fn insert_path_char(&mut self, ch: char) {
        let byte_index = char_index_to_byte_index(&self.path, self.path_cursor);
        self.path.insert(byte_index, ch);
        self.path_cursor += 1;
        self.detect_file_encryption();
    }

    fn delete_path_backward(&mut self) {
        if self.path_cursor == 0 {
            return;
        }
        let end = char_index_to_byte_index(&self.path, self.path_cursor);
        let start = char_index_to_byte_index(&self.path, self.path_cursor - 1);
        self.path.replace_range(start..end, "");
        self.path_cursor -= 1;
        self.detect_file_encryption();
    }

    fn delete_path_forward(&mut self) {
        if self.path_cursor >= self.path.chars().count() {
            return;
        }
        let start = char_index_to_byte_index(&self.path, self.path_cursor);
        let end = char_index_to_byte_index(&self.path, self.path_cursor + 1);
        self.path.replace_range(start..end, "");
        self.detect_file_encryption();
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LibraryImportRunVariablesModalState {
    pub(crate) prepared: PreparedLibraryImport,
    pub(crate) return_to_import: LibraryImportModalState,
    error: Option<String>,
}

impl LibraryImportRunVariablesModalState {
    pub(crate) fn new(
        prepared: PreparedLibraryImport,
        return_to_import: LibraryImportModalState,
    ) -> Self {
        Self {
            prepared,
            return_to_import,
            error: None,
        }
    }

    pub(crate) fn path(&self) -> &str {
        self.prepared.path()
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryImportResultModalState {
    lines: Vec<String>,
}

impl LibraryImportResultModalState {
    pub(crate) fn from_outcome(outcome: &LibraryImportOutcome) -> Self {
        let mut lines = vec![format!("Imported {} trigger(s).", outcome.imported())];
        if outcome.imported_settings() {
            lines.push("Settings imported.".to_string());
        }
        if outcome.imported_stats() {
            lines.push("Stats updated.".to_string());
        }
        Self { lines }
    }

    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }

    pub(crate) const fn footer_text(&self) -> &'static str {
        LIBRARY_IMPORT_RESULT_FOOTER
    }

    pub(crate) fn set_error(&mut self, _error: String) {}
}
