use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use taurine_core::db::crud::{
    ExistingTriggerUpdate, NewTrigger, TriggerListItem, TriggerRow, create_trigger, delete_trigger,
    update_existing_trigger,
};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter, decompress};
use taurine_core::exchange::{
    ExchangeFormat, ExchangePayload, ExportOptions, ImportConflictAction, ImportOptions,
    ImportStatsMode, decode_exchange_blob, detect_exchange_format, encode_exchange_blob,
    export_triggers, import_payload_transactionally, payload_contains_run_variables,
    resolve_export_path,
};

use crate::widgets::library::state::{
    LibraryImportModalState, LibraryKind, LibraryMetadataRow, LibraryTrigger,
};

pub(crate) const DEFAULT_SCRIPT_FALLBACK: &str = "Script content unavailable.";
const DEFAULT_OUTPUT_FALLBACK: &str = "No output available.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryImportConflictMode {
    Skip,
    Overwrite,
}

impl LibraryImportConflictMode {
    pub(crate) const ALL: [Self; 2] = [Self::Skip, Self::Overwrite];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Overwrite => "overwrite",
        }
    }

    pub(crate) const fn to_action(self) -> ImportConflictAction {
        match self {
            Self::Skip => ImportConflictAction::Skip,
            Self::Overwrite => ImportConflictAction::Overwrite,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RememberedConflictChoice {
    OverwriteAll,
    SkipAll,
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
    pub(crate) mode: PendingLibrarySaveMode,
    pub(crate) trigger: String,
    pub(crate) content: String,
    pub(crate) kind: LibraryKind,
    pub(crate) target_os: String,
    pub(crate) interpreter: Option<ScriptInterpreter>,
    pub(crate) behavior: Option<ScriptBehavior>,
}

impl PendingLibrarySave {
    #[cfg(test)]
    pub(crate) const fn mode(&self) -> &PendingLibrarySaveMode {
        &self.mode
    }

    pub(crate) fn apply(&self) -> taurine_core::Result<String> {
        let mut conn = taurine_core::db::init::setup()?;

        let trigger_id = match &self.mode {
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
                let existing_auto_case: bool = conn
                    .query_row("SELECT auto_case FROM triggers WHERE id = ?1", [id], |r| {
                        r.get(0)
                    })
                    .unwrap_or(false);

                update_existing_trigger(
                    &mut conn,
                    ExistingTriggerUpdate {
                        id,
                        name,
                        description: description.as_deref(),
                        trigger_type: self.kind.trigger_type(),
                        trigger: &self.trigger,
                        content: &self.content,
                        action_type: self.kind.action_type(),
                        target_os: &self.target_os,
                        tags_json,
                        auto_case: existing_auto_case,
                        usage_count: *usage_count,
                        last_used_at: *last_used_at,
                        interpreter: self.interpreter.or(*interpreter),
                        behavior: self.behavior.or(*behavior),
                    },
                )?;
                id.clone()
            }
            PendingLibrarySaveMode::Create => create_trigger(
                &mut conn,
                NewTrigger {
                    name: None,
                    description: None,
                    trigger_type: self.kind.trigger_type(),
                    trigger: &self.trigger,
                    content: &self.content,
                    action_type: self.kind.action_type(),
                    target_os: &self.target_os,
                    tags_json: "[]",
                    auto_case: false,
                    interpreter: self.interpreter,
                    behavior: self.behavior.or(Some(ScriptBehavior::Inline)),
                },
            )?,
        };

        taurine_core::rpc::notify_daemon_reload();
        Ok(trigger_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryDelete {
    pub(crate) trigger_id: String,
    pub(crate) restore_index: usize,
}

impl PendingLibraryDelete {
    pub(crate) const fn restore_index(&self) -> usize {
        self.restore_index
    }

    pub(crate) fn apply(&self) -> taurine_core::Result<()> {
        let conn = taurine_core::db::init::setup()?;
        if !delete_trigger(&conn, &self.trigger_id)? {
            return Err(taurine_core::Error::NotFound(
                "Trigger no longer exists.".to_string(),
            ));
        }
        taurine_core::rpc::notify_daemon_reload();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryExport {
    pub(crate) path: String,
    pub(crate) encrypt: bool,
    pub(crate) password: Option<String>,
    pub(crate) include_settings: bool,
    pub(crate) include_sensitive_settings: bool,
    pub(crate) include_stats: bool,
}

impl PendingLibraryExport {
    pub(crate) fn apply(&self) -> taurine_core::Result<PathBuf> {
        let path = resolve_export_path(Some(PathBuf::from(self.path.as_str())))?;
        let conn = taurine_core::db::init::setup()?;
        let payload = export_triggers(
            &conn,
            ExportOptions {
                include_settings: self.include_settings,
                include_stats: self.include_stats,
                include_sensitive_settings: self.include_sensitive_settings,
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

    pub(crate) const fn include_stats(&self) -> bool {
        self.include_stats
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryImportPrepare {
    pub(crate) path: String,
    pub(crate) password: Option<String>,
    pub(crate) options: ImportOptions,
    pub(crate) conflict_mode: LibraryImportConflictMode,
    pub(crate) return_to_modal: LibraryImportModalState,
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
    imported_stats: bool,
}

impl LibraryImportOutcome {
    #[cfg(test)]
    pub(crate) const fn new(
        imported: usize,
        imported_settings: bool,
        imported_stats: bool,
    ) -> Self {
        Self {
            imported,
            imported_settings,
            imported_stats,
        }
    }

    pub(crate) const fn imported(&self) -> usize {
        self.imported
    }

    pub(crate) const fn imported_settings(&self) -> bool {
        self.imported_settings
    }

    pub(crate) const fn imported_stats(&self) -> bool {
        self.imported_stats
    }
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
            path: self.path.clone(),
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
    #[allow(dead_code)]
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
            imported_stats: self.options.stats_mode != ImportStatsMode::Ignore
                && self.payload.stats.is_some(),
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

    pub(crate) fn handled() -> Self {
        Self::default()
    }

    pub(crate) fn open_selected(id: String) -> Self {
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

    pub(crate) fn open_create() -> Self {
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

    pub(crate) fn save(pending_save: PendingLibrarySave) -> Self {
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

    pub(crate) fn delete(pending_delete: PendingLibraryDelete) -> Self {
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

    pub(crate) fn export(pending_export: PendingLibraryExport) -> Self {
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

    pub(crate) fn prepare_import(pending_import_prepare: PendingLibraryImportPrepare) -> Self {
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

    pub(crate) fn import(prepared: PreparedLibraryImport) -> Self {
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

    pub(crate) fn close() -> Self {
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

pub(crate) fn sort_items(items: &mut [LibraryTrigger]) {
    items.sort_by(|left, right| {
        let left_trigger = left.trigger().to_ascii_lowercase();
        let right_trigger = right.trigger().to_ascii_lowercase();

        left_trigger
            .cmp(&right_trigger)
            .then_with(|| left.kind_label().cmp(right.kind_label()))
            .then_with(|| left.target_os.cmp(&right.target_os))
            .then_with(|| left.preview().cmp(right.preview()))
    });
}

pub(crate) fn char_index_to_byte_index(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(value.len())
}

pub(crate) fn split_lines_with_trailing(value: &str) -> Vec<&str> {
    if value.is_empty() {
        return vec![""];
    }

    value.split('\n').collect()
}

pub(crate) fn line_start_positions(value: &str) -> Vec<usize> {
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

pub(crate) fn line_lengths(value: &str) -> Vec<usize> {
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

pub(crate) fn char_index_for_line_col(value: &str, line_index: usize, column: usize) -> usize {
    let starts = line_start_positions(value);
    let lengths = line_lengths(value);
    let safe_line = line_index.min(starts.len().saturating_sub(1));
    starts[safe_line] + column.min(lengths[safe_line])
}

pub(crate) fn preview_from_item(item: &TriggerListItem) -> String {
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

pub(crate) fn modal_content_from_row(
    row: &TriggerRow,
    kind: LibraryKind,
) -> taurine_core::Result<String> {
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

pub(crate) fn build_metadata_rows(row: &TriggerRow) -> Vec<LibraryMetadataRow> {
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

fn load_script_content(row: &TriggerRow) -> taurine_core::Result<Option<String>> {
    row.script_binary
        .as_deref()
        .map(decompress)
        .transpose()
        .map(|content| content.and_then(|content| normalized_modal_text(Some(content.as_str()))))
}

pub(crate) fn build_search_text(
    item: &TriggerListItem,
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

pub(crate) fn normalized_modal_text(value: Option<&str>) -> Option<String> {
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

pub(crate) fn display_target_os(target_os: &str) -> &str {
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

pub(crate) const fn interpreter_label(interpreter: ScriptInterpreter) -> &'static str {
    match interpreter {
        ScriptInterpreter::Bash => "bash",
        ScriptInterpreter::PowerShell => "powershell",
        ScriptInterpreter::Python => "python",
        ScriptInterpreter::Node => "node",
        ScriptInterpreter::NodeEsm => "node-esm",
        ScriptInterpreter::Cmd => "cmd",
    }
}

pub(crate) const fn behavior_label(behavior: ScriptBehavior) -> &'static str {
    match behavior {
        ScriptBehavior::Inline => "inline",
        ScriptBehavior::Silent => "silent",
    }
}

pub(crate) fn default_script_interpreter_for_target_os(target_os: &str) -> ScriptInterpreter {
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
