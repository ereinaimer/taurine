#![allow(dead_code)]
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::db::crud::{
    SUPPORTED_TARGET_OS_VALUES, TriggerListItem, TriggerRow, TriggerType,
};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};
use taurine_core::exchange::ImportStatsMode;

use crate::widgets::library::actions::{
    LibraryImportConflictMode, LibraryImportOutcome, LibraryInteraction, PendingLibraryDelete,
    PendingLibraryExport, PendingLibraryImportPrepare, PendingLibrarySave, PreparedLibraryImport,
};

pub(crate) const LIBRARY_FOOTER: &str =
    "/ Search   n New   i Import   x Export   d Delete   Enter Edit   q Quit";
pub(crate) const LIBRARY_EDIT_MODAL_FOOTER: &str =
    "Ctrl+S Save   Esc Cancel   Tab Next   Shift+Tab Prev";
pub(crate) const LIBRARY_CREATE_MODAL_FOOTER: &str =
    "Ctrl+S Save   Esc Cancel   Tab Next   Shift+Tab Prev";
pub(crate) const LIBRARY_EXPORT_MODAL_FOOTER: &str = "↑/↓ Move   Tab Next";
pub(crate) const LIBRARY_IMPORT_MODAL_FOOTER: &str = "↑/↓ Move   Tab Next";
pub(crate) const LIBRARY_IMPORT_RESULT_FOOTER: &str = "Enter Close   Esc Close";
pub(crate) const LIBRARY_EXPORT_RESULT_FOOTER: &str = "Enter Close   Esc Close";
pub(crate) const LIBRARY_DELETE_MODAL_FOOTER: &str = "Esc Cancel";
pub(crate) const LIBRARY_IMPORT_RUN_VARIABLES_FOOTER: &str = "y Continue   n Cancel   Esc Cancel";
pub(crate) const SCRIPT_LANGUAGE_OPTIONS: [ScriptInterpreter; 6] = [
    ScriptInterpreter::Bash,
    ScriptInterpreter::PowerShell,
    ScriptInterpreter::Python,
    ScriptInterpreter::Node,
    ScriptInterpreter::NodeEsm,
    ScriptInterpreter::Cmd,
];
pub(crate) const SCRIPT_MODE_OPTIONS: [ScriptBehavior; 2] =
    [ScriptBehavior::Inline, ScriptBehavior::Silent];
pub(crate) const EXPORT_MODAL_FIELDS: [LibraryExportModalField; 7] = [
    LibraryExportModalField::Path,
    LibraryExportModalField::Encrypt,
    LibraryExportModalField::Password,
    LibraryExportModalField::IncludeSettings,
    LibraryExportModalField::IncludeSensitiveSettings,
    LibraryExportModalField::IncludeStats,
    LibraryExportModalField::ActionButton,
];
pub(crate) const IMPORT_MODAL_FIELDS: [LibraryImportModalField; 7] = [
    LibraryImportModalField::Path,
    LibraryImportModalField::Password,
    LibraryImportModalField::IncludeSettings,
    LibraryImportModalField::IncludeSensitiveSettings,
    LibraryImportModalField::StatsMode,
    LibraryImportModalField::ConflictMode,
    LibraryImportModalField::ActionButton,
];
pub(crate) const SNIPPET_MODAL_FIELDS: [LibraryModalField; 4] = [
    LibraryModalField::Trigger,
    LibraryModalField::Content,
    LibraryModalField::Kind,
    LibraryModalField::TargetOs,
];
pub(crate) const SCRIPT_MODAL_FIELDS: [LibraryModalField; 6] = [
    LibraryModalField::Trigger,
    LibraryModalField::Content,
    LibraryModalField::Kind,
    LibraryModalField::TargetOs,
    LibraryModalField::Language,
    LibraryModalField::Mode,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryKind {
    Snippet,
    Script,
    HotkeySnippet,
    HotkeyScript,
}

impl LibraryKind {
    pub(crate) const ALL: [Self; 4] = [
        Self::Snippet,
        Self::Script,
        Self::HotkeySnippet,
        Self::HotkeyScript,
    ];

    pub(crate) fn from_parts(trigger_type: TriggerType, action_type: &str) -> Self {
        let is_script = action_type.eq_ignore_ascii_case("script");

        match (trigger_type, is_script) {
            (TriggerType::Hotkey, true) => Self::HotkeyScript,
            (TriggerType::Hotkey, false) => Self::HotkeySnippet,
            (TriggerType::Word, true) => Self::Script,
            (TriggerType::Word, false) => Self::Snippet,
            (TriggerType::Regex, true) => Self::Script,
            (TriggerType::Regex, false) => Self::Snippet,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Snippet => "snippet",
            Self::Script => "script",
            Self::HotkeySnippet => "hotkey snippet",
            Self::HotkeyScript => "hotkey script",
        }
    }

    pub(crate) const fn content_label(self) -> &'static str {
        match self {
            Self::Snippet | Self::HotkeySnippet => "Output",
            Self::Script | Self::HotkeyScript => "Script",
        }
    }

    pub(crate) const fn is_script(self) -> bool {
        matches!(self, Self::Script | Self::HotkeyScript)
    }

    pub(crate) fn trigger_type(self) -> TriggerType {
        match self {
            Self::Snippet | Self::Script => TriggerType::Word,
            Self::HotkeySnippet | Self::HotkeyScript => TriggerType::Hotkey,
        }
    }

    pub(crate) fn action_type(self) -> &'static str {
        if self.is_script() { "script" } else { "text" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryTrigger {
    id: String,
    name: String,
    trigger: String,
    preview: String,
    kind: LibraryKind,
    pub(crate) target_os: String,
    search_text: String,
    uses: u64,
}

impl LibraryTrigger {
    #[allow(dead_code)]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn trigger(&self) -> &str {
        &self.trigger
    }

    pub(crate) fn preview(&self) -> &str {
        &self.preview
    }

    pub(crate) const fn kind_label(&self) -> &'static str {
        self.kind.label()
    }

    pub(crate) fn metadata_label(&self) -> String {
        format!("{} // {} uses", self.target_os, self.uses)
    }

    fn matches_query(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }

        let needle = query.to_ascii_lowercase();
        self.search_text.contains(&needle)
    }
}

