use std::path::PathBuf;
use zeroize::Zeroize;

use taurine_core::db::crud::{
    ActionType, InvocationType, NewEntry, TriggerListItem, TriggerType, create_entry,
    delete_trigger,
};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};
use taurine_core::exchange::{
    ExchangePayload, ImportConflictAction, decode_exchange_blob, encode_exchange_blob,
    export_triggers, import_payload_transactionally, payload_contains_run_variables,
    resolve_export_path,
};

use crate::widgets::library::state::{LibraryImportModalState, LibraryTrigger};

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
pub(crate) struct PendingLibraryCreate {
    pub(crate) trigger: String,
    pub(crate) trigger_type: TriggerType,
    pub(crate) content: String,
    pub(crate) action_type: String,
    pub(crate) target_os: String,
    pub(crate) tags: Vec<String>,
    pub(crate) allow_apps: Vec<String>,
    pub(crate) block_apps: Vec<String>,
    pub(crate) auto_case: bool,
    pub(crate) interpreter: Option<ScriptInterpreter>,
    pub(crate) behavior: Option<ScriptBehavior>,
}

impl PendingLibraryCreate {
    fn invocation_type(&self) -> InvocationType {
        match self.trigger_type {
            TriggerType::Word => InvocationType::Word,
            TriggerType::Hotkey => InvocationType::Hotkey,
            TriggerType::Regex => InvocationType::Regex,
        }
    }

    fn join_filters(items: &[String]) -> Option<String> {
        (!items.is_empty()).then(|| join_app_filters(items))
    }

