use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::{
    db::crud::{
        AutomationListItem, AutomationRow, ExistingAutomationUpdate, NewAutomation,
        SUPPORTED_TARGET_OS_VALUES, TriggerType, create_automation, delete_automation,
        update_existing_automation,
    },
    engine::shell::{ScriptBehavior, ScriptInterpreter, decompress},
    exchange::{
        ExchangeFormat, ExchangePayload, ExportOptions, ImportConflictAction, ImportMetricsMode,
        ImportOptions, decode_exchange_blob, detect_exchange_format, encode_exchange_blob,
        export_automations, import_payload_transactionally, payload_contains_run_variables,
        resolve_export_path,
    },
};

const LIBRARY_FOOTER: &str =
    "/ Search   n New   i Import   x Export   d Delete   Enter Edit   q Quit";
const LIBRARY_EDIT_MODAL_FOOTER: &str = "Ctrl+S Save   Esc Cancel   Tab Next   Shift+Tab Prev";
const LIBRARY_CREATE_MODAL_FOOTER: &str = "Ctrl+S Save   Esc Cancel   Tab Next   Shift+Tab Prev";
const LIBRARY_EXPORT_MODAL_FOOTER: &str = "Ctrl+S Export   Esc Cancel   Tab Next   Shift+Tab Prev";
const LIBRARY_EXPORT_PASSWORD_FOOTER: &str =
    "Ctrl+S Export   Esc Cancel   Tab Next   Shift+Tab Prev   Enter Show/Hide";
const LIBRARY_IMPORT_MODAL_FOOTER: &str = "Ctrl+S Import   Esc Cancel   Tab Next   Shift+Tab Prev";
const LIBRARY_IMPORT_PASSWORD_FOOTER: &str =
    "Ctrl+S Import   Esc Cancel   Tab Next   Shift+Tab Prev   Enter Show/Hide";