impl From<TriggerListItem> for LibraryTrigger {
    fn from(item: TriggerListItem) -> Self {
        let kind = LibraryKind::from_parts(item.trigger_type, item.action_type.as_str());
        let preview = crate::widgets::library::actions::preview_from_item(&item);
        let target_os =
            crate::widgets::library::actions::display_target_os(&item.target_os).to_string();
        let search_text =
            crate::widgets::library::actions::build_search_text(&item, kind.label(), &target_os);

        Self {
            id: item.id,
            name: item.name,
            trigger: item.trigger,
            preview,
            kind,
            target_os,
            search_text,
            uses: item.usage_count.max(0) as u64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryModalField {
    Trigger,
    Content,
    Kind,
    TargetOs,
    Language,
    Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonSelection {
    Cancel,
    Confirm,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryImportModalField {
    Path,
    Password,
    IncludeSettings,
    IncludeSensitiveSettings,
    StatsMode,
    ConflictMode,
    ActionButton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryEditorMode {
    Edit,
    Create,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryMetadataRow {
    label: &'static str,
    value: String,
}

impl LibraryMetadataRow {
    pub(crate) fn new(label: &'static str, value: String) -> Self {
        Self { label, value }
    }

    pub(crate) const fn label(&self) -> &'static str {
        self.label
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryDeleteModalState {
    trigger_id: String,
    name: String,
    selected_yes: bool,
    restore_index: usize,
    return_to_editor: Option<LibraryEditorModalState>,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibrarySelectState {
    title: &'static str,
    pub(crate) options: Vec<String>,
    pub(crate) selected: usize,
}

impl LibrarySelectState {
    pub(crate) const fn title(&self) -> &'static str {
        self.title
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

    fn set_error(&mut self, error: String) {
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
        let byte_index = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor,
        );
        self.path.insert(byte_index, ch);
        self.path_cursor += 1;
    }

    fn delete_path_backward(&mut self) {
        if self.path_cursor == 0 {
            return;
        }

        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor,
        );
        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor - 1,
        );
        self.path.replace_range(start..end, "");
        self.path_cursor -= 1;
    }

    fn delete_path_forward(&mut self) {
        if self.path_cursor >= self.path.chars().count() {
            return;
        }

        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor,
        );
        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor + 1,
        );
        self.path.replace_range(start..end, "");
    }

    fn insert_password_char(&mut self, ch: char) {
        let byte_index = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor,
        );
        self.password.insert(byte_index, ch);
        self.password_cursor += 1;
    }

    fn delete_password_backward(&mut self) {
        if self.password_cursor == 0 {
            return;
        }

        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor,
        );
        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor - 1,
        );
        self.password.replace_range(start..end, "");
        self.password_cursor -= 1;
    }

    fn delete_password_forward(&mut self) {
        if self.password_cursor >= self.password.chars().count() {
            return;
        }

        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor,
        );
        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor + 1,
        );
        self.password.replace_range(start..end, "");
    }
}

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
        let byte_index = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor,
        );
        self.path.insert(byte_index, ch);
        self.path_cursor += 1;
        self.detect_file_encryption();
    }

    fn delete_path_backward(&mut self) {
        if self.path_cursor == 0 {
            return;
        }
        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor,
        );
        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor - 1,
        );
        self.path.replace_range(start..end, "");
        self.path_cursor -= 1;
        self.detect_file_encryption();
    }

    fn delete_path_forward(&mut self) {
        if self.path_cursor >= self.path.chars().count() {
            return;
        }
        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor,
        );
        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.path,
            self.path_cursor + 1,
        );
        self.path.replace_range(start..end, "");
        self.detect_file_encryption();
    }

    fn insert_password_char(&mut self, ch: char) {
        let byte_index = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor,
        );
        self.password.insert(byte_index, ch);
        self.password_cursor += 1;
    }

    fn delete_password_backward(&mut self) {
        if self.password_cursor == 0 {
            return;
        }
        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor,
        );
        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor - 1,
        );
        self.password.replace_range(start..end, "");
        self.password_cursor -= 1;
    }

    fn delete_password_forward(&mut self) {
        if self.password_cursor >= self.password.chars().count() {
            return;
        }
        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor,
        );
        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.password,
            self.password_cursor + 1,
        );
        self.password.replace_range(start..end, "");
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LibraryImportRunVariablesModalState {
    prepared: PreparedLibraryImport,
    return_to_import: LibraryImportModalState,
    error: Option<String>,
}