    /// Insert the draft as a new entry, returning its id so the page
    /// can select the new row. Conflict and validation failures
    /// surface with the modal still open.
    pub(crate) fn apply(&self) -> taurine_core::Result<String> {
        let conn = taurine_core::db::init::setup()?;
        let tags_json = serde_json::to_string(&self.tags).map_err(|error| {
            taurine_core::Error::Config(format!("Tags could not be encoded: {error}"))
        })?;
        let (id, _) = create_entry(
            &conn,
            NewEntry {
                description: None,
                content: self.content.clone(),
                action_type: self.action_type.clone(),
                target_os: self.target_os.clone(),
                only_apps: Self::join_filters(&self.allow_apps),
                except_apps: Self::join_filters(&self.block_apps),
                tags_json,
                auto_case: self.auto_case,
                interpreter: self.interpreter,
                behavior: self.behavior,
                invocations: vec![(self.invocation_type(), self.trigger.clone(), false)],
            },
        )?;
        taurine_core::rpc::notify_daemon_reload();
        Ok(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryToggle {
    pub(crate) trigger_id: String,
    pub(crate) enabled: bool,
    pub(crate) restore_index: usize,
}

impl PendingLibraryToggle {
    pub(crate) fn apply(&self) -> taurine_core::Result<()> {
        let conn = taurine_core::db::init::setup()?;
        taurine_core::db::crud::set_trigger_enabled(&conn, &self.trigger_id, self.enabled)?;
        taurine_core::rpc::notify_daemon_reload();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EditedField {
    Trigger(String),
    Description(Option<String>),
    Content(String),
    InvocationType(InvocationType),
    Interpreter(ScriptInterpreter),
    Behavior(ScriptBehavior),
    AutoCase(bool),
    RequireConfirmation(bool),
    TargetOs(String),
    Tags(Vec<String>),
    OnlyApps(Vec<String>),
    ExceptApps(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryEdit {
    pub(crate) trigger_id: String,
    pub(crate) trigger: String,
    pub(crate) field: EditedField,
    pub(crate) restore_index: usize,
}

/// Join stored filter items for the database, escaping literal
/// commas the same way the CLI split understands them.
pub(crate) fn join_app_filters(items: &[String]) -> String {
    items
        .iter()
        .map(|item| item.replace(',', "\\,"))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
thread_local! {
    static MOCK_CLIPBOARD: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Test-only clipboard stand-in; the default suite never touches the host.
#[cfg(test)]
pub(crate) fn set_mock_clipboard(text: Option<String>) {
    MOCK_CLIPBOARD.with(|slot| *slot.borrow_mut() = text);
}

/// Read the system clipboard. Unit tests read the mock above, so
/// copy/paste wiring stays hermetic; prod reads the host clipboard
/// through core like the daemon listener.
#[cfg(test)]
pub(crate) fn read_clipboard() -> Option<String> {
    MOCK_CLIPBOARD.with(|slot| slot.borrow().clone())
}

/// Read the system clipboard through core like the daemon listener.
#[cfg(not(test))]
pub(crate) fn read_clipboard() -> Option<String> {
    taurine_core::engine::variables::system::clipboard::resolve("")
}

/// Write the system clipboard (mocked in unit tests, see above).
#[cfg(test)]
pub(crate) fn write_clipboard(text: &str) {
    MOCK_CLIPBOARD.with(|slot| *slot.borrow_mut() = Some(text.to_string()));
}

/// Write the system clipboard through core like the daemon listener.
#[cfg(not(test))]
pub(crate) fn write_clipboard(text: &str) {
    taurine_core::engine::variables::system::clipboard::store(text);
}

impl PendingLibraryEdit {
    /// Renaming an invocation changes what the expander matches, so the
    /// daemon reloads. Descriptions never reach the expander, so no
    /// daemon reload. Content changes reload so expansion picks them up.
    pub(crate) fn apply(&self) -> taurine_core::Result<()> {
        let conn = taurine_core::db::init::setup()?;
        match &self.field {
            EditedField::Trigger(trigger) => {
                taurine_core::db::crud::rename_alias_invocation(
                    &conn,
                    &self.trigger_id,
                    &self.trigger,
                    trigger,
                )?;
                taurine_core::rpc::notify_daemon_reload();
            }
            EditedField::Description(description) => {
                taurine_core::db::crud::set_trigger_description(
                    &conn,
                    &self.trigger_id,
                    description.as_deref(),
                )?;
            }
            EditedField::Content(body) => {
                taurine_core::db::crud::set_trigger_content(&conn, &self.trigger_id, body)?;
                taurine_core::rpc::notify_daemon_reload();
            }
            // honey: invocation, language, and behavior all change what
            // the expander matches or runs, so the daemon reloads.
            EditedField::InvocationType(invocation_type) => {
                taurine_core::db::crud::set_alias_invocation_type(
                    &conn,
                    &self.trigger_id,
                    &self.trigger,
                    *invocation_type,
                )?;
                taurine_core::rpc::notify_daemon_reload();
            }
            EditedField::Interpreter(interpreter) => {
                taurine_core::db::crud::set_script_interpreter(
                    &conn,
                    &self.trigger_id,
                    *interpreter,
                )?;
                taurine_core::rpc::notify_daemon_reload();
            }
            EditedField::Behavior(behavior) => {
                taurine_core::db::crud::set_script_behavior(&conn, &self.trigger_id, *behavior)?;
                taurine_core::rpc::notify_daemon_reload();
            }
            EditedField::AutoCase(auto_case) => {
                taurine_core::db::crud::set_trigger_auto_case(&conn, &self.trigger_id, *auto_case)?;
                taurine_core::rpc::notify_daemon_reload();
            }
            // honey: confirmation gates voice expansion like auto-case
            // gates text, so the daemon reloads too.
            EditedField::RequireConfirmation(confirm) => {
                taurine_core::db::crud::set_alias_require_confirmation(
                    &conn,
                    &self.trigger_id,
                    &self.trigger,
                    *confirm,
                )?;
                taurine_core::rpc::notify_daemon_reload();
            }
            EditedField::TargetOs(target_os) => {
                taurine_core::db::crud::set_trigger_target_os(&conn, &self.trigger_id, target_os)?;
                taurine_core::rpc::notify_daemon_reload();
            }
            // honey: tags never reach the expander, so no daemon reload.
            EditedField::Tags(tags) => {
                taurine_core::db::crud::set_trigger_tags(&conn, &self.trigger_id, tags)?;
            }
            // honey: app filters gate expansion, so the daemon reloads.
            EditedField::OnlyApps(apps) => {
                let value = (!apps.is_empty()).then(|| join_app_filters(apps));
                taurine_core::db::crud::set_trigger_only_apps(
                    &conn,
                    &self.trigger_id,
                    value.as_deref(),
                )?;
                taurine_core::rpc::notify_daemon_reload();
            }
            EditedField::ExceptApps(apps) => {
                let value = (!apps.is_empty()).then(|| join_app_filters(apps));
                taurine_core::db::crud::set_trigger_except_apps(
                    &conn,
                    &self.trigger_id,
                    value.as_deref(),
                )?;
                taurine_core::rpc::notify_daemon_reload();
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryExport {
    pub(crate) path: String,
    pub(crate) password: Option<String>,
}

impl PendingLibraryExport {
    pub(crate) fn apply(&self) -> taurine_core::Result<PathBuf> {
        let path = resolve_export_path(Some(PathBuf::from(self.path.as_str())))?;
        let conn = taurine_core::db::init::setup()?;
        let payload = export_triggers(&conn)?;
        let mut pw = self.password.clone();
        let encoded_res = encode_exchange_blob(&payload, pw.as_deref());
        if let Some(ref mut p) = pw {
            p.zeroize();
        }
        let encoded = encoded_res?;
        taurine_core::exchange::write_export_file(&path, &encoded)?;
        Ok(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingLibraryImportPrepare {
    pub(crate) path: String,
    pub(crate) password: Option<String>,
    pub(crate) conflict_mode: LibraryImportConflictMode,
    pub(crate) return_to_modal: LibraryImportModalState,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedLibraryImport {
    path: String,
    payload: ExchangePayload,
    conflict_mode: LibraryImportConflictMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryImportOutcome {
    imported: usize,
}

impl LibraryImportOutcome {
    #[cfg(test)]
    pub(crate) const fn new(imported: usize) -> Self {
        Self { imported }
    }

    pub(crate) const fn imported(&self) -> usize {
        self.imported
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
        let payload = match decode_exchange_blob(&bytes, self.password.as_deref()) {
            Ok(payload) => payload,
            Err(err) if err.to_string().contains("password required") => {
                return Err(taurine_core::Error::Config(
                    "A password is required to import this file.".to_string(),
                ));
            }
            Err(err) if err.to_string() == "wrong password" => {
                return Err(taurine_core::Error::Config(
                    "Wrong password, try again.".to_string(),
                ));
            }
            Err(err) if err.to_string().contains("corrupted") => {
                return Err(taurine_core::Error::Config(
                    "File is corrupted and cannot be imported.".to_string(),
                ));
            }
            Err(err) => return Err(err),
        };
        let prepared = PreparedLibraryImport {
            path: self.path.clone(),
            payload,
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
        let imported = import_payload_transactionally(&mut conn, &self.payload, |_, _| {
            Ok(self.conflict_mode.to_action())
        })?;

        Ok(LibraryImportOutcome { imported })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct LibraryInteraction {
    pending_delete: Option<PendingLibraryDelete>,
    pending_create: Option<PendingLibraryCreate>,
    pending_toggle: Option<PendingLibraryToggle>,
    pending_edit: Option<PendingLibraryEdit>,
    pending_export: Option<PendingLibraryExport>,
    pending_import_prepare: Option<PendingLibraryImportPrepare>,
    pending_import_commit: Option<PreparedLibraryImport>,
    close_modal: bool,
}

impl LibraryInteraction {
    pub(crate) const fn pending_delete(&self) -> Option<&PendingLibraryDelete> {
        self.pending_delete.as_ref()
    }

    pub(crate) const fn pending_create(&self) -> Option<&PendingLibraryCreate> {
        self.pending_create.as_ref()
    }

    pub(crate) const fn pending_toggle(&self) -> Option<&PendingLibraryToggle> {
        self.pending_toggle.as_ref()
    }

    pub(crate) const fn pending_edit(&self) -> Option<&PendingLibraryEdit> {
        self.pending_edit.as_ref()
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

    pub(crate) fn delete(pending_delete: PendingLibraryDelete) -> Self {
        Self {
            pending_delete: Some(pending_delete),
            pending_create: None,
            pending_toggle: None,
            pending_edit: None,
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    pub(crate) fn create(pending_create: PendingLibraryCreate) -> Self {
        Self {
            pending_delete: None,
            pending_create: Some(pending_create),
            pending_toggle: None,
            pending_edit: None,
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    pub(crate) fn toggle(pending_toggle: PendingLibraryToggle) -> Self {
        Self {
            pending_delete: None,
            pending_create: None,
            pending_toggle: Some(pending_toggle),
            pending_edit: None,
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    pub(crate) fn edit(pending_edit: PendingLibraryEdit) -> Self {
        Self {
            pending_delete: None,
            pending_create: None,
            pending_toggle: None,
            pending_edit: Some(pending_edit),
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    pub(crate) fn export(pending_export: PendingLibraryExport) -> Self {
        Self {
            pending_delete: None,
            pending_create: None,
            pending_toggle: None,
            pending_edit: None,
            pending_export: Some(pending_export),
            pending_import_prepare: None,
            pending_import_commit: None,
            close_modal: false,
        }
    }

    pub(crate) fn prepare_import(pending_import_prepare: PendingLibraryImportPrepare) -> Self {
        Self {
            pending_delete: None,
            pending_create: None,
            pending_toggle: None,
            pending_edit: None,
            pending_export: None,
            pending_import_prepare: Some(pending_import_prepare),
            pending_import_commit: None,
            close_modal: false,
        }
    }

    pub(crate) fn import(prepared: PreparedLibraryImport) -> Self {
        Self {
            pending_delete: None,
            pending_create: None,
            pending_toggle: None,
            pending_edit: None,
            pending_export: None,
            pending_import_prepare: None,
            pending_import_commit: Some(prepared),
            close_modal: false,
        }
    }

    pub(crate) fn close() -> Self {
        Self {
            pending_delete: None,
            pending_create: None,
            pending_toggle: None,
            pending_edit: None,
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

const SCRIPT_PREVIEW_FALLBACK: &str = "Script content unavailable.";

/// Full multi-line body for the detail content section: script source for
/// scripts, raw output for text. Unlike `preview_from_item` (single-line,
/// description-first for the list subtitle) this never returns the
/// description.
pub(crate) fn content_from_item(item: &TriggerListItem) -> String {
    if ActionType::parse_str(&item.action_type) == Some(ActionType::Script) {
        if let Some(script) = item.script_content.as_deref() {
            let trimmed = script.trim();
            if !trimmed.is_empty() {
                return script.to_string();
            }
        }
        if !item.output.trim().is_empty() {
            return item.output.clone();
        }
        return SCRIPT_PREVIEW_FALLBACK.to_string();
    }
    if !item.output.trim().is_empty() {
        return item.output.clone();
    }
    if let Some(script) = item.script_content.as_deref()
        && !script.trim().is_empty()
    {
        return script.to_string();
    }
    "No preview available.".to_string()
}

pub(crate) fn preview_from_item(item: &TriggerListItem) -> String {
    if let Some(description) = normalized_preview_text(item.description.as_deref())
        && !is_script_placeholder(&description)
    {
        return description;
    }

    if ActionType::parse_str(&item.action_type) == Some(ActionType::Script) {
        if let Some(script_content) = normalized_preview_text(item.script_content.as_deref()) {
            return script_content;
        }

        if let Some(output) = normalized_preview_text(Some(item.output.as_str()))
            && !is_script_placeholder(&output)
        {
            return output;
        }

        return SCRIPT_PREVIEW_FALLBACK.to_string();
    }

    if let Some(output) = normalized_preview_text(Some(item.output.as_str())) {
        return output;
    }

    if let Some(script_content) = normalized_preview_text(item.script_content.as_deref()) {
        return script_content;
    }

    "No preview available.".to_string()
}

/// Alias detail line per CLI `list.rs`: `type: invocation`.
pub(crate) fn alias_line(invocation_type: &str, invocation: &str) -> String {
    format!("{invocation_type}: {invocation}")
}

pub(crate) fn build_search_text(
    item: &TriggerListItem,
    kind_label: &str,
    display_target_os: &str,
    invocation: &str,
) -> String {
    let mut parts = vec![
        invocation,
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

fn is_script_placeholder(value: &str) -> bool {
    let normalized = value.trim();
    (normalized.starts_with("[Script:") && normalized.ends_with(']'))
        || normalized
            .to_ascii_lowercase()
            .starts_with("shell script (")
}

pub(crate) fn display_target_os(target_os: &str) -> &str {
    if let Some(os) = taurine_core::db::TargetOs::parse_str(target_os) {
        os.display_name()
    } else {
        target_os
    }
}