const LIBRARY_IMPORT_RESULT_FOOTER: &str = "Enter Close   Esc Close";
const LIBRARY_EXPORT_RESULT_FOOTER: &str = "Enter Close   Esc Close";
const LIBRARY_DELETE_MODAL_FOOTER: &str = "Esc Cancel";
const LIBRARY_IMPORT_RUN_VARIABLES_FOOTER: &str = "y Continue   n Cancel   Esc Cancel";
const DEFAULT_SCRIPT_FALLBACK: &str = "Script content unavailable.";
const DEFAULT_OUTPUT_FALLBACK: &str = "No output available.";
const SCRIPT_LANGUAGE_OPTIONS: [ScriptInterpreter; 6] = [
    ScriptInterpreter::Bash,
    ScriptInterpreter::PowerShell,
    ScriptInterpreter::Python,
    ScriptInterpreter::Node,
    ScriptInterpreter::NodeEsm,
    ScriptInterpreter::Cmd,
];
const SCRIPT_MODE_OPTIONS: [ScriptBehavior; 2] = [ScriptBehavior::Inline, ScriptBehavior::Silent];
const EXPORT_ENCRYPTION_OPTIONS: [LibraryExportModalField; 6] = [
    LibraryExportModalField::Path,
    LibraryExportModalField::Encrypt,
    LibraryExportModalField::Password,
    LibraryExportModalField::PasswordToggle,
    LibraryExportModalField::IncludeSettings,
    LibraryExportModalField::IncludeMetrics,
];
const EXPORT_PLAINTEXT_OPTIONS: [LibraryExportModalField; 4] = [
    LibraryExportModalField::Path,
    LibraryExportModalField::Encrypt,
    LibraryExportModalField::IncludeSettings,
    LibraryExportModalField::IncludeMetrics,
];
const IMPORT_MODAL_FIELDS: [LibraryImportModalField; 6] = [
    LibraryImportModalField::Path,
    LibraryImportModalField::Password,
    LibraryImportModalField::PasswordToggle,
    LibraryImportModalField::IncludeSettings,
    LibraryImportModalField::MetricsMode,
    LibraryImportModalField::ConflictMode,
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

    fn trigger_type(self) -> TriggerType {
        match self {
            Self::Snippet | Self::Script => TriggerType::Word,
            Self::HotkeySnippet | Self::HotkeyScript => TriggerType::Hotkey,
        }
    }

    fn action_type(self) -> &'static str {
        if self.is_script() { "script" } else { "text" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryAutomation {
    id: String,
    name: String,
    trigger: String,
    preview: String,
    kind: LibraryKind,
    target_os: String,
    search_text: String,
    uses: u64,
}

impl LibraryAutomation {
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

impl From<AutomationListItem> for LibraryAutomation {
    fn from(item: AutomationListItem) -> Self {
        let kind = LibraryKind::from_parts(item.trigger_type, item.action_type.as_str());
        let preview = preview_from_item(&item);
        let target_os = display_target_os(&item.target_os).to_string();
        let search_text = build_search_text(&item, kind.label(), &target_os);

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
pub(crate) enum LibraryExportModalField {
    Path,
    Encrypt,
    Password,
    PasswordToggle,
    IncludeSettings,
    IncludeMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryImportModalField {
    Path,
    Password,
    PasswordToggle,
    IncludeSettings,
    MetricsMode,
    ConflictMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryImportConflictMode {
    Skip,
    Overwrite,
}

impl LibraryImportConflictMode {
    const ALL: [Self; 2] = [Self::Skip, Self::Overwrite];

    const fn label(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Overwrite => "overwrite",
        }
    }

    const fn to_action(self) -> ImportConflictAction {
        match self {
            Self::Skip => ImportConflictAction::Skip,
            Self::Overwrite => ImportConflictAction::Overwrite,
        }
    }
}

const SNIPPET_MODAL_FIELDS: [LibraryModalField; 4] = [
    LibraryModalField::Trigger,
    LibraryModalField::Content,
    LibraryModalField::Kind,
    LibraryModalField::TargetOs,
];

const SCRIPT_MODAL_FIELDS: [LibraryModalField; 6] = [
    LibraryModalField::Trigger,
    LibraryModalField::Content,
    LibraryModalField::Kind,
    LibraryModalField::TargetOs,
    LibraryModalField::Language,
    LibraryModalField::Mode,
];

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
    fn new(label: &'static str, value: String) -> Self {
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
    automation_id: String,
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
    show_password: bool,
    include_settings: bool,
    include_metrics: bool,
    focus: LibraryExportModalField,
    error: Option<String>,
}

impl LibraryExportModalState {
    fn new() -> taurine_core::Result<Self> {
        let path = resolve_export_path(None)?.to_string_lossy().into_owned();
        let path_cursor = path.chars().count();

        Ok(Self {
            path,
            path_cursor,
            encrypt: true,
            password: String::new(),
            password_cursor: 0,
            show_password: false,
            include_settings: false,
            include_metrics: false,
            focus: LibraryExportModalField::Path,
            error: None,
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

    pub(crate) fn password_display_value(&self) -> String {
        if self.show_password {
            self.password.clone()
        } else {
            self.password_masked()
        }
    }

    pub(crate) const fn password_cursor(&self) -> usize {
        self.password_cursor
    }

    #[cfg(test)]
    pub(crate) const fn show_password(&self) -> bool {
        self.show_password
    }

    pub(crate) const fn password_toggle_label(&self) -> &'static str {
        if self.show_password { "hide" } else { "show" }
    }

    pub(crate) const fn include_settings(&self) -> bool {
        self.include_settings
    }

    pub(crate) const fn include_metrics(&self) -> bool {
        self.include_metrics
    }

    pub(crate) const fn focus(&self) -> LibraryExportModalField {
        self.focus
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    fn visible_fields(&self) -> &'static [LibraryExportModalField] {
        if self.encrypt {
            &EXPORT_ENCRYPTION_OPTIONS
        } else {
            &EXPORT_PLAINTEXT_OPTIONS
        }
    }

    pub(crate) fn footer_text(&self) -> &'static str {
        if self.encrypt && self.focus == LibraryExportModalField::PasswordToggle {
            LIBRARY_EXPORT_PASSWORD_FOOTER
        } else {
            LIBRARY_EXPORT_MODAL_FOOTER
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        self.error = None;

        if matches!(key.code, KeyCode::Char('s' | 'S'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return match self.build_pending_export() {
                Ok(pending_export) => LibraryInteraction::export(pending_export),
                Err(error) => {
                    self.error = Some(error.to_string());
                    LibraryInteraction::handled()
                }
            };
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
            include_metrics: self.include_metrics,
        })
    }

    fn handle_focused_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match self.focus {
            LibraryExportModalField::Path => self.handle_path_key(key),
            LibraryExportModalField::Encrypt => self.handle_encrypt_key(key),
            LibraryExportModalField::Password => self.handle_password_key(key),
            LibraryExportModalField::PasswordToggle => self.handle_password_toggle_key(key),
            LibraryExportModalField::IncludeSettings => self.handle_include_settings_key(key),
            LibraryExportModalField::IncludeMetrics => self.handle_include_metrics_key(key),
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
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                self.encrypt = !self.encrypt;
                if !self.encrypt {
                    self.show_password = false;
                }
                self.ensure_focus_visible();
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

    fn handle_password_toggle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                self.show_password = !self.show_password;
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_include_settings_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                self.include_settings = !self.include_settings;
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_include_metrics_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                self.include_metrics = !self.include_metrics;
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
        let next_index = if forward {
            (current_index + 1) % fields.len()
        } else if current_index == 0 {
            fields.len().saturating_sub(1)
        } else {
            current_index - 1
        };
        self.focus = fields[next_index];
    }

    fn ensure_focus_visible(&mut self) {
        if self.visible_fields().contains(&self.focus) {
            return;
        }

        self.focus = LibraryExportModalField::IncludeSettings;
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
pub(crate) struct LibraryImportModalState {
    path: String,
    path_cursor: usize,
    password: String,
    password_cursor: usize,
    show_password: bool,
    include_settings: bool,
    metrics_mode: ImportMetricsMode,
    conflict_mode: LibraryImportConflictMode,
    focus: LibraryImportModalField,
    error: Option<String>,
    selector: Option<LibrarySelectState>,
}

impl LibraryImportModalState {
    fn new() -> Self {
        Self {
            path: String::new(),
            path_cursor: 0,
            password: String::new(),
            password_cursor: 0,
            show_password: false,
            include_settings: false,
            metrics_mode: ImportMetricsMode::Ignore,
            conflict_mode: LibraryImportConflictMode::Skip,
            focus: LibraryImportModalField::Path,
            error: None,
            selector: None,
        }
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn path_cursor(&self) -> usize {
        self.path_cursor
    }

    pub(crate) fn password_display_value(&self) -> String {
        if self.show_password {
            self.password.clone()
        } else {
            "*".repeat(self.password.chars().count())
        }
    }

    pub(crate) const fn password_cursor(&self) -> usize {
        self.password_cursor
    }

    #[cfg(test)]
    pub(crate) const fn show_password(&self) -> bool {
        self.show_password
    }

    pub(crate) const fn password_toggle_label(&self) -> &'static str {
        if self.show_password { "hide" } else { "show" }
    }

    pub(crate) const fn include_settings(&self) -> bool {
        self.include_settings
    }

    #[cfg(test)]
    pub(crate) const fn metrics_mode(&self) -> ImportMetricsMode {
        self.metrics_mode
    }

    #[cfg(test)]
    pub(crate) const fn conflict_mode(&self) -> LibraryImportConflictMode {
        self.conflict_mode
    }

    pub(crate) const fn focus(&self) -> LibraryImportModalField {
        self.focus
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub(crate) fn footer_text(&self) -> &'static str {
        if self.selector.is_some() {
            "j/k Move   ↑/↓ Move   Enter Save   Esc Cancel"
        } else if self.focus == LibraryImportModalField::PasswordToggle {
            LIBRARY_IMPORT_PASSWORD_FOOTER
        } else {
            LIBRARY_IMPORT_MODAL_FOOTER
        }
    }

    pub(crate) fn selector(&self) -> Option<&LibrarySelectState> {
        self.selector.as_ref()
    }

    pub(crate) const fn metrics_mode_label(&self) -> &'static str {
        match self.metrics_mode {
            ImportMetricsMode::Ignore => "ignore",
            ImportMetricsMode::Merge => "merge",
            ImportMetricsMode::Overwrite => "overwrite",
        }
    }

    pub(crate) const fn conflict_mode_label(&self) -> &'static str {
        self.conflict_mode.label()
    }

    fn visible_fields(&self) -> &'static [LibraryImportModalField] {
        &IMPORT_MODAL_FIELDS
    }

    fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        if self.selector.is_some() {
            return self.handle_selector_key(key);
        }

        self.error = None;

        if matches!(key.code, KeyCode::Char('s' | 'S'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return match self.build_pending_prepare() {
                Ok(pending_prepare) => LibraryInteraction::prepare_import(pending_prepare),
                Err(error) => {
                    self.error = Some(error.to_string());
                    LibraryInteraction::handled()
                }
            };
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

    fn build_pending_prepare(&self) -> taurine_core::Result<PendingLibraryImportPrepare> {
        if self.path.trim().is_empty() {
            return Err(taurine_core::Error::Config(
                "Import path is required.".to_string(),
            ));
        }

        Ok(PendingLibraryImportPrepare {
            path: self.path.clone(),
            password: (!self.password.is_empty()).then(|| self.password.clone()),
            options: ImportOptions {
                include_settings: self.include_settings,
                metrics_mode: self.metrics_mode,
            },
            conflict_mode: self.conflict_mode,
            return_to_modal: self.clone(),
        })
    }

    fn handle_focused_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match self.focus {
            LibraryImportModalField::Path => self.handle_path_key(key),
            LibraryImportModalField::Password => self.handle_password_key(key),
            LibraryImportModalField::PasswordToggle => self.handle_password_toggle_key(key),
            LibraryImportModalField::IncludeSettings => self.handle_include_settings_key(key),
            LibraryImportModalField::MetricsMode => self.handle_metrics_mode_key(key),
            LibraryImportModalField::ConflictMode => self.handle_conflict_mode_key(key),
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
                    LibraryImportModalField::MetricsMode => {
                        self.metrics_mode = match selector.selected {
                            0 => ImportMetricsMode::Ignore,
                            1 => ImportMetricsMode::Merge,
                            _ => ImportMetricsMode::Overwrite,
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

    fn handle_password_toggle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                self.show_password = !self.show_password;
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_include_settings_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                self.include_settings = !self.include_settings;
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_metrics_mode_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                self.selector = Some(LibrarySelectState {
                    title: "Select Metrics Mode",
                    options: vec![
                        "ignore".to_string(),
                        "merge".to_string(),
                        "overwrite".to_string(),
                    ],
                    selected: match self.metrics_mode {
                        ImportMetricsMode::Ignore => 0,
                        ImportMetricsMode::Merge => 1,
                        ImportMetricsMode::Overwrite => 2,
                    },
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_conflict_mode_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
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
        let next_index = if forward {
            (current_index + 1) % fields.len()
        } else if current_index == 0 {
            fields.len().saturating_sub(1)
        } else {
            current_index - 1
        };
        self.focus = fields[next_index];
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
pub(crate) struct LibraryAutomationDetail {
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

impl LibraryAutomationDetail {
    pub(crate) fn from_row(row: AutomationRow) -> taurine_core::Result<Self> {
        let kind = LibraryKind::from_parts(row.trigger_type, row.action_type.as_str());
        let content = modal_content_from_row(&row, kind)?;
        let metadata_rows = build_metadata_rows(&row);

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
    original: Option<LibraryAutomationDetail>,
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
    fn new_edit(automation: LibraryAutomationDetail) -> Self {
        let trigger_cursor = automation.trigger().chars().count();
        let content_cursor = automation.content().chars().count();
        Self {
            mode: LibraryEditorMode::Edit,
            trigger: automation.trigger().to_string(),
            trigger_cursor,
            content: automation.content().to_string(),
            content_cursor,
            content_cursor_goal: None,
            kind: automation.kind(),
            target_os: automation.target_os_raw().to_string(),
            interpreter: automation.interpreter().unwrap_or_else(|| {
                default_script_interpreter_for_target_os(automation.target_os_raw())
            }),
            behavior: automation.behavior().unwrap_or(ScriptBehavior::Inline),
            focus: LibraryModalField::Trigger,
            content_scroll: 0,
            error: None,
            original: Some(automation),
            selector: None,
        }
    }

    fn new_create() -> Self {
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
            interpreter: default_script_interpreter_for_target_os(SUPPORTED_TARGET_OS_VALUES[0]),
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
        display_target_os(&self.target_os)
    }

    pub(crate) const fn is_script_kind(&self) -> bool {
        self.kind.is_script()
    }

    pub(crate) const fn language_label(&self) -> &'static str {
        interpreter_label(self.interpreter)
    }

    pub(crate) const fn mode_label(&self) -> &'static str {
        behavior_label(self.behavior)
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
            .map(LibraryAutomationDetail::metadata_rows)
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

    fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
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
        let lines = split_lines_with_trailing(&self.content);
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
                    .map(|v| display_target_os(v).to_string())
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
                    .map(|value| interpreter_label(*value).to_string())
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
                    .map(|value| behavior_label(*value).to_string())
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
        let byte_index = char_index_to_byte_index(&self.trigger, self.trigger_cursor);
        self.trigger.insert(byte_index, ch);
        self.trigger_cursor += 1;
    }

    fn backspace_trigger(&mut self) {
        if self.trigger_cursor == 0 {
            return;
        }

        let end = char_index_to_byte_index(&self.trigger, self.trigger_cursor);
        let start = char_index_to_byte_index(&self.trigger, self.trigger_cursor - 1);
        self.trigger.drain(start..end);
        self.trigger_cursor -= 1;
    }

    fn delete_trigger(&mut self) {
        if self.trigger_cursor >= self.trigger.chars().count() {
            return;
        }

        let start = char_index_to_byte_index(&self.trigger, self.trigger_cursor);
        let end = char_index_to_byte_index(&self.trigger, self.trigger_cursor + 1);
        self.trigger.drain(start..end);
    }

    fn insert_content_char(&mut self, ch: char) {
        let byte_index = char_index_to_byte_index(&self.content, self.content_cursor);
        self.content.insert(byte_index, ch);
        self.content_cursor += 1;
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn backspace_content(&mut self) {
        if self.content_cursor == 0 {
            return;
        }

        let end = char_index_to_byte_index(&self.content, self.content_cursor);
        let start = char_index_to_byte_index(&self.content, self.content_cursor - 1);
        self.content.drain(start..end);
        self.content_cursor -= 1;
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn delete_content(&mut self) {
        if self.content_cursor >= self.content.chars().count() {
            return;
        }

        let start = char_index_to_byte_index(&self.content, self.content_cursor);
        let end = char_index_to_byte_index(&self.content, self.content_cursor + 1);
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
        let starts = line_start_positions(&self.content);
        let (line_index, column) = line_col_for_char_index(&self.content, self.content_cursor);
        let goal = self.content_cursor_goal.unwrap_or(column);
        let next_line = (line_index as isize + delta_lines)
            .clamp(0, starts.len().saturating_sub(1) as isize) as usize;
        self.content_cursor = char_index_for_line_col(&self.content, next_line, goal);
        self.content_cursor_goal = Some(goal);
        self.follow_content_cursor();
    }

    fn move_content_to_line_edge(&mut self, start: bool) {
        let (line_index, _) = line_col_for_char_index(&self.content, self.content_cursor);
        let target_column = if start {
            0
        } else {
            line_lengths(&self.content)
                .get(line_index)
                .copied()
                .unwrap_or_default()
        };
        self.content_cursor = char_index_for_line_col(&self.content, line_index, target_column);
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn current_content_line(&self) -> usize {
        line_col_for_char_index(&self.content, self.content_cursor).0
    }

    fn content_line_count(&self) -> usize {
        line_start_positions(&self.content).len().max(1)
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

    fn visible_fields(&self) -> &'static [LibraryModalField] {
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
        self.interpreter = default_script_interpreter_for_target_os(&self.target_os);
        self.behavior = ScriptBehavior::Inline;
    }
}

impl LibraryDeleteModalState {
    fn from_item(item: &LibraryAutomation, restore_index: usize) -> Self {
        Self {
            automation_id: item.id().to_string(),
            name: item.name.clone(),
            selected_yes: true,
            restore_index,
            return_to_editor: None,
            error: None,
        }
    }

    pub(crate) fn automation_id(&self) -> &str {
        &self.automation_id
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
    fn set_error(&mut self, error: String) {
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
pub(crate) enum PendingLibrarySaveMode {
    Update {
        id: String,
        name: String,
        description: Option<String>,
        tags_json: String,
        usage_count: i64,
        last_used_at: Option<i64>,
        interpreter: Option<ScriptInterpreter>,
        behavior: Option<ScriptBehavior>,
    },
    Create,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibrarySave {
    mode: PendingLibrarySaveMode,
    trigger: String,
    pub(crate) content: String,
    kind: LibraryKind,
    target_os: String,
    interpreter: Option<ScriptInterpreter>,
    behavior: Option<ScriptBehavior>,
}

impl PendingLibrarySave {
    #[cfg(test)]
    pub(crate) const fn mode(&self) -> &PendingLibrarySaveMode {
        &self.mode
    }

    pub(crate) fn apply(&self) -> taurine_core::Result<String> {
        let mut conn = taurine_core::db::init::setup()?;

        let automation_id = match &self.mode {
            PendingLibrarySaveMode::Update {
                id,
                name,
                description,
                tags_json,
                usage_count,
                last_used_at,
                interpreter,
                behavior,
            } => {
                update_existing_automation(
                    &mut conn,
                    ExistingAutomationUpdate {
                        id,
                        name,
                        description: description.as_deref(),
                        trigger_type: self.kind.trigger_type(),
                        trigger: &self.trigger,
                        content: &self.content,
                        action_type: self.kind.action_type(),
                        target_os: &self.target_os,
                        tags_json,
                        usage_count: *usage_count,
                        last_used_at: *last_used_at,
                        interpreter: self.interpreter.or(*interpreter),
                        behavior: self.behavior.or(*behavior),
                    },
                )?;
                id.clone()
            }
            PendingLibrarySaveMode::Create => create_automation(
                &mut conn,
                NewAutomation {
                    name: None,
                    description: None,
                    trigger_type: self.kind.trigger_type(),
                    trigger: &self.trigger,
                    content: &self.content,
                    action_type: self.kind.action_type(),
                    target_os: &self.target_os,
                    tags_json: "[]",
                    interpreter: self.interpreter,
                    behavior: self.behavior.or(Some(ScriptBehavior::Inline)),
                },
            )?,
        };

        taurine_core::rpc::notify_daemon_reload();
        Ok(automation_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryDelete {
    pub(crate) automation_id: String,
    pub(crate) restore_index: usize,
}

impl PendingLibraryDelete {
    pub(crate) const fn restore_index(&self) -> usize {
        self.restore_index
    }

    pub(crate) fn apply(&self) -> taurine_core::Result<()> {
        let conn = taurine_core::db::init::setup()?;
        if !delete_automation(&conn, &self.automation_id)? {
            return Err(taurine_core::Error::NotFound(
                "Automation no longer exists.".to_string(),
            ));
        }
        taurine_core::rpc::notify_daemon_reload();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryExport {
    path: String,
    encrypt: bool,
    password: Option<String>,
    include_settings: bool,
    include_metrics: bool,
}

impl PendingLibraryExport {
    pub(crate) fn apply(&self) -> taurine_core::Result<PathBuf> {
        let path = resolve_export_path(Some(PathBuf::from(self.path.as_str())))?;
        let conn = taurine_core::db::init::setup()?;
        let payload = export_automations(
            &conn,
            ExportOptions {
                include_settings: self.include_settings,
                include_metrics: self.include_metrics,
                include_sensitive_settings: false,
            },
        )?;
        let encoded = encode_exchange_blob(&payload, self.encrypt, self.password.as_deref())?;
        std::fs::write(&path, encoded)?;
        Ok(path)
    }

    pub(crate) const fn encrypt(&self) -> bool {
        self.encrypt
    }

    pub(crate) const fn include_settings(&self) -> bool {
        self.include_settings
    }

    pub(crate) const fn include_metrics(&self) -> bool {
        self.include_metrics
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryImportPrepare {
    path: String,
    password: Option<String>,
    options: ImportOptions,
    conflict_mode: LibraryImportConflictMode,
    return_to_modal: LibraryImportModalState,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedLibraryImport {
    path: String,
    payload: ExchangePayload,
    options: ImportOptions,
    conflict_mode: LibraryImportConflictMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryImportOutcome {
    imported: usize,
    imported_settings: bool,
    imported_metrics: bool,
}

impl LibraryImportOutcome {
    #[cfg(test)]
    pub(crate) const fn new(
        imported: usize,
        imported_settings: bool,
        imported_metrics: bool,
    ) -> Self {
        Self {
            imported,
            imported_settings,
            imported_metrics,
        }
    }

    pub(crate) const fn imported(&self) -> usize {
        self.imported
    }

    pub(crate) const fn imported_settings(&self) -> bool {
        self.imported_settings
    }

    pub(crate) const fn imported_metrics(&self) -> bool {
        self.imported_metrics
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryExportResultModalState {
    body: String,
}

impl LibraryExportResultModalState {
    fn new(path: &Path, encrypt: bool, include_settings: bool, include_metrics: bool) -> Self {
        let subject = match (include_settings, include_metrics) {
            (false, false) => "Automations".to_string(),
            (true, false) => "Automations and Settings".to_string(),
            (false, true) => "Automations and Metrics".to_string(),
            (true, true) => "Automations, Settings and Metrics".to_string(),
        };

        let body = match (include_settings, include_metrics, encrypt) {
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
        let mut lines = vec![format!("Imported {} automation(s).", outcome.imported())];
        if outcome.imported_settings() {
            lines.push("Settings imported.".to_string());
        }
        if outcome.imported_metrics() {
            lines.push("Metrics updated.".to_string());
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LibraryImportPreparedResult {
    NeedsRunVariableConfirmation {
        prepared: PreparedLibraryImport,
        return_to_modal: Box<LibraryImportModalState>,
    },
    Imported(LibraryImportOutcome),
}

impl PendingLibraryImportPrepare {
    pub(crate) fn prepare(&self) -> taurine_core::Result<LibraryImportPreparedResult> {
        let path = self.path.trim();
        let bytes = std::fs::read(path)?;
        let format = detect_exchange_format(&bytes)?;
        if format == ExchangeFormat::Encrypted && self.password.as_deref().unwrap_or("").is_empty()
        {
            return Err(taurine_core::Error::Config(
                "A password is required to import TAU1 exchange files.".to_string(),
            ));
        }

        let payload = decode_exchange_blob(&bytes, self.password.as_deref())?;
        let prepared = PreparedLibraryImport {
            path: path.to_string(),
            payload,
            options: self.options,
            conflict_mode: self.conflict_mode,
        };

        if payload_contains_run_variables(&prepared.payload) {
            Ok(LibraryImportPreparedResult::NeedsRunVariableConfirmation {
                prepared,
                return_to_modal: Box::new(self.return_to_modal.clone()),
            })
        } else {
            let outcome = prepared.apply()?;
            Ok(LibraryImportPreparedResult::Imported(outcome))
        }
    }
}

impl PreparedLibraryImport {
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn apply(&self) -> taurine_core::Result<LibraryImportOutcome> {
        let mut conn = taurine_core::db::init::setup()?;
        let imported =
            import_payload_transactionally(&mut conn, &self.payload, self.options, |_, _| {
                Ok(self.conflict_mode.to_action())
            })?;

        Ok(LibraryImportOutcome {
            imported,
            imported_settings: self.options.include_settings && self.payload.settings.is_some(),
            imported_metrics: self.options.metrics_mode != ImportMetricsMode::Ignore
                && self.payload.metrics.is_some(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LibraryOpenRequest {
    Selected(String),
    Create,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct LibraryInteraction {
    open_request: Option<LibraryOpenRequest>,
    pending_save: Option<PendingLibrarySave>,
    pending_delete: Option<PendingLibraryDelete>,
    pending_export: Option<PendingLibraryExport>,
    pending_import_prepare: Option<PendingLibraryImportPrepare>,
    pending_import_commit: Option<PreparedLibraryImport>,
    close_modal: bool,
}

impl LibraryInteraction {
    pub(crate) fn into_open_request(self) -> Option<LibraryOpenRequest> {
        self.open_request
    }

    pub(crate) const fn pending_save(&self) -> Option<&PendingLibrarySave> {
        self.pending_save.as_ref()
    }

    pub(crate) const fn pending_delete(&self) -> Option<&PendingLibraryDelete> {
        self.pending_delete.as_ref()
    }

    pub(crate) const fn pending_export(&self) -> Option<&PendingLibraryExport> {
        self.pending_export.as_ref()
    }

    pub(crate) const fn pending_import_prepare(&self) -> Option<&PendingLibraryImportPrepare> {
        self.pending_import_prepare.as_ref()
    }

    pub(crate) const fn pending_import_commit(&self) -> Option<&PreparedLibraryImport> {
        self.pending_import_commit.as_ref()
    }

    pub(crate) const fn should_close_modal(&self) -> bool {
        self.close_modal
    }

    fn handled() -> Self {
        Self::default()
    }

    fn open_selected(id: String) -> Self {
        Self {
            open_request: Some(LibraryOpenRequest::Selected(id)),
            pending_save: None,
            pending_delete: None,
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    fn open_create() -> Self {
        Self {
            open_request: Some(LibraryOpenRequest::Create),
            pending_save: None,
            pending_delete: None,
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    fn save(pending_save: PendingLibrarySave) -> Self {
        Self {
            open_request: None,
            pending_save: Some(pending_save),
            pending_delete: None,
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    fn delete(pending_delete: PendingLibraryDelete) -> Self {
        Self {
            open_request: None,
            pending_save: None,
            pending_delete: Some(pending_delete),
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    fn export(pending_export: PendingLibraryExport) -> Self {
        Self {
            open_request: None,
            pending_save: None,
            pending_delete: None,
            pending_export: Some(pending_export),
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    fn prepare_import(pending_import_prepare: PendingLibraryImportPrepare) -> Self {
        Self {
            open_request: None,
            pending_save: None,
            pending_delete: None,
            pending_export: None,
            pending_import_prepare: Some(pending_import_prepare),
            pending_import_commit: None,
            close_modal: false,
        }
    }

    fn import(prepared: PreparedLibraryImport) -> Self {
        Self {
            open_request: None,
            pending_save: None,
            pending_delete: None,
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: Some(prepared),
            close_modal: false,
        }
    }

    fn close() -> Self {
        Self {
            open_request: None,
            pending_save: None,
            pending_delete: None,
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct LibraryPageState {
    items: Vec<LibraryAutomation>,
    filtered_indices: Vec<usize>,
    selected: usize,
    search_query: String,
    search_mode: bool,
    modal: Option<LibraryModal>,
    status_message: Option<String>,
    load_error: Option<String>,
}

impl LibraryPageState {
    pub(crate) fn replace_items(&mut self, mut items: Vec<LibraryAutomation>) {
        sort_items(&mut items);
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

    pub(crate) fn open_editor_modal(&mut self, automation: LibraryAutomationDetail) {
        self.modal = Some(LibraryModal::Editor(LibraryEditorModalState::new_edit(
            automation,
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
        include_metrics: bool,
    ) {
        self.modal = Some(LibraryModal::ExportResult(
            LibraryExportResultModalState::new(path, encrypt, include_settings, include_metrics),
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
            self.load_error = Some("No automation selected.".to_string());
            return;
        };
        let Some(item) = self.item_at_filtered(selected_index).cloned() else {
            self.load_error = Some("No automation selected.".to_string());
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

    pub(crate) fn item_at_filtered(&self, index: usize) -> Option<&LibraryAutomation> {
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
            Some("No automations yet.")
        } else if self.filtered_indices.is_empty() {
            Some("No automations match your search.")
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
                            automation_id: state.automation_id().to_string(),
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
                        automation_id: state.automation_id().to_string(),
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

    fn selected_item(&self) -> Option<&LibraryAutomation> {
        self.selected_index()
            .and_then(|selected| self.item_at_filtered(selected))
    }
}

fn sort_items(items: &mut [LibraryAutomation]) {
    items.sort_by(|left, right| {
        let left_trigger = left.trigger.to_ascii_lowercase();
        let right_trigger = right.trigger.to_ascii_lowercase();

        left_trigger
            .cmp(&right_trigger)
            .then_with(|| left.kind_label().cmp(right.kind_label()))
            .then_with(|| left.target_os.cmp(&right.target_os))
            .then_with(|| left.preview.cmp(&right.preview))
    });
}

fn char_index_to_byte_index(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(value.len())
}

fn split_lines_with_trailing(value: &str) -> Vec<&str> {
    if value.is_empty() {
        return vec![""];
    }

    value.split('\n').collect()
}

fn line_start_positions(value: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut char_index = 0usize;
    for ch in value.chars() {
        char_index += 1;
        if ch == '\n' {
            starts.push(char_index);
        }
    }
    starts
}

fn line_lengths(value: &str) -> Vec<usize> {
    split_lines_with_trailing(value)
        .into_iter()
        .map(|line| line.chars().count())
        .collect()
}

pub(crate) fn line_col_for_char_index(value: &str, char_index: usize) -> (usize, usize) {
    let starts = line_start_positions(value);
    let lengths = line_lengths(value);
    let safe_index = char_index.min(value.chars().count());

    for (line_index, start) in starts.iter().enumerate().rev() {
        if safe_index >= *start {
            let column = safe_index.saturating_sub(*start).min(lengths[line_index]);
            return (line_index, column);
        }
    }

    (0, safe_index)
}

fn char_index_for_line_col(value: &str, line_index: usize, column: usize) -> usize {
    let starts = line_start_positions(value);
    let lengths = line_lengths(value);
    let safe_line = line_index.min(starts.len().saturating_sub(1));
    starts[safe_line] + column.min(lengths[safe_line])
}

fn preview_from_item(item: &AutomationListItem) -> String {
    if let Some(description) = normalized_preview_text(item.description.as_deref())
        && !is_script_placeholder(&description)
    {
        return description;
    }

    if item.action_type.eq_ignore_ascii_case("script") {
        if let Some(script_content) = normalized_preview_text(item.script_content.as_deref()) {
            return script_content;
        }

        if let Some(output) = normalized_preview_text(Some(item.output.as_str()))
            && !is_script_placeholder(&output)
        {
            return output;
        }

        return DEFAULT_SCRIPT_FALLBACK.to_string();
    }

    if let Some(output) = normalized_preview_text(Some(item.output.as_str())) {
        return output;
    }

    if let Some(script_content) = normalized_preview_text(item.script_content.as_deref()) {
        return script_content;
    }

    "No preview available.".to_string()
}

fn modal_content_from_row(row: &AutomationRow, kind: LibraryKind) -> taurine_core::Result<String> {
    if kind.is_script() {
        if let Some(script_content) = load_script_content(row)? {
            return Ok(script_content);
        }

        if let Some(output) = normalized_modal_text(Some(row.output.as_str()))
            && !is_script_placeholder(&output)
        {
            return Ok(output);
        }

        return Ok(DEFAULT_SCRIPT_FALLBACK.to_string());
    }

    Ok(normalized_modal_text(Some(row.output.as_str()))
        .unwrap_or_else(|| DEFAULT_OUTPUT_FALLBACK.to_string()))
}

fn build_metadata_rows(row: &AutomationRow) -> Vec<LibraryMetadataRow> {
    let mut rows = Vec::new();

    rows.push(LibraryMetadataRow::new(
        "Uses",
        format_usage_count(row.usage_count.max(0) as u64),
    ));

    if let Some(last_used_at) = row.last_used_at.and_then(format_relative_time) {
        rows.push(LibraryMetadataRow::new("Last used", last_used_at));
    }

    if let Some(created_at) = format_relative_time(row.created_at) {
        rows.push(LibraryMetadataRow::new("Created", created_at));
    }

    if let Some(updated_at) = format_relative_time(row.updated_at) {
        rows.push(LibraryMetadataRow::new("Updated", updated_at));
    }

    rows
}

fn load_script_content(row: &AutomationRow) -> taurine_core::Result<Option<String>> {
    row.script_binary
        .as_deref()
        .map(decompress)
        .transpose()
        .map(|content| content.and_then(|content| normalized_modal_text(Some(content.as_str()))))
}

fn build_search_text(
    item: &AutomationListItem,
    kind_label: &str,
    display_target_os: &str,
) -> String {
    let mut parts = vec![
        item.name.as_str(),
        item.trigger.as_str(),
        item.output.as_str(),
        kind_label,
        display_target_os,
        item.target_os.as_str(),
    ];

    if let Some(description) = item.description.as_deref() {
        parts.push(description);
    }

    if let Some(script_content) = item.script_content.as_deref() {
        parts.push(script_content);
    }

    parts
        .into_iter()
        .filter_map(|part| normalized_preview_text(Some(part)))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalized_preview_text(value: Option<&str>) -> Option<String> {
    let value = value?;
    let first_non_empty = value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(value.trim());

    let collapsed = first_non_empty
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

fn normalized_modal_text(value: Option<&str>) -> Option<String> {
    let value = value?.replace("\r\n", "\n");
    (!value.is_empty()).then_some(value)
}

fn is_script_placeholder(value: &str) -> bool {
    let normalized = value.trim();
    (normalized.starts_with("[Script:") && normalized.ends_with(']'))
        || normalized
            .to_ascii_lowercase()
            .starts_with("shell script (")
}

fn display_target_os(target_os: &str) -> &str {
    match target_os {
        "all" => "all",
        "win" => "windows",
        "mac" => "macos",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        _ => target_os,
    }
}

const fn interpreter_label(interpreter: ScriptInterpreter) -> &'static str {
    match interpreter {
        ScriptInterpreter::Bash => "bash",
        ScriptInterpreter::PowerShell => "powershell",
        ScriptInterpreter::Python => "python",
        ScriptInterpreter::Node => "node",
        ScriptInterpreter::NodeEsm => "node-esm",
        ScriptInterpreter::Cmd => "cmd",
    }
}

const fn behavior_label(behavior: ScriptBehavior) -> &'static str {
    match behavior {
        ScriptBehavior::Inline => "inline",
        ScriptBehavior::Silent => "silent",
    }
}

fn default_script_interpreter_for_target_os(target_os: &str) -> ScriptInterpreter {
    match target_os {
        "win" => ScriptInterpreter::PowerShell,
        "linux" | "mac" => ScriptInterpreter::Bash,
        _ if cfg!(windows) => ScriptInterpreter::PowerShell,
        _ => ScriptInterpreter::Bash,
    }
}

fn format_usage_count(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }

    formatted.chars().rev().collect()
}

fn format_relative_time(timestamp: i64) -> Option<String> {
    if timestamp <= 0 {
        return None;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)?;
    let diff = now.saturating_sub(timestamp);

    if diff < 60 {
        Some("just now".to_string())
    } else {
        let minutes = diff / 60;
        if minutes < 60 {
            return Some(format!("{minutes}m ago"));
        }

        let hours = minutes / 60;
        if hours < 24 {
            return Some(format!("{hours}h ago"));
        }

        let days = hours / 24;
        if days < 30 {
            return Some(format!("{days}d ago"));
        }

        let months = days / 30;
        if months < 12 {
            return Some(format!("{months}mo ago"));
        }

        Some(format!("{}y ago", days / 365))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn list_item(
        id: &str,
        description: Option<&str>,
        trigger_type: TriggerType,
        trigger: &str,
        output: &str,
        action_type: &str,
        target_os: &str,
        usage_count: i64,
        script_content: Option<&str>,
    ) -> AutomationListItem {
        AutomationListItem {
            id: id.to_string(),
            name: trigger.to_string(),
            description: description.map(str::to_string),
            trigger_type,
            trigger: trigger.to_string(),
            output: output.to_string(),
            action_type: action_type.to_string(),
            target_os: target_os.to_string(),
            only_apps: None,
            except_apps: None,
            usage_count,
            last_used_at: None,
            created_at: 0,
            script_content: script_content.map(str::to_string),
            interpreter: None,
            behavior: None,
        }
    }

    fn automation_row(
        trigger_type: TriggerType,
        trigger: &str,
        output: &str,
        action_type: &str,
        target_os: &str,
        usage_count: i64,
        script_content: Option<&str>,
    ) -> AutomationRow {
        AutomationRow {
            id: format!("automation-{trigger}"),
            name: format!("Automation {trigger}"),
            description: Some("Open Reddit".to_string()),
            trigger_type,
            trigger: trigger.to_string(),
            output: output.to_string(),
            action_type: action_type.to_string(),
            target_os: target_os.to_string(),
            only_apps: None,
            except_apps: None,
            tags: "[]".to_string(),
            usage_count,
            last_used_at: Some(1),
            created_at: 1,
            updated_at: 1,
            version: 1,
            is_deleted: false,
            is_synced: true,
            is_enabled: true,
            interpreter: Some(ScriptInterpreter::PowerShell),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: script_content
                .map(|content| taurine_core::engine::shell::compress(content).unwrap()),
        }
    }

    fn sample_state() -> LibraryPageState {
        let mut state = LibraryPageState::default();
        state.replace_items(vec![
            LibraryAutomation::from(list_item(
                "id-gm",
                None,
                TriggerType::Word,
                "gm",
                "Good Morning",
                "text",
                "all",
                9,
                None,
            )),
            LibraryAutomation::from(list_item(
                "id-deploy",
                None,
                TriggerType::Word,
                "deploy",
                "[Script: bash]",
                "script",
                "linux",
                4,
                Some("npm run build && npm publish"),
            )),
            LibraryAutomation::from(list_item(
                "id-alt+r",
                Some("Open Reddit"),
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            )),
        ]);
        state
    }

    #[test]
    fn word_text_maps_to_snippet() {
        let item = LibraryAutomation::from(list_item(
            "id-gm",
            None,
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ));
        assert_eq!(item.kind_label(), "snippet");
    }

    #[test]
    fn word_script_maps_to_script() {
        let item = LibraryAutomation::from(list_item(
            "id-deploy",
            None,
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "all",
            4,
            Some("npm publish"),
        ));
        assert_eq!(item.kind_label(), "script");
    }

    #[test]
    fn hotkey_text_maps_to_hotkey_snippet() {
        let item = LibraryAutomation::from(list_item(
            "id-thanks",
            None,
            TriggerType::Hotkey,
            "alt+t",
            "Thanks!",
            "text",
            "all",
            12,
            None,
        ));
        assert_eq!(item.kind_label(), "hotkey snippet");
    }

    #[test]
    fn hotkey_script_maps_to_hotkey_script() {
        let item = LibraryAutomation::from(list_item(
            "id-alt+r",
            None,
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ));
        assert_eq!(item.kind_label(), "hotkey script");
    }

    #[test]
    fn preview_prefers_description_before_other_content() {
        let item = LibraryAutomation::from(list_item(
            "id-alt+r",
            Some("Open Reddit"),
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ));

        assert_eq!(item.preview(), "Open Reddit");
    }

    #[test]
    fn placeholder_script_description_does_not_block_real_script_preview() {
        let item = LibraryAutomation::from(list_item(
            "id-alt+r",
            Some("Shell script (CLI argument)"),
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ));

        assert_eq!(item.preview(), "Start-Process https://reddit.com");
    }

    #[test]
    fn preview_falls_back_to_text_output_when_description_is_empty() {
        let item = LibraryAutomation::from(list_item(
            "id-gm",
            Some("   "),
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ));

        assert_eq!(item.preview(), "Good Morning");
    }

    #[test]
    fn preview_falls_back_to_script_content_when_description_is_empty() {
        let item = LibraryAutomation::from(list_item(
            "id-alt+r",
            None,
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ));

        assert_eq!(item.preview(), "Start-Process https://reddit.com");
    }

    #[test]
    fn script_preview_does_not_use_script_language_placeholder() {
        let item = LibraryAutomation::from(list_item(
            "id-deploy",
            None,
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "all",
            4,
            Some("npm run build && npm publish"),
        ));

        assert_ne!(item.preview(), "[Script: bash]");
        assert_eq!(item.preview(), "npm run build && npm publish");
    }

    #[test]
    fn script_preview_does_not_use_shell_script_description_placeholder() {
        let item = LibraryAutomation::from(list_item(
            "id-deploy",
            Some("Shell script (CLI argument)"),
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "all",
            4,
            Some("npm run build && npm publish"),
        ));

        assert_ne!(item.preview(), "Shell script (CLI argument)");
        assert_eq!(item.preview(), "npm run build && npm publish");
    }

    #[test]
    fn empty_script_content_falls_back_safely() {
        let item = LibraryAutomation::from(list_item(
            "id-deploy",
            Some("Shell script (CLI argument)"),
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "all",
            4,
            Some("   "),
        ));

        assert_eq!(item.preview(), DEFAULT_SCRIPT_FALLBACK);
    }

    #[test]
    fn search_matches_trigger() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn search_matches_preview() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "publish".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "deploy");
    }

    #[test]
    fn search_matches_description_when_available() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "open".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
    }

    #[test]
    fn search_matches_name_when_it_differs_from_trigger() {
        let mut state = LibraryPageState::default();
        state.replace_items(vec![LibraryAutomation::from(AutomationListItem {
            id: "id-alt+r".to_string(),
            name: "Reddit opener".to_string(),
            description: Some("Open Reddit".to_string()),
            trigger_type: TriggerType::Hotkey,
            trigger: "alt+r".to_string(),
            output: "[Script: powershell]".to_string(),
            action_type: "script".to_string(),
            target_os: "win".to_string(),
            only_apps: None,
            except_apps: None,
            usage_count: 6,
            last_used_at: None,
            created_at: 0,
            script_content: Some("Start-Process https://reddit.com".to_string()),
            interpreter: None,
            behavior: None,
        })]);
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "reddit opener".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
    }

    #[test]
    fn search_matches_script_content_even_when_description_is_visible() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "start-process".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
    }

    #[test]
    fn search_matches_kind_label() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "hotkey".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
    }

    #[test]
    fn search_matches_target_os() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "windows".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().target_os, "windows");
    }

    #[test]
    fn search_is_case_insensitive() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "GOOD".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::SHIFT));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn selection_clamps_at_bounds() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(state.selected_index(), Some(0));

        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

        assert_eq!(state.selected_index(), Some(2));
    }

    #[test]
    fn selection_moves_to_first_match_when_filter_removes_selected_item() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "good".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.selected_index(), Some(0));
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn empty_list_reports_empty_state() {
        let state = LibraryPageState::default();
        assert_eq!(state.empty_state_message(), Some("No automations yet."));
    }

    #[test]
    fn no_match_search_reports_no_match_state() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "zzz".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(
            state.empty_state_message(),
            Some("No automations match your search.")
        );
    }

    #[test]
    fn metadata_uses_double_slash_separator() {
        let item = LibraryAutomation::from(list_item(
            "id-gm",
            None,
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ));

        assert_eq!(item.metadata_label(), "all // 9 uses");
    }

    #[test]
    fn normalized_modal_text_preserves_meaningful_outer_whitespace() {
        assert_eq!(
            normalized_modal_text(Some("  padded body  ")).as_deref(),
            Some("  padded body  ")
        );
        assert_eq!(
            normalized_modal_text(Some("first\r\nsecond")).as_deref(),
            Some("first\nsecond")
        );
    }

    #[test]
    fn kind_selector_uses_kind_title() {
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();
        let mut state = sample_state();
        state.open_editor_modal(detail);

        state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let Some(LibraryModal::Editor(modal)) = state.modal() else {
            panic!("expected editor modal");
        };
        assert_eq!(
            modal.selector().map(LibrarySelectState::title),
            Some("Select Kind")
        );
    }

    #[test]
    fn target_os_selector_uses_target_os_title() {
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();
        let mut state = sample_state();
        state.open_editor_modal(detail);

        state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let Some(LibraryModal::Editor(modal)) = state.modal() else {
            panic!("expected editor modal");
        };
        assert_eq!(
            modal.selector().map(LibrarySelectState::title),
            Some("Select Target OS")
        );
    }

    #[test]
    fn pressing_enter_requests_selected_automation_modal() {
        let mut state = sample_state();
        let expected_id = state
            .selected_index()
            .and_then(|index| state.item_at_filtered(index))
            .map(|item| item.id().to_string());

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            interaction.into_open_request(),
            expected_id.map(LibraryOpenRequest::Selected)
        );
    }

    #[test]
    fn pressing_n_opens_editor_modal_in_create_mode() {
        let mut state = sample_state();

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

        assert_eq!(
            interaction.into_open_request(),
            Some(LibraryOpenRequest::Create)
        );
    }

    #[test]
    fn pressing_x_opens_export_modal() {
        let mut state = sample_state();

        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        assert!(matches!(state.modal(), Some(LibraryModal::Export(_))));
    }

    #[test]
    fn pressing_i_opens_import_modal() {
        let mut state = sample_state();

        state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        assert!(matches!(state.modal(), Some(LibraryModal::Import(_))));
    }

    #[test]
    fn import_modal_defaults_match_current_behavior() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        let Some(LibraryModal::Import(modal)) = state.modal() else {
            panic!("expected import modal");
        };
        assert_eq!(modal.path(), "");
        assert_eq!(modal.password_display_value(), "");
        assert!(!modal.show_password());
        assert!(!modal.include_settings());
        assert_eq!(modal.metrics_mode(), ImportMetricsMode::Ignore);
        assert_eq!(modal.conflict_mode(), LibraryImportConflictMode::Skip);
        assert_eq!(state.footer_text(), LIBRARY_IMPORT_MODAL_FOOTER);
    }

    #[test]
    fn import_modal_requires_non_empty_path() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
            panic!("expected import modal");
        };
        let interaction =
            modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));

        assert!(interaction.pending_import_prepare().is_none());
        assert_eq!(
            modal.error(),
            Some("Configuration error: Import path is required.")
        );
    }

    #[test]
    fn import_modal_password_field_accepts_input_and_stays_masked() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
            panic!("expected import modal");
        };
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for ch in "secret".chars() {
            modal.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(modal.password_display_value(), "******");
        assert!(!modal.show_password());
    }

    #[test]
    fn import_modal_password_toggle_preserves_value() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
            panic!("expected import modal");
        };
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for ch in "secret".chars() {
            modal.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryImportModalField::PasswordToggle);

        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(modal.show_password());
        assert_eq!(modal.password_display_value(), "secret");
        assert_eq!(modal.password_toggle_label(), "hide");
        assert_eq!(state.footer_text(), LIBRARY_IMPORT_PASSWORD_FOOTER);
    }

    #[test]
    fn import_modal_metrics_selector_uses_existing_modes() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
            panic!("expected import modal");
        };
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let selector = modal.selector().expect("metrics selector");
        assert_eq!(selector.title(), "Select Metrics Mode");
        assert_eq!(selector.options, vec!["ignore", "merge", "overwrite"]);
    }

    #[test]
    fn import_modal_conflict_selector_uses_safe_modes() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
            panic!("expected import modal");
        };
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let selector = modal.selector().expect("conflict selector");
        assert_eq!(selector.title(), "Select Conflict Mode");
        assert_eq!(selector.options, vec!["skip", "overwrite"]);
    }

    #[test]
    fn import_modal_owns_input_and_keeps_search_inactive() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert!(!state.is_search_active());
        assert!(matches!(state.modal(), Some(LibraryModal::Import(_))));
    }

    #[test]
    fn import_result_modal_uses_reliable_result_lines() {
        let outcome = LibraryImportOutcome::new(12, true, true);
        let mut state = sample_state();

        state.open_import_result_modal(&outcome);

        let Some(LibraryModal::ImportResult(modal)) = state.modal() else {
            panic!("expected import result modal");
        };
        assert_eq!(modal.lines()[0], "Imported 12 automation(s).");
        assert!(
            modal
                .lines()
                .iter()
                .any(|line| line == "Settings imported.")
        );
        assert!(modal.lines().iter().any(|line| line == "Metrics updated."));
        assert_eq!(state.footer_text(), LIBRARY_IMPORT_RESULT_FOOTER);
    }

    #[test]
    fn import_result_modal_closes_on_enter() {
        let outcome = LibraryImportOutcome::new(3, false, false);
        let mut state = sample_state();
        state.open_import_result_modal(&outcome);

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(interaction.should_close_modal());
    }

    #[test]
    fn import_result_modal_closes_on_escape() {
        let outcome = LibraryImportOutcome::new(3, false, false);
        let mut state = sample_state();
        state.open_import_result_modal(&outcome);

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(interaction.should_close_modal());
    }

    #[test]
    fn import_result_modal_owns_input_and_keeps_search_inactive() {
        let outcome = LibraryImportOutcome::new(3, false, false);
        let mut state = sample_state();
        state.open_import_result_modal(&outcome);

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert!(!state.is_search_active());
        assert!(matches!(state.modal(), Some(LibraryModal::ImportResult(_))));
    }

    #[test]
    fn export_result_modal_body_for_automations_without_encryption_matches_exactly() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");

        state.open_export_result_modal(&path, false, false, false);

        let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
            panic!("expected export result modal");
        };
        assert_eq!(modal.body(), "Automations are exported to: backup.tau");
    }

    #[test]
    fn export_result_modal_body_for_automations_with_encryption_matches_exactly() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");

        state.open_export_result_modal(&path, true, false, false);

        let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
            panic!("expected export result modal");
        };
        assert_eq!(
            modal.body(),
            "Automations are exported to: backup.tau as an encrypted export."
        );
    }

    #[test]
    fn export_result_modal_body_for_automations_and_settings_matches_exactly() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");

        state.open_export_result_modal(&path, false, true, false);

        let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
            panic!("expected export result modal");
        };
        assert_eq!(
            modal.body(),
            "Automations and Settings were exported to: backup.tau"
        );
    }

    #[test]
    fn export_result_modal_body_for_automations_and_settings_with_encryption_matches_exactly() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");

        state.open_export_result_modal(&path, true, true, false);

        let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
            panic!("expected export result modal");
        };
        assert_eq!(
            modal.body(),
            "Automations and Settings were exported to: backup.tau with encryption."
        );
    }

    #[test]
    fn export_result_modal_body_for_automations_and_metrics_matches_exactly() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");

        state.open_export_result_modal(&path, false, false, true);

        let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
            panic!("expected export result modal");
        };
        assert_eq!(
            modal.body(),
            "Automations and Metrics were exported to: backup.tau"
        );
    }

    #[test]
    fn export_result_modal_body_for_automations_and_metrics_with_encryption_matches_exactly() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");

        state.open_export_result_modal(&path, true, false, true);

        let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
            panic!("expected export result modal");
        };
        assert_eq!(
            modal.body(),
            "Automations and Metrics were exported to: backup.tau with encryption."
        );
    }

    #[test]
    fn export_result_modal_body_for_all_export_data_without_encryption_matches_exactly() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");

        state.open_export_result_modal(&path, false, true, true);

        let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
            panic!("expected export result modal");
        };
        assert_eq!(
            modal.body(),
            "Automations, Settings and Metrics were exported to: backup.tau"
        );
    }

    #[test]
    fn export_result_modal_body_for_all_export_data_with_encryption_matches_exactly() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");

        state.open_export_result_modal(&path, true, true, true);

        let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
            panic!("expected export result modal");
        };
        assert_eq!(
            modal.body(),
            "Automations, Settings and Metrics were exported to: backup.tau with encryption."
        );
    }

    #[test]
    fn export_result_modal_does_not_use_separate_encryption_line() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");

        state.open_export_result_modal(&path, true, true, true);

        let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
            panic!("expected export result modal");
        };
        assert!(!modal.body().contains("This export was encrypted."));
        assert_eq!(state.footer_text(), LIBRARY_EXPORT_RESULT_FOOTER);
    }

    #[test]
    fn export_result_modal_closes_on_enter() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");
        state.open_export_result_modal(&path, false, false, false);

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(interaction.should_close_modal());
    }

    #[test]
    fn export_result_modal_closes_on_escape() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");
        state.open_export_result_modal(&path, false, false, false);

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(interaction.should_close_modal());
    }

    #[test]
    fn export_result_modal_owns_input_and_keeps_search_inactive() {
        let mut state = sample_state();
        let path = PathBuf::from("backup.tau");
        state.open_export_result_modal(&path, false, false, false);

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert!(!state.is_search_active());
        assert!(matches!(state.modal(), Some(LibraryModal::ExportResult(_))));
    }

    #[test]
    fn export_modal_defaults_match_cli_behavior() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        let Some(LibraryModal::Export(modal)) = state.modal() else {
            panic!("expected export modal");
        };
        assert!(modal.path().ends_with(".tau"));
        assert!(modal.encrypt());
        assert_eq!(modal.password_masked(), "");
        assert_eq!(modal.password_display_value(), "");
        assert!(!modal.show_password());
        assert_eq!(modal.password_toggle_label(), "show");
        assert!(!modal.include_settings());
        assert!(!modal.include_metrics());
        assert_eq!(state.footer_text(), LIBRARY_EXPORT_MODAL_FOOTER);
    }

    #[test]
    fn export_modal_tab_skips_password_when_encryption_is_disabled() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
            panic!("expected export modal");
        };
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryExportModalField::Encrypt);

        modal.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!modal.encrypt());

        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryExportModalField::IncludeSettings);
    }

    #[test]
    fn export_modal_requires_password_when_encryption_is_enabled() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
            panic!("expected export modal");
        };
        let interaction =
            modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));

        assert!(interaction.pending_export().is_none());
        assert_eq!(
            modal.error(),
            Some("Configuration error: Encryption password is required.")
        );
    }

    #[test]
    fn export_modal_password_field_stores_typed_characters() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
            panic!("expected export modal");
        };
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        assert_eq!(modal.password_masked(), "***");
        assert_eq!(modal.password_display_value(), "***");
        assert!(!modal.show_password());
    }

    #[test]
    fn export_modal_password_visibility_toggle_preserves_value() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
            panic!("expected export modal");
        };
        // Tab to Password, type, then Tab to PasswordToggle
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for ch in "secret".chars() {
            modal.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryExportModalField::PasswordToggle);

        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(modal.show_password());
        assert_eq!(modal.password_display_value(), "secret");
        assert_eq!(modal.password_toggle_label(), "hide");

        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!modal.show_password());
        assert_eq!(modal.password_display_value(), "******");
        assert_eq!(modal.password_toggle_label(), "show");
    }

    #[test]
    fn export_modal_password_footer_includes_show_hide_hint_when_toggle_focused() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        // Tab to Password, then Tab to PasswordToggle
        {
            let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
                panic!("expected export modal");
            };
            modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert_eq!(modal.focus(), LibraryExportModalField::Password);
        }
        assert_eq!(state.footer_text(), LIBRARY_EXPORT_MODAL_FOOTER);

        {
            let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
                panic!("expected export modal");
            };
            modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert_eq!(modal.focus(), LibraryExportModalField::PasswordToggle);
        }
        assert_eq!(state.footer_text(), LIBRARY_EXPORT_PASSWORD_FOOTER);
    }

    #[test]
    fn disabling_encryption_hides_password_field_and_resets_visibility() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
            panic!("expected export modal");
        };
        // Tab to Password, type, Tab to PasswordToggle, toggle show
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for ch in "secret".chars() {
            modal.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(modal.show_password());

        // BackTab twice: PasswordToggle -> Password -> Encrypt
        modal.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        modal.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        modal.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

        assert!(!modal.encrypt());
        assert!(!modal.show_password());
        assert_eq!(modal.visible_fields(), &EXPORT_PLAINTEXT_OPTIONS);
        assert_eq!(modal.focus(), LibraryExportModalField::Encrypt);
    }

    #[test]
    fn export_modal_ctrl_s_creates_pending_export_when_plaintext() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
            panic!("expected export modal");
        };
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

        let interaction =
            modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));

        let pending = interaction.pending_export().expect("pending export");
        assert!(!pending.encrypt);
        assert_eq!(pending.password, None);
        assert!(!pending.include_settings);
        assert!(!pending.include_metrics);
    }

    #[test]
    fn export_modal_owns_input_and_keeps_search_inactive() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert!(!state.is_search_active());
        assert!(matches!(state.modal(), Some(LibraryModal::Export(_))));
    }

    #[test]
    fn pressing_d_with_selected_automation_opens_delete_confirmation_modal() {
        let mut state = sample_state();

        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(matches!(
            state.modal(),
            Some(LibraryModal::ConfirmDelete(_))
        ));
    }

    #[test]
    fn pressing_d_from_editor_edit_mode_keeps_editor_open() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();
        state.open_editor_modal(detail);

        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(matches!(state.modal(), Some(LibraryModal::Editor(_))));
    }

    #[test]
    fn typing_d_in_create_modal_trigger_field_inserts_text() {
        let mut state = sample_state();
        state.open_create_modal();

        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        let Some(LibraryModal::Editor(modal)) = state.modal() else {
            panic!("expected editor modal");
        };
        assert_eq!(modal.trigger(), "d");
        assert!(modal.error().is_none());
    }

    #[test]
    fn typing_d_in_create_modal_content_field_inserts_text() {
        let mut state = sample_state();
        state.open_create_modal();
        state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        let Some(LibraryModal::Editor(modal)) = state.modal() else {
            panic!("expected editor modal");
        };
        assert_eq!(modal.content(), "d");
        assert!(modal.error().is_none());
    }

    #[test]
    fn typing_d_in_edit_modal_trigger_field_inserts_text() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();
        state.open_editor_modal(detail);

        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        let Some(LibraryModal::Editor(modal)) = state.modal() else {
            panic!("expected editor modal");
        };
        assert_eq!(modal.trigger(), "gmd");
        assert!(modal.error().is_none());
    }

    #[test]
    fn typing_d_in_edit_modal_content_field_inserts_text() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();
        state.open_editor_modal(detail);
        state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        let Some(LibraryModal::Editor(modal)) = state.modal() else {
            panic!("expected editor modal");
        };
        assert_eq!(modal.content(), "Good Morningd");
        assert!(modal.error().is_none());
    }

    #[test]
    fn pressing_d_from_create_mode_does_not_open_delete_confirmation() {
        let mut state = sample_state();
        state.open_create_modal();

        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(matches!(state.modal(), Some(LibraryModal::Editor(_))));
    }

    #[test]
    fn pressing_d_from_edit_mode_with_text_focus_does_not_open_delete_confirmation() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();
        state.open_editor_modal(detail);

        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(matches!(state.modal(), Some(LibraryModal::Editor(_))));
    }

    #[test]
    fn pressing_escape_closes_open_modal() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap();
        state.open_editor_modal(detail);

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(interaction.should_close_modal());
    }

    #[test]
    fn script_modal_uses_actual_script_content_instead_of_description() {
        let mut row = automation_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        );
        row.description = Some("Open Reddit".to_string());

        let detail = LibraryAutomationDetail::from_row(row).unwrap();

        assert_eq!(detail.content_label(), "Script");
        assert_eq!(detail.content(), "Start-Process https://reddit.com");
    }

    #[test]
    fn snippet_modal_uses_actual_output_content() {
        let row = automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        );

        let detail = LibraryAutomationDetail::from_row(row).unwrap();

        assert_eq!(detail.content_label(), "Output");
        assert_eq!(detail.content(), "Good Morning");
    }

    #[test]
    fn modal_footer_replaces_library_actions_while_open() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();

        state.open_editor_modal(detail);

        assert_eq!(state.footer_text(), LIBRARY_EDIT_MODAL_FOOTER);
    }

    #[test]
    fn modal_owns_input_and_keeps_search_inactive() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();
        state.open_editor_modal(detail);

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert!(!state.is_search_active());
        assert!(state.is_modal_open());
    }

    #[test]
    fn delete_confirmation_owns_input_and_keeps_search_inactive() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert!(!state.is_search_active());
        assert!(matches!(
            state.modal(),
            Some(LibraryModal::ConfirmDelete(_))
        ));
    }

    #[test]
    fn delete_confirmation_cancel_restores_editor_modal() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();
        state.open_editor_modal(detail);
        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

        assert!(matches!(state.modal(), Some(LibraryModal::Editor(_))));
    }

    #[test]
    fn delete_confirmation_enter_creates_pending_delete() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let pending = interaction.pending_delete().expect("pending delete");
        assert_eq!(pending.automation_id, "id-alt+r");
        assert_eq!(pending.restore_index(), 0);
    }

    #[test]
    fn select_after_delete_chooses_nearest_remaining_item() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.replace_items(vec![
            LibraryAutomation::from(list_item(
                "id-gm",
                None,
                TriggerType::Word,
                "gm",
                "Good Morning",
                "text",
                "all",
                9,
                None,
            )),
            LibraryAutomation::from(list_item(
                "id-deploy",
                None,
                TriggerType::Word,
                "deploy",
                "[Script: bash]",
                "script",
                "linux",
                4,
                Some("npm run build && npm publish"),
            )),
        ]);

        state.select_after_delete(2);

        assert_eq!(state.selected_index(), Some(1));
        assert_eq!(state.item_at_filtered(1).unwrap().trigger(), "gm");
    }

    #[test]
    fn tab_and_shift_tab_cycle_modal_focus() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Word,
                "gm",
                "Good Morning",
                "text",
                "all",
                9,
                None,
            ))
            .unwrap(),
        );

        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::Content);

        modal.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(modal.focus(), LibraryModalField::Trigger);
    }

    #[test]
    fn content_focus_supports_cursor_navigation() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Word,
                "gm",
                "line one\nline two\nline three",
                "text",
                "all",
                9,
                None,
            ))
            .unwrap(),
        );
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        modal.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert_eq!(modal.content_line_indicator(1).as_deref(), Some("2/3"));
    }

    #[test]
    fn editor_modal_initializes_editable_fields_from_selected_automation() {
        let modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            ))
            .unwrap(),
        );

        assert_eq!(modal.trigger(), "alt+r");
        assert_eq!(modal.content(), "Start-Process https://reddit.com");
        assert_eq!(modal.kind_label(), "hotkey script");
        assert_eq!(modal.target_os(), "windows");
    }

    #[test]
    fn editing_trigger_updates_modal_draft_state() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Word,
                "gm",
                "Good Morning",
                "text",
                "all",
                9,
                None,
            ))
            .unwrap(),
        );

        modal.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::SHIFT));

        assert_eq!(modal.trigger(), "gm!");
    }

    #[test]
    fn editing_content_updates_modal_draft_state() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Word,
                "gm",
                "Good",
                "text",
                "all",
                9,
                None,
            ))
            .unwrap(),
        );
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT));

        assert_eq!(modal.content(), "Good M");
    }

    #[test]
    fn kind_selector_updates_kind_on_enter() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Word,
                "gm",
                "Good",
                "text",
                "all",
                9,
                None,
            ))
            .unwrap(),
        );
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(modal.selector().is_some());
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(modal.kind_label(), "script");
        assert_eq!(modal.content_label(), "Script");
    }

    #[test]
    fn create_modal_initially_hides_language_and_mode_for_snippet() {
        let modal = LibraryEditorModalState::new_create();

        assert_eq!(modal.visible_fields(), &SNIPPET_MODAL_FIELDS);
        assert!(!modal.is_script_kind());
    }

    #[test]
    fn changing_kind_to_script_shows_language_and_mode() {
        let mut modal = LibraryEditorModalState::new_create();
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(modal.kind_label(), "script");
        assert_eq!(modal.visible_fields(), &SCRIPT_MODAL_FIELDS);
        assert_eq!(
            modal.interpreter(),
            default_script_interpreter_for_target_os("all")
        );
        assert_eq!(modal.mode_label(), "inline");
    }

    #[test]
    fn changing_kind_to_hotkey_script_shows_language_and_mode() {
        let mut modal = LibraryEditorModalState::new_create();
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(modal.kind_label(), "hotkey script");
        assert_eq!(modal.visible_fields(), &SCRIPT_MODAL_FIELDS);
        assert_eq!(modal.mode_label(), "inline");
    }

    #[test]
    fn changing_kind_back_to_snippet_hides_language_and_mode() {
        let mut modal = LibraryEditorModalState::new_create();
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(modal.kind_label(), "snippet");
        assert_eq!(modal.visible_fields(), &SNIPPET_MODAL_FIELDS);
        assert_eq!(modal.focus(), LibraryModalField::Kind);
    }

    #[test]
    fn new_script_mode_defaults_to_inline() {
        let mut modal = LibraryEditorModalState::new_create();
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(modal.behavior(), ScriptBehavior::Inline);
        assert_eq!(modal.mode_label(), "inline");
    }

    #[test]
    fn language_selector_uses_exact_supported_options() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            ))
            .unwrap(),
        );
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let selector = modal.selector().expect("language selector");
        assert_eq!(selector.title(), "Select Language");
        assert_eq!(
            selector.options,
            vec!["bash", "powershell", "python", "node", "node-esm", "cmd"]
        );
    }

    #[test]
    fn mode_selector_uses_exact_supported_options() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            ))
            .unwrap(),
        );
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let selector = modal.selector().expect("mode selector");
        assert_eq!(selector.title(), "Select Mode");
        assert_eq!(selector.options, vec!["inline", "silent"]);
    }

    #[test]
    fn selecting_language_updates_draft_language() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            ))
            .unwrap(),
        );
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(modal.language_label(), "python");
    }

    #[test]
    fn selecting_mode_updates_draft_mode() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            ))
            .unwrap(),
        );
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(modal.behavior(), ScriptBehavior::Inline);
        assert_eq!(modal.mode_label(), "inline");
    }

    #[test]
    fn tab_visits_language_and_mode_only_for_script_kinds() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            ))
            .unwrap(),
        );

        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::Content);
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::Kind);
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::TargetOs);
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::Language);
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::Mode);
    }

    #[test]
    fn tab_skips_language_and_mode_for_snippet_kinds() {
        let mut modal = LibraryEditorModalState::new_create();

        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::Content);
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::Kind);
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::TargetOs);
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::Trigger);
    }

    #[test]
    fn typing_j_and_k_in_content_field() {
        let mut modal = LibraryEditorModalState::new_create();
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)); // Focus content

        modal.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));

        assert_eq!(modal.content(), "jk");
    }

    #[test]
    fn target_os_selector_updates_target_os_on_enter() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Word,
                "gm",
                "Good",
                "text",
                "all",
                9,
                None,
            ))
            .unwrap(),
        );
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(modal.selector().is_some());
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(modal.target_os(), "windows");
    }

    #[test]
    fn ctrl_s_creates_pending_save_for_existing_automation() {
        let mut modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            ))
            .unwrap(),
        );

        let interaction =
            modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let pending = interaction.pending_save().unwrap();

        assert_eq!(pending.kind, LibraryKind::HotkeyScript);
        assert_eq!(pending.content, "Start-Process https://reddit.com");
        assert_eq!(pending.interpreter, Some(ScriptInterpreter::PowerShell));
        assert_eq!(pending.behavior, Some(ScriptBehavior::Silent));
        assert!(matches!(
            pending.mode(),
            PendingLibrarySaveMode::Update { id, .. } if id == "automation-alt+r"
        ));
    }

    #[test]
    fn create_modal_initializes_empty_defaults() {
        let modal = LibraryEditorModalState::new_create();

        assert_eq!(modal.mode(), LibraryEditorMode::Create);
        assert_eq!(modal.trigger(), "");
        assert_eq!(modal.content(), "");
        assert_eq!(modal.kind_label(), "snippet");
        assert_eq!(modal.target_os(), "all");
        assert_eq!(
            modal.interpreter(),
            default_script_interpreter_for_target_os("all")
        );
        assert_eq!(modal.behavior(), ScriptBehavior::Inline);
    }

    #[test]
    fn ctrl_s_creates_pending_save_for_new_automation() {
        let mut modal = LibraryEditorModalState::new_create();
        modal.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::SHIFT));
        modal.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

        let interaction =
            modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let pending = interaction.pending_save().unwrap();

        assert!(matches!(pending.mode(), PendingLibrarySaveMode::Create));
        assert_eq!(pending.kind, LibraryKind::Snippet);
        assert_eq!(pending.target_os, "all");
        assert_eq!(pending.trigger, "gm");
        assert_eq!(pending.content, "Hi");
        assert_eq!(pending.interpreter, None);
        assert_eq!(pending.behavior, None);
    }

    #[test]
    fn ctrl_s_for_new_script_captures_language_and_mode() {
        let mut modal = LibraryEditorModalState::new_create();
        modal.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let language_steps = match modal.interpreter() {
            ScriptInterpreter::Bash => 2,
            ScriptInterpreter::PowerShell => 1,
            _ => 0,
        };
        for _ in 0..language_steps {
            modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let interaction =
            modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let pending = interaction.pending_save().unwrap();

        assert_eq!(pending.kind, LibraryKind::Script);
        assert_eq!(pending.interpreter, Some(ScriptInterpreter::Python));
        assert_eq!(pending.behavior, Some(ScriptBehavior::Silent));
    }

    #[test]
    fn editing_existing_script_preserves_language_and_mode() {
        let modal = LibraryEditorModalState::new_edit(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            ))
            .unwrap(),
        );

        assert_eq!(modal.language_label(), "powershell");
        assert_eq!(modal.mode_label(), "silent");
    }

    #[test]
    fn modal_keeps_library_selection_stable_after_close() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        let selected_before = state.selected_index();

        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "linux",
            4,
            Some("npm run build && npm publish"),
        ))
        .unwrap();
        state.open_editor_modal(detail);
        state.clear_modal();

        assert_eq!(state.selected_index(), selected_before);
        assert_eq!(state.search_query(), "");
    }
}