impl LibraryImportRunVariablesModalState {
    fn new(prepared: PreparedLibraryImport, return_to_import: LibraryImportModalState) -> Self {
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

    fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryTriggerDetail {
    id: String,
    name: String,
    description: Option<String>,
    tags_json: String,
    usage_count: i64,
    last_used_at: Option<i64>,
    trigger: String,
    kind: LibraryKind,
    content: String,
    target_os_raw: String,
    metadata_rows: Vec<LibraryMetadataRow>,
    interpreter: Option<ScriptInterpreter>,
    behavior: Option<ScriptBehavior>,
}

impl LibraryTriggerDetail {
    pub(crate) fn from_row(row: TriggerRow) -> taurine_core::Result<Self> {
        let kind = LibraryKind::from_parts(row.trigger_type, row.action_type.as_str());
        let content = crate::widgets::library::actions::modal_content_from_row(&row, kind)?;
        let metadata_rows = crate::widgets::library::actions::build_metadata_rows(&row);

        Ok(Self {
            id: row.id,
            name: row.name,
            description: row.description,
            tags_json: row.tags,
            usage_count: row.usage_count,
            last_used_at: row.last_used_at,
            trigger: row.trigger,
            kind,
            content,
            target_os_raw: row.target_os,
            metadata_rows,
            interpreter: row.interpreter,
            behavior: row.behavior,
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub(crate) fn tags_json(&self) -> &str {
        &self.tags_json
    }

    pub(crate) const fn usage_count(&self) -> i64 {
        self.usage_count
    }

    pub(crate) const fn last_used_at(&self) -> Option<i64> {
        self.last_used_at
    }

    pub(crate) fn trigger(&self) -> &str {
        &self.trigger
    }

    pub(crate) const fn kind(&self) -> LibraryKind {
        self.kind
    }

    #[cfg(test)]
    pub(crate) const fn content_label(&self) -> &'static str {
        self.kind.content_label()
    }

    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    pub(crate) fn target_os_raw(&self) -> &str {
        &self.target_os_raw
    }

    pub(crate) fn metadata_rows(&self) -> &[LibraryMetadataRow] {
        &self.metadata_rows
    }

    pub(crate) const fn interpreter(&self) -> Option<ScriptInterpreter> {
        self.interpreter
    }

    pub(crate) const fn behavior(&self) -> Option<ScriptBehavior> {
        self.behavior
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryEditorModalState {
    mode: LibraryEditorMode,
    original: Option<LibraryTriggerDetail>,
    trigger: String,
    trigger_cursor: usize,
    content: String,
    content_cursor: usize,
    content_cursor_goal: Option<usize>,
    kind: LibraryKind,
    target_os: String,
    interpreter: ScriptInterpreter,
    behavior: ScriptBehavior,
    focus: LibraryModalField,
    content_scroll: usize,
    error: Option<String>,
    selector: Option<LibrarySelectState>,
}

impl LibraryEditorModalState {
    pub(crate) fn new_edit(trigger: LibraryTriggerDetail) -> Self {
        let trigger_cursor = trigger.trigger().chars().count();
        let content_cursor = trigger.content().chars().count();
        Self {
            mode: LibraryEditorMode::Edit,
            trigger: trigger.trigger().to_string(),
            trigger_cursor,
            content: trigger.content().to_string(),
            content_cursor,
            content_cursor_goal: None,
            kind: trigger.kind(),
            target_os: trigger.target_os_raw().to_string(),
            interpreter: trigger.interpreter().unwrap_or_else(|| {
                crate::widgets::library::actions::default_script_interpreter_for_target_os(
                    trigger.target_os_raw(),
                )
            }),
            behavior: trigger.behavior().unwrap_or(ScriptBehavior::Inline),
            focus: LibraryModalField::Trigger,
            content_scroll: 0,
            error: None,
            original: Some(trigger),
            selector: None,
        }
    }

    pub(crate) fn new_create() -> Self {
        Self {
            mode: LibraryEditorMode::Create,
            original: None,
            trigger: String::new(),
            trigger_cursor: 0,
            content: String::new(),
            content_cursor: 0,
            content_cursor_goal: None,
            kind: LibraryKind::Snippet,
            target_os: SUPPORTED_TARGET_OS_VALUES[0].to_string(),
            interpreter: crate::widgets::library::actions::default_script_interpreter_for_target_os(
                SUPPORTED_TARGET_OS_VALUES[0],
            ),
            behavior: ScriptBehavior::Inline,
            focus: LibraryModalField::Trigger,
            content_scroll: 0,
            error: None,
            selector: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn mode(&self) -> LibraryEditorMode {
        self.mode
    }

    pub(crate) const fn focus(&self) -> LibraryModalField {
        self.focus
    }

    pub(crate) fn trigger(&self) -> &str {
        &self.trigger
    }

    pub(crate) const fn trigger_cursor(&self) -> usize {
        self.trigger_cursor
    }

    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    pub(crate) const fn content_cursor(&self) -> usize {
        self.content_cursor
    }

    pub(crate) const fn kind_label(&self) -> &'static str {
        self.kind.label()
    }

    pub(crate) const fn content_label(&self) -> &'static str {
        self.kind.content_label()
    }

    pub(crate) fn target_os(&self) -> &str {
        crate::widgets::library::actions::display_target_os(&self.target_os)
    }

    pub(crate) const fn is_script_kind(&self) -> bool {
        self.kind.is_script()
    }

    pub(crate) const fn language_label(&self) -> &'static str {
        crate::widgets::library::actions::interpreter_label(self.interpreter)
    }

    pub(crate) const fn mode_label(&self) -> &'static str {
        crate::widgets::library::actions::behavior_label(self.behavior)
    }

    #[cfg(test)]
    pub(crate) const fn interpreter(&self) -> ScriptInterpreter {
        self.interpreter
    }

    #[cfg(test)]
    pub(crate) const fn behavior(&self) -> ScriptBehavior {
        self.behavior
    }

    pub(crate) fn metadata_rows(&self) -> &[LibraryMetadataRow] {
        self.original
            .as_ref()
            .map(LibraryTriggerDetail::metadata_rows)
            .unwrap_or(&[])
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub(crate) fn effective_content_scroll(&self, visible_lines: u16) -> usize {
        let max_scroll = self
            .content_line_count()
            .saturating_sub(visible_lines.max(1) as usize);
        self.content_scroll.min(max_scroll)
    }

    pub(crate) fn content_line_indicator(&self, _visible_lines: u16) -> Option<String> {
        let total_lines = self.content_line_count();
        if total_lines <= 1 {
            return None;
        }

        Some(format!(
            "{}/{}",
            self.current_content_line() + 1,
            total_lines
        ))
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        if self.selector.is_some() {
            return self.handle_selector_key(key);
        }

        self.error = None;

        if matches!(key.code, KeyCode::Char('s' | 'S'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return LibraryInteraction::save(self.build_pending_save());
        }

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => LibraryInteraction::close(),
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

    pub(crate) fn visible_content_lines(&self, visible_lines: u16) -> Vec<String> {
        let lines = crate::widgets::library::actions::split_lines_with_trailing(&self.content);
        let scroll = self.effective_content_scroll(visible_lines);
        let mut visible = lines
            .into_iter()
            .skip(scroll)
            .take(visible_lines.max(1) as usize)
            .map(str::to_string)
            .collect::<Vec<_>>();
        if visible.is_empty() {
            visible.push(String::new());
        }
        visible
    }

    pub(crate) fn build_pending_save(&self) -> PendingLibrarySave {
        use crate::widgets::library::actions::PendingLibrarySaveMode;
        let mode = match (self.mode, self.original.as_ref()) {
            (LibraryEditorMode::Edit, Some(original)) => PendingLibrarySaveMode::Update {
                id: original.id().to_string(),
                name: original.name().to_string(),
                description: original.description().map(str::to_string),
                tags_json: original.tags_json().to_string(),
                usage_count: original.usage_count(),
                last_used_at: original.last_used_at(),
                interpreter: original.interpreter(),
                behavior: original.behavior(),
            },
            (LibraryEditorMode::Create, _) => PendingLibrarySaveMode::Create,
            (LibraryEditorMode::Edit, None) => PendingLibrarySaveMode::Create,
        };

        PendingLibrarySave {
            mode,
            trigger: self.trigger.clone(),
            content: self.content.clone(),
            kind: self.kind,
            target_os: self.target_os.clone(),
            interpreter: self.kind.is_script().then_some(self.interpreter),
            behavior: self.kind.is_script().then_some(self.behavior),
        }
    }

    fn handle_focused_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match self.focus {
            LibraryModalField::Trigger => self.handle_trigger_key(key),
            LibraryModalField::Content => self.handle_content_key(key),
            LibraryModalField::Kind => self.handle_kind_key(key),
            LibraryModalField::TargetOs => self.handle_target_os_key(key),
            LibraryModalField::Language => self.handle_language_key(key),
            LibraryModalField::Mode => self.handle_mode_key(key),
        }
    }

    fn handle_trigger_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.backspace_trigger();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.delete_trigger();
                LibraryInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.trigger_cursor = self.trigger_cursor.saturating_sub(1);
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.trigger_cursor = (self.trigger_cursor + 1).min(self.trigger.chars().count());
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.trigger_cursor = 0;
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.trigger_cursor = self.trigger.chars().count();
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_trigger_char(ch);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_content_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Enter, KeyModifiers::NONE) => {
                self.insert_content_char('\n');
                LibraryInteraction::handled()
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.backspace_content();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.delete_content();
                LibraryInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.move_content_horizontal(-1);
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.move_content_horizontal(1);
                LibraryInteraction::handled()
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_content_vertical(-1);
                LibraryInteraction::handled()
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_content_vertical(1);
                LibraryInteraction::handled()
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => {
                self.move_content_vertical(-5);
                LibraryInteraction::handled()
            }
            (KeyCode::PageDown, KeyModifiers::NONE) => {
                self.move_content_vertical(5);
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.move_content_to_line_edge(true);
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.move_content_to_line_edge(false);
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_content_char(ch);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_kind_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                let options = LibraryKind::ALL
                    .iter()
                    .map(|k| k.label().to_string())
                    .collect();
                let selected = LibraryKind::ALL
                    .iter()
                    .position(|&k| k == self.kind)
                    .unwrap_or(0);
                self.selector = Some(LibrarySelectState {
                    title: "Select Kind",
                    options,
                    selected,
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_selector_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        let Some(selector) = self.selector.as_mut() else {
            return LibraryInteraction::handled();
        };

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.selector = None;
                LibraryInteraction::handled()
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if self.focus == LibraryModalField::Kind {
                    let previous_kind = self.kind;
                    self.kind = LibraryKind::ALL[selector.selected];
                    if !previous_kind.is_script() && self.kind.is_script() {
                        self.initialize_script_defaults();
                    }
                    self.ensure_focus_visible();
                } else if self.focus == LibraryModalField::TargetOs {
                    self.target_os = SUPPORTED_TARGET_OS_VALUES[selector.selected].to_string();
                } else if self.focus == LibraryModalField::Language {
                    self.interpreter = SCRIPT_LANGUAGE_OPTIONS[selector.selected];
                } else if self.focus == LibraryModalField::Mode {
                    self.behavior = SCRIPT_MODE_OPTIONS[selector.selected];
                }
                self.selector = None;
                LibraryInteraction::handled()
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                selector.selected =
                    (selector.selected + 1).min(selector.options.len().saturating_sub(1));
                LibraryInteraction::handled()
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                selector.selected = selector.selected.saturating_sub(1);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_target_os_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                let options = SUPPORTED_TARGET_OS_VALUES
                    .iter()
                    .map(|v| crate::widgets::library::actions::display_target_os(v).to_string())
                    .collect();
                let selected = SUPPORTED_TARGET_OS_VALUES
                    .iter()
                    .position(|&v| v == self.target_os)
                    .unwrap_or(0);
                self.selector = Some(LibrarySelectState {
                    title: "Select Target OS",
                    options,
                    selected,
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_language_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                let options = SCRIPT_LANGUAGE_OPTIONS
                    .iter()
                    .map(|value| {
                        crate::widgets::library::actions::interpreter_label(*value).to_string()
                    })
                    .collect();
                let selected = SCRIPT_LANGUAGE_OPTIONS
                    .iter()
                    .position(|value| *value == self.interpreter)
                    .unwrap_or(0);
                self.selector = Some(LibrarySelectState {
                    title: "Select Language",
                    options,
                    selected,
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_mode_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                let options = SCRIPT_MODE_OPTIONS
                    .iter()
                    .map(|value| {
                        crate::widgets::library::actions::behavior_label(*value).to_string()
                    })
                    .collect();
                let selected = SCRIPT_MODE_OPTIONS
                    .iter()
                    .position(|value| *value == self.behavior)
                    .unwrap_or(0);
                self.selector = Some(LibrarySelectState {
                    title: "Select Mode",
                    options,
                    selected,
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn insert_trigger_char(&mut self, ch: char) {
        let byte_index = crate::widgets::library::actions::char_index_to_byte_index(
            &self.trigger,
            self.trigger_cursor,
        );
        self.trigger.insert(byte_index, ch);
        self.trigger_cursor += 1;
    }

    fn backspace_trigger(&mut self) {
        if self.trigger_cursor == 0 {
            return;
        }

        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.trigger,
            self.trigger_cursor,
        );
        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.trigger,
            self.trigger_cursor - 1,
        );
        self.trigger.drain(start..end);
        self.trigger_cursor -= 1;
    }

    fn delete_trigger(&mut self) {
        if self.trigger_cursor >= self.trigger.chars().count() {
            return;
        }

        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.trigger,
            self.trigger_cursor,
        );
        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.trigger,
            self.trigger_cursor + 1,
        );
        self.trigger.drain(start..end);
    }

    fn insert_content_char(&mut self, ch: char) {
        let byte_index = crate::widgets::library::actions::char_index_to_byte_index(
            &self.content,
            self.content_cursor,
        );
        self.content.insert(byte_index, ch);
        self.content_cursor += 1;
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn backspace_content(&mut self) {
        if self.content_cursor == 0 {
            return;
        }

        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.content,
            self.content_cursor,
        );
        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.content,
            self.content_cursor - 1,
        );
        self.content.drain(start..end);
        self.content_cursor -= 1;
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn delete_content(&mut self) {
        if self.content_cursor >= self.content.chars().count() {
            return;
        }

        let start = crate::widgets::library::actions::char_index_to_byte_index(
            &self.content,
            self.content_cursor,
        );
        let end = crate::widgets::library::actions::char_index_to_byte_index(
            &self.content,
            self.content_cursor + 1,
        );
        self.content.drain(start..end);
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn move_content_horizontal(&mut self, delta: isize) {
        let max = self.content.chars().count() as isize;
        let next = (self.content_cursor as isize + delta).clamp(0, max);
        self.content_cursor = next as usize;
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn move_content_vertical(&mut self, delta_lines: isize) {
        let starts = crate::widgets::library::actions::line_start_positions(&self.content);
        let (line_index, column) = crate::widgets::library::actions::line_col_for_char_index(
            &self.content,
            self.content_cursor,
        );
        let goal = self.content_cursor_goal.unwrap_or(column);
        let next_line = (line_index as isize + delta_lines)
            .clamp(0, starts.len().saturating_sub(1) as isize) as usize;
        self.content_cursor = crate::widgets::library::actions::char_index_for_line_col(
            &self.content,
            next_line,
            goal,
        );
        self.content_cursor_goal = Some(goal);
        self.follow_content_cursor();
    }

    fn move_content_to_line_edge(&mut self, start: bool) {
        let (line_index, _) = crate::widgets::library::actions::line_col_for_char_index(
            &self.content,
            self.content_cursor,
        );
        let target_column = if start {
            0
        } else {
            crate::widgets::library::actions::line_lengths(&self.content)
                .get(line_index)
                .copied()
                .unwrap_or_default()
        };
        self.content_cursor = crate::widgets::library::actions::char_index_for_line_col(
            &self.content,
            line_index,
            target_column,
        );
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn current_content_line(&self) -> usize {
        crate::widgets::library::actions::line_col_for_char_index(
            &self.content,
            self.content_cursor,
        )
        .0
    }

    fn content_line_count(&self) -> usize {
        crate::widgets::library::actions::line_start_positions(&self.content)
            .len()
            .max(1)
    }

    fn follow_content_cursor(&mut self) {
        self.content_scroll = self.current_content_line().saturating_sub(2);
    }

    pub(crate) fn footer_text(&self) -> &'static str {
        if self.selector.is_some() {
            "j/k Move   ↑/↓ Move   Enter Save   Esc Cancel"
        } else if self.mode == LibraryEditorMode::Edit {
            LIBRARY_EDIT_MODAL_FOOTER
        } else {
            LIBRARY_CREATE_MODAL_FOOTER
        }
    }

    pub(crate) fn selector(&self) -> Option<&LibrarySelectState> {
        self.selector.as_ref()
    }

    pub(crate) fn visible_fields(&self) -> &'static [LibraryModalField] {
        if self.is_script_kind() {
            &SCRIPT_MODAL_FIELDS
        } else {
            &SNIPPET_MODAL_FIELDS
        }
    }

    fn advance_focus(&mut self, forward: bool) {
        let fields = self.visible_fields();
        let current_index = fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        let next_index = if forward {
            (current_index + 1) % fields.len()
        } else if current_index == 0 {
            fields.len().saturating_sub(1)
        } else {
            current_index - 1
        };
        self.focus = fields[next_index];
        self.content_cursor_goal = None;
        if self.focus == LibraryModalField::Content {
            self.follow_content_cursor();
        }
    }

    fn ensure_focus_visible(&mut self) {
        if self.visible_fields().contains(&self.focus) {
            return;
        }

        self.focus = LibraryModalField::TargetOs;
        self.content_cursor_goal = None;
    }

    fn initialize_script_defaults(&mut self) {
        self.interpreter =
            crate::widgets::library::actions::default_script_interpreter_for_target_os(
                &self.target_os,
            );
        self.behavior = ScriptBehavior::Inline;
    }
}

impl LibraryDeleteModalState {
    fn from_item(item: &LibraryTrigger, restore_index: usize) -> Self {
        Self {
            trigger_id: item.id().to_string(),
            name: item.name.clone(),
            selected_yes: true,
            restore_index,
            return_to_editor: None,
            error: None,
        }
    }

    pub(crate) fn trigger_id(&self) -> &str {
        &self.trigger_id
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) const fn selected_yes(&self) -> bool {
        self.selected_yes
    }

    pub(crate) const fn restore_index(&self) -> usize {
        self.restore_index
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LibraryModal {
    Editor(LibraryEditorModalState),
    Export(LibraryExportModalState),
    ExportResult(LibraryExportResultModalState),
    Import(LibraryImportModalState),
    ImportResult(LibraryImportResultModalState),
    ConfirmImportRunVariables(LibraryImportRunVariablesModalState),
    ConfirmDelete(LibraryDeleteModalState),
}

impl LibraryModal {
    pub(crate) fn set_error(&mut self, error: String) {
        match self {
            Self::Editor(state) => state.set_error(error),
            Self::Export(state) => state.set_error(error),
            Self::ExportResult(state) => state.set_error(error),
            Self::Import(state) => state.set_error(error),
            Self::ImportResult(state) => state.set_error(error),
            Self::ConfirmImportRunVariables(state) => state.set_error(error),
            Self::ConfirmDelete(state) => state.set_error(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryExportResultModalState {
    body: String,
}

impl LibraryExportResultModalState {
    fn new(path: &Path, encrypt: bool, include_settings: bool, include_stats: bool) -> Self {
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

    fn set_error(&mut self, _error: String) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryImportResultModalState {
    lines: Vec<String>,
}

impl LibraryImportResultModalState {
    fn from_outcome(outcome: &LibraryImportOutcome) -> Self {
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

    fn set_error(&mut self, _error: String) {}
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct LibraryPageState {
    items: Vec<LibraryTrigger>,
    filtered_indices: Vec<usize>,
    selected: usize,
    search_query: String,
    search_mode: bool,
    pub(crate) modal: Option<LibraryModal>,
    status_message: Option<String>,
    load_error: Option<String>,
}

impl LibraryPageState {
    pub(crate) fn replace_items(&mut self, mut items: Vec<LibraryTrigger>) {
        crate::widgets::library::actions::sort_items(&mut items);
        self.items = items;
        self.load_error = None;
        self.status_message = None;
        self.rebuild_filter();
    }

    pub(crate) fn set_load_error(&mut self, error: String) {
        self.load_error = Some(error);
    }

    pub(crate) fn set_status_message(&mut self, message: String) {
        self.status_message = Some(message);
    }

    pub(crate) fn set_save_error(&mut self, error: String) {
        if let Some(modal) = self.modal.as_mut() {
            modal.set_error(error);
        } else {
            self.status_message = Some(error);
        }
    }

    pub(crate) fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    pub(crate) fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub(crate) fn footer_text(&self) -> &'static str {
        if let Some(modal) = &self.modal {
            match modal {
                LibraryModal::Editor(state) => state.footer_text(),
                LibraryModal::Export(state) => state.footer_text(),
                LibraryModal::ExportResult(state) => state.footer_text(),
                LibraryModal::Import(state) => state.footer_text(),
                LibraryModal::ImportResult(state) => state.footer_text(),
                LibraryModal::ConfirmImportRunVariables(_) => LIBRARY_IMPORT_RUN_VARIABLES_FOOTER,
                LibraryModal::ConfirmDelete(_) => LIBRARY_DELETE_MODAL_FOOTER,
            }
        } else if self.search_mode {
            "Type Search   Enter Finish   Esc Cancel"
        } else {
            LIBRARY_FOOTER
        }
    }

    pub(crate) fn search_query(&self) -> &str {
        &self.search_query
    }

    pub(crate) const fn is_search_active(&self) -> bool {
        self.search_mode
    }

    pub(crate) const fn is_modal_open(&self) -> bool {
        self.modal.is_some()
    }

    pub(crate) const fn modal(&self) -> Option<&LibraryModal> {
        self.modal.as_ref()
    }

    pub(crate) fn open_editor_modal(&mut self, trigger: LibraryTriggerDetail) {
        self.modal = Some(LibraryModal::Editor(LibraryEditorModalState::new_edit(
            trigger,
        )));
    }

    pub(crate) fn open_create_modal(&mut self) {
        self.modal = Some(LibraryModal::Editor(LibraryEditorModalState::new_create()));
    }

    pub(crate) fn open_export_modal(&mut self) {
        match LibraryExportModalState::new() {
            Ok(state) => self.modal = Some(LibraryModal::Export(state)),
            Err(error) => self.set_status_message(error.to_string()),
        }
    }

    pub(crate) fn open_import_modal(&mut self) {
        self.modal = Some(LibraryModal::Import(LibraryImportModalState::new()));
    }

    pub(crate) fn open_export_result_modal(
        &mut self,
        path: &Path,
        encrypt: bool,
        include_settings: bool,
        include_stats: bool,
    ) {
        self.modal = Some(LibraryModal::ExportResult(
            LibraryExportResultModalState::new(path, encrypt, include_settings, include_stats),
        ));
    }

    pub(crate) fn open_import_result_modal(&mut self, outcome: &LibraryImportOutcome) {
        self.modal = Some(LibraryModal::ImportResult(
            LibraryImportResultModalState::from_outcome(outcome),
        ));
    }

    pub(crate) fn open_import_run_variables_modal(
        &mut self,
        prepared: PreparedLibraryImport,
        return_to_import: LibraryImportModalState,
    ) {
        self.modal = Some(LibraryModal::ConfirmImportRunVariables(
            LibraryImportRunVariablesModalState::new(prepared, return_to_import),
        ));
    }

    fn open_delete_modal_for_selected(&mut self) {
        let Some(selected_index) = self.selected_index() else {
            self.load_error = Some("No trigger selected.".to_string());
            return;
        };
        let Some(item) = self.item_at_filtered(selected_index).cloned() else {
            self.load_error = Some("No trigger selected.".to_string());
            return;
        };
        self.modal = Some(LibraryModal::ConfirmDelete(
            LibraryDeleteModalState::from_item(&item, selected_index),
        ));
    }

    pub(crate) fn clear_modal(&mut self) {
        self.modal = None;
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        if self.filtered_indices.is_empty() {
            None
        } else {
            Some(
                self.selected
                    .min(self.filtered_indices.len().saturating_sub(1)),
            )
        }
    }

    pub(crate) fn filtered_len(&self) -> usize {
        self.filtered_indices.len()
    }

    pub(crate) fn item_at_filtered(&self, index: usize) -> Option<&LibraryTrigger> {
        self.filtered_indices
            .get(index)
            .and_then(|item_index| self.items.get(*item_index))
    }

    pub(crate) fn select_item_by_id(&mut self, id: &str) {
        if let Some(position) = self
            .filtered_indices
            .iter()
            .position(|item_index| self.items[*item_index].id() == id)
        {
            self.selected = position;
        }
    }

    pub(crate) fn select_after_delete(&mut self, previous_index: usize) {
        if self.filtered_indices.is_empty() {
            self.selected = 0;
        } else {
            self.selected = previous_index.min(self.filtered_indices.len().saturating_sub(1));
        }
    }

    pub(crate) fn empty_state_message(&self) -> Option<&'static str> {
        if self.items.is_empty() {
            Some("No triggers yet.")
        } else if self.filtered_indices.is_empty() {
            Some("No triggers match your search.")
        } else {
            None
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }

        if self.search_mode {
            self.handle_search_key(key);
            return LibraryInteraction::handled();
        }

        self.status_message = None;

        match (key.code, key.modifiers) {
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.search_mode = true;
                LibraryInteraction::handled()
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_selection(1);
                LibraryInteraction::handled()
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_selection(-1);
                LibraryInteraction::handled()
            }
            (KeyCode::Char('n'), KeyModifiers::NONE) => LibraryInteraction::open_create(),
            (KeyCode::Char('i'), KeyModifiers::NONE) => {
                self.open_import_modal();
                LibraryInteraction::handled()
            }
            (KeyCode::Char('x'), KeyModifiers::NONE) => {
                self.open_export_modal();
                LibraryInteraction::handled()
            }
            (KeyCode::Char('d'), KeyModifiers::NONE) => {
                self.open_delete_modal_for_selected();
                LibraryInteraction::handled()
            }
            (KeyCode::Enter, KeyModifiers::NONE) => self
                .selected_item()
                .map(|item| LibraryInteraction::open_selected(item.id().to_string()))
                .unwrap_or_default(),
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        let Some(modal) = self.modal.take() else {
            return LibraryInteraction::handled();
        };

        match modal {
            LibraryModal::Editor(mut state) => {
                let interaction = state.handle_key(key);
                if !interaction.should_close_modal() {
                    self.modal = Some(LibraryModal::Editor(state));
                }
                interaction
            }
            LibraryModal::Export(mut state) => {
                let interaction = state.handle_key(key);
                if !interaction.should_close_modal() {
                    self.modal = Some(LibraryModal::Export(state));
                }
                interaction
            }
            LibraryModal::ExportResult(state) => match (key.code, key.modifiers) {
                (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Esc, KeyModifiers::NONE) => {
                    LibraryInteraction::close()
                }
                _ => {
                    self.modal = Some(LibraryModal::ExportResult(state));
                    LibraryInteraction::handled()
                }
            },
            LibraryModal::Import(mut state) => {
                let interaction = state.handle_key(key);
                if !interaction.should_close_modal() {
                    self.modal = Some(LibraryModal::Import(state));
                }
                interaction
            }
            LibraryModal::ImportResult(state) => match (key.code, key.modifiers) {
                (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Esc, KeyModifiers::NONE) => {
                    LibraryInteraction::close()
                }
                _ => {
                    self.modal = Some(LibraryModal::ImportResult(state));
                    LibraryInteraction::handled()
                }
            },
            LibraryModal::ConfirmImportRunVariables(state) => match (key.code, key.modifiers) {
                (KeyCode::Char('y'), KeyModifiers::NONE)
                | (KeyCode::Char('Y'), KeyModifiers::NONE)
                | (KeyCode::Enter, KeyModifiers::NONE) => {
                    let prepared = state.prepared.clone();
                    self.modal = Some(LibraryModal::ConfirmImportRunVariables(state));
                    LibraryInteraction::import(prepared)
                }
                (KeyCode::Char('n'), KeyModifiers::NONE)
                | (KeyCode::Char('N'), KeyModifiers::NONE)
                | (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.modal = Some(LibraryModal::Import(state.return_to_import));
                    LibraryInteraction::handled()
                }
                _ => {
                    self.modal = Some(LibraryModal::ConfirmImportRunVariables(state));
                    LibraryInteraction::handled()
                }
            },
            LibraryModal::ConfirmDelete(mut state) => match (key.code, key.modifiers) {
                (KeyCode::Left, KeyModifiers::NONE) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                    state.selected_yes = true;
                    self.modal = Some(LibraryModal::ConfirmDelete(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Right, KeyModifiers::NONE) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
                    state.selected_yes = false;
                    self.modal = Some(LibraryModal::ConfirmDelete(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    if state.selected_yes() {
                        let interaction = LibraryInteraction::delete(PendingLibraryDelete {
                            trigger_id: state.trigger_id().to_string(),
                            restore_index: state.restore_index(),
                        });
                        self.modal = Some(LibraryModal::ConfirmDelete(state));
                        interaction
                    } else {
                        self.restore_delete_modal_parent(state);
                        LibraryInteraction::handled()
                    }
                }
                (KeyCode::Char('y'), KeyModifiers::NONE)
                | (KeyCode::Char('Y'), KeyModifiers::NONE) => {
                    let interaction = LibraryInteraction::delete(PendingLibraryDelete {
                        trigger_id: state.trigger_id().to_string(),
                        restore_index: state.restore_index(),
                    });
                    self.modal = Some(LibraryModal::ConfirmDelete(state));
                    interaction
                }
                (KeyCode::Char('n'), KeyModifiers::NONE)
                | (KeyCode::Char('N'), KeyModifiers::NONE)
                | (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.restore_delete_modal_parent(state);
                    LibraryInteraction::handled()
                }
                _ => {
                    self.modal = Some(LibraryModal::ConfirmDelete(state));
                    LibraryInteraction::handled()
                }
            },
        }
    }

    fn restore_delete_modal_parent(&mut self, state: LibraryDeleteModalState) {
        if let Some(editor) = state.return_to_editor {
            self.modal = Some(LibraryModal::Editor(editor));
        } else {
            self.modal = None;
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => self.search_mode = false,
            (KeyCode::Enter, KeyModifiers::NONE) => self.search_mode = false,
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.search_query.pop();
                self.rebuild_filter();
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_query.push(ch);
                self.rebuild_filter();
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let Some(current) = self.selected_index() else {
            self.selected = 0;
            return;
        };

        let max_index = self.filtered_indices.len().saturating_sub(1) as isize;
        let next = (current as isize + delta).clamp(0, max_index);
        self.selected = next as usize;
    }

    fn rebuild_filter(&mut self) {
        let previously_selected = self.selected_item().cloned();
        self.filtered_indices = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.matches_query(&self.search_query).then_some(index))
            .collect();

        if self.filtered_indices.is_empty() {
            self.selected = 0;
            return;
        }

        if let Some(previous_item) = previously_selected
            && let Some(position) = self
                .filtered_indices
                .iter()
                .position(|item_index| self.items[*item_index] == previous_item)
        {
            self.selected = position;
            return;
        }

        self.selected = 0;
    }

    fn selected_item(&self) -> Option<&LibraryTrigger> {
        self.selected_index()
            .and_then(|selected| self.item_at_filtered(selected))
    }
}
