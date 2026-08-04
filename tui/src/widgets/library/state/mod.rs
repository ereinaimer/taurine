mod delete;
mod editor;
mod export;
mod import;
mod trigger;

pub(crate) use delete::*;
pub(crate) use editor::*;
pub(crate) use export::*;
pub(crate) use import::*;
pub(crate) use trigger::*;

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::widgets::library::actions::{
    LibraryImportOutcome, LibraryInteraction, PendingLibraryDelete, PreparedLibraryImport,
};

pub(crate) const LIBRARY_FOOTER: &str =
    "/ Search   n New   i Import   x Export   d Delete   Enter Edit   q Quit";

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
pub(crate) enum LibraryImportModalField {
    Path,
    Password,
    IncludeSettings,
    IncludeSensitiveSettings,
    StatsMode,
    ConflictMode,
    ActionButton,
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
                    state.set_selected_yes(true);
                    self.modal = Some(LibraryModal::ConfirmDelete(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Right, KeyModifiers::NONE) | (KeyCode::Char('l'), KeyModifiers::NONE) => {
                    state.set_selected_yes(false);
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
