mod delete;
mod edit;
mod export;
mod import;
mod trigger;

pub(crate) use delete::*;
pub(crate) use edit::*;
pub(crate) use export::*;
pub(crate) use import::*;
pub(crate) use trigger::*;

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::widgets::field::TextField;
use crate::widgets::library::actions::{
    LibraryImportOutcome, LibraryInteraction, PendingLibraryDelete, PreparedLibraryImport,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonSelection {
    Cancel,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryImportModalField {
    Path,
    Password,
    ConflictMode,
    ActionButton,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LibraryModal {
    // honey: unreachable until the shortcut rework lands; kept with tests.
    #[allow(dead_code)]
    Export(LibraryExportModalState),
    ExportResult(LibraryExportResultModalState),
    Import(LibraryImportModalState),
    ImportResult(LibraryImportResultModalState),
    ConfirmImportRunVariables(LibraryImportRunVariablesModalState),
    // honey: unreachable until the shortcut rework lands; kept with tests.
    #[allow(dead_code)]
    ConfirmDelete(LibraryDeleteModalState),
}

impl LibraryModal {
    pub(crate) fn set_error(&mut self, error: String) {
        match self {
            Self::Export(state) => state.set_error(error),
            Self::ExportResult(state) => state.set_error(error),
            Self::Import(state) => state.set_error(error),
            Self::ImportResult(state) => state.set_error(error),
            Self::ConfirmImportRunVariables(state) => state.set_error(error),
            Self::ConfirmDelete(state) => state.set_error(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LibraryPageState {
    items: Vec<LibraryTrigger>,
    filtered_indices: Vec<usize>,
    selected: usize,
    search: TextField,
    search_mode: bool,
    window_anchor: Option<usize>,
    pub(crate) modal: Option<LibraryModal>,
    status_message: Option<String>,
    load_error: Option<String>,
    split_ratio: f32,
    detail_ratio: f32,
    divider_hover: Option<super::DividerSide>,
    divider_drag: Option<super::DividerSide>,
    detail_scroll: usize,
    usage_expanded: bool,
    edit: Option<ActiveEdit>,
    last_edit_at: Option<u64>,
    content_width: u16,
    last_divider_click: Option<(super::DividerSide, u64)>,
}

/// Double-click window for divider reset, mirroring the overlay.
pub(crate) const DIVIDER_DOUBLE_CLICK_MS: u64 = 300;

/// Millis after the last keystroke before an open edit autosaves.
pub(crate) const AUTOSAVE_DELAY_MS: u64 = 1000;

pub(crate) fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

impl Default for LibraryPageState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            filtered_indices: Vec::new(),
            selected: 0,
            search: TextField::new(""),
            search_mode: false,
            window_anchor: None,
            modal: None,
            status_message: None,
            load_error: None,
            split_ratio: super::DEFAULT_SPLIT_RATIO,
            detail_ratio: super::DEFAULT_DETAIL_RATIO,
            divider_hover: None,
            divider_drag: None,
            detail_scroll: 0,
            usage_expanded: false,
            edit: None,
            last_edit_at: None,
            content_width: 0,
            last_divider_click: None,
        }
    }
}

impl LibraryPageState {
    pub(crate) fn replace_items(&mut self, mut items: Vec<LibraryTrigger>) {
        // honey: a refresh (e.g. after a target switch commit) must not
        // kill a live session; drop it only when its trigger is gone.
        // The click-time anchor survives for the same reason: stale
        // values fall back safely inside visible_window.
        let anchor = self.window_anchor;
        let live_edit = self
            .edit
            .take()
            .filter(|edit| items.iter().any(|item| item.id() == edit.trigger_id()));
        crate::widgets::library::actions::sort_items(&mut items);
        self.items = items;
        self.load_error = None;
        self.status_message = None;
        self.edit = live_edit;
        self.reset_detail_scroll();
        self.rebuild_filter();
        self.window_anchor = anchor;
    }

    pub(crate) fn set_load_error(&mut self, error: String) {
        self.load_error = Some(error);
    }

    // honey: unreachable until the shortcut rework lands; kept with tests.
    #[allow(dead_code)]
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

    pub(crate) fn search_query(&self) -> &str {
        self.search.text()
    }

    pub(crate) fn search_field(&self) -> &TextField {
        &self.search
    }

    pub(crate) const fn is_search_active(&self) -> bool {
        self.search_mode
    }

    pub(crate) const fn is_modal_open(&self) -> bool {
        self.modal.is_some()
    }

    pub(crate) fn split_ratio(&self) -> f32 {
        self.split_ratio
            .clamp(super::MIN_SPLIT_RATIO, super::MAX_SPLIT_RATIO)
    }

    pub(crate) fn set_split_ratio(&mut self, ratio: f32) {
        self.split_ratio = ratio.clamp(super::MIN_SPLIT_RATIO, super::MAX_SPLIT_RATIO);
    }

    pub(crate) fn detail_ratio(&self) -> f32 {
        self.detail_ratio
            .clamp(super::MIN_DETAIL_RATIO, super::MAX_DETAIL_RATIO)
    }

    pub(crate) fn set_detail_ratio(&mut self, ratio: f32) {
        self.detail_ratio = ratio.clamp(super::MIN_DETAIL_RATIO, super::MAX_DETAIL_RATIO);
    }

    pub(crate) const fn divider_hover(&self) -> Option<super::DividerSide> {
        self.divider_hover
    }

    pub(crate) fn set_divider_hover(&mut self, hover: Option<super::DividerSide>) {
        self.divider_hover = hover;
    }

    pub(crate) const fn divider_drag(&self) -> Option<super::DividerSide> {
        self.divider_drag
    }

    pub(crate) fn set_divider_drag(&mut self, drag: Option<super::DividerSide>) {
        self.divider_drag = drag;
        if drag.is_some() {
            self.divider_hover = drag;
        }
    }

    /// Second click on the same divider within the window resets that
    /// pane to its default width instead of starting a drag. Returns
    /// true when a reset happened.
    pub(crate) fn divider_double_click(&mut self, side: super::DividerSide) -> bool {
        self.divider_double_click_at(side, now_millis())
    }

    pub(crate) fn divider_double_click_at(&mut self, side: super::DividerSide, now: u64) -> bool {
        let double = self.last_divider_click.is_some_and(|(last_side, at)| {
            last_side == side && now.saturating_sub(at) <= DIVIDER_DOUBLE_CLICK_MS
        });
        self.last_divider_click = Some((side, now));
        if !double {
            return false;
        }
        self.last_divider_click = None;
        match side {
            super::DividerSide::List => self.set_split_ratio(super::DEFAULT_SPLIT_RATIO),
            super::DividerSide::Props => self.set_detail_ratio(super::DEFAULT_DETAIL_RATIO),
        }
        true
    }

    pub(crate) const fn detail_scroll(&self) -> usize {
        self.detail_scroll
    }

    /// Last known content text width, tracked by the event loop so caret
    /// math stays width-aware without threading geometry through keys.
    /// Zero until the first event; moves then skip scroll adjustment.
    pub(crate) fn set_content_width(&mut self, width: u16) {
        self.content_width = width;
    }
    pub(crate) fn scroll_detail(&mut self, delta: isize, max_scroll: usize) {
        let next = self.detail_scroll as isize + delta;
        self.detail_scroll = next.clamp(0, max_scroll as isize).max(0) as usize;
    }

    fn reset_detail_scroll(&mut self) {
        self.detail_scroll = 0;
    }

    pub(crate) const fn usage_expanded(&self) -> bool {
        self.usage_expanded
    }

    pub(crate) fn toggle_usage(&mut self) {
        self.usage_expanded = !self.usage_expanded;
    }

    /// Record a text mutation for autosave timing. Caret moves do not
    /// stamp: only real changes restart the delay.
    pub(crate) fn stamp_edit(&mut self) {
        if self.edit.is_some() {
            self.last_edit_at = Some(now_millis());
        }
    }

    /// Persist an idle edit without closing the session. The refresh
    /// keeps live sessions, so typing continues uninterrupted.
    pub(crate) fn autosave_tick(&mut self) -> LibraryInteraction {
        self.autosave_tick_at(now_millis())
    }

    pub(crate) fn autosave_tick_at(&mut self, now: u64) -> LibraryInteraction {
        let Some(stamped) = self.last_edit_at else {
            return LibraryInteraction::handled();
        };
        if now.saturating_sub(stamped) < AUTOSAVE_DELAY_MS {
            return LibraryInteraction::handled();
        }
        self.last_edit_at = None;
        self.build_edit_interaction()
    }

    /// Enable/disable toggle for the selected trigger. Returns a persist
    /// interaction; the caller refreshes the list on success.
    pub(crate) fn toggle_selected_enabled(&self) -> LibraryInteraction {
        let Some(selected) = self.selected_index() else {
            return LibraryInteraction::handled();
        };
        let Some(item) = self.item_at_filtered(selected) else {
            return LibraryInteraction::handled();
        };
        LibraryInteraction::toggle(crate::widgets::library::actions::PendingLibraryToggle {
            trigger_id: item.id().to_string(),
            enabled: !item.is_enabled(),
            restore_index: selected,
        })
    }

    pub(crate) const fn modal(&self) -> Option<&LibraryModal> {
        self.modal.as_ref()
    }

    // honey: unreachable until the shortcut rework lands; kept with tests.
    #[allow(dead_code)]
    pub(crate) fn open_export_modal(&mut self) {
        match LibraryExportModalState::new() {
            Ok(state) => self.modal = Some(LibraryModal::Export(state)),
            Err(error) => self.set_status_message(error.to_string()),
        }
    }

    // honey: unreachable until the shortcut rework lands; kept with tests.
    #[allow(dead_code)]
    pub(crate) fn open_import_modal(&mut self) {
        self.modal = Some(LibraryModal::Import(LibraryImportModalState::new()));
    }

    pub(crate) fn open_export_result_modal(&mut self, path: &Path) {
        self.modal = Some(LibraryModal::ExportResult(
            LibraryExportResultModalState::new(path),
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

    // honey: unreachable until the shortcut rework lands; kept with tests.
    #[allow(dead_code)]
    pub(crate) fn open_delete_modal_for_selected(&mut self) {
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

    /// Visible window preferring a click-time anchor so a clicked row stays
    /// where it was; falls back to bottom-anchored scrolling when stale.
    pub(crate) fn visible_window(&self, capacity: usize) -> (usize, usize) {
        let total = self.filtered_len();
        if capacity == 0 || total == 0 {
            return (0, 0);
        }
        let selected = self.selected_index().unwrap_or(0);
        if let Some(anchor) = self.window_anchor {
            let start = anchor.min(total.saturating_sub(1));
            let end = start.saturating_add(capacity).min(total);
            if selected >= start && selected < end {
                return (start, end);
            }
        }
        crate::widgets::util::visible_range(total, selected, capacity)
    }

    pub(crate) fn filtered_len(&self) -> usize {
        self.filtered_indices.len()
    }

    /// Startup selection: highlight the bottom row of the first window
    /// so the topmost entries stay visible with the cursor on the row
    /// touching the search bar.
    pub(crate) fn select_first_window_bottom(&mut self, capacity: usize) {
        let total = self.filtered_indices.len();
        if total == 0 || capacity == 0 {
            self.selected = 0;
            return;
        }
        self.selected = capacity.min(total).saturating_sub(1);
    }

    pub(crate) fn item_at_filtered(&self, index: usize) -> Option<&LibraryTrigger> {
        self.filtered_indices
            .get(index)
            .and_then(|item_index| self.items.get(*item_index))
    }

    pub(crate) fn select_by_id(&mut self, id: &str) -> bool {
        let Some(position) = self
            .filtered_indices
            .iter()
            .position(|item_index| self.items[*item_index].id() == id)
        else {
            return false;
        };
        self.selected = position;
        self.reset_detail_scroll();
        true
    }

    /// Reselect the exact alias row after a refresh. Same trigger id can
    /// own several rows (one per invocation), so the trigger string
    /// disambiguates; falls back to id match, then stays put.
    pub(crate) fn select_row(&mut self, id: &str, trigger: &str) -> bool {
        let position = self
            .filtered_indices
            .iter()
            .position(|item_index| {
                let item = &self.items[*item_index];
                item.id() == id && item.trigger() == trigger
            })
            .or_else(|| {
                self.filtered_indices
                    .iter()
                    .position(|item_index| self.items[*item_index].id() == id)
            });
        let Some(position) = position else {
            return false;
        };
        self.selected = position;
        self.reset_detail_scroll();
        true
    }

    pub(crate) fn select_after_delete(&mut self, previous_index: usize) {
        self.window_anchor = None;
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

        if self.edit.is_some() {
            return self.handle_edit_key(key);
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
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_selection(1);
                LibraryInteraction::handled()
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_selection(-1);
                LibraryInteraction::handled()
            }
            // honey: trigger editor removed pending revamp; Enter reserved.
            (KeyCode::Enter, KeyModifiers::NONE) => LibraryInteraction::handled(),
            // honey: navigation keys never start a search; any other
            // bare character filters the list immediately (type-to-search).
            (KeyCode::Char('1' | '2'), KeyModifiers::NONE) => LibraryInteraction::handled(),
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_mode = true;
                self.search.insert(ch);
                self.selected = 0;
                self.window_anchor = None;
                self.rebuild_filter();
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    /// Keys while a trigger part is being edited. Single-line parts
    /// (name, description): Enter commits, Esc cancels, Up/Down commit
    /// then move. Content: Enter breaks the line, Tab commits, arrows
    /// move the caret, Esc discards. Everything else is swallowed so
    /// typing never leaks into search.
    fn handle_edit_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        let is_content = self
            .edit
            .as_ref()
            .is_some_and(|edit| edit.target() == EditTarget::Content);
        if is_content {
            return self.handle_content_key(key);
        }
        match (key.code, key.modifiers) {
            // honey: Enter stays in the field; autosave persists.
            (KeyCode::Enter, KeyModifiers::NONE) => LibraryInteraction::handled(),
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.cancel_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                let interaction = self.commit_edit();
                self.move_selection(1);
                interaction
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                let interaction = self.commit_edit();
                self.move_selection(-1);
                interaction
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().backspace();
                }
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().delete_at();
                }
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().move_left();
                }
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().move_right();
                }
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().move_home();
                }
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().move_end();
                }
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Some(edit) = self.edit.as_mut() {
                    edit.push_char(ch);
                }
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_content_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.cancel_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Tab, KeyModifiers::NONE) => self.commit_edit(),
            (KeyCode::Enter, KeyModifiers::NONE) => {
                self.content_newline();
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.move_content_caret(|body| {
                    body.backspace();
                });
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.move_content_caret(|body| {
                    body.delete_at();
                });
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_left());
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_right());
                LibraryInteraction::handled()
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_up());
                LibraryInteraction::handled()
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_down());
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_home());
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_end());
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_content_char(ch);
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        let Some(modal) = self.modal.take() else {
            return LibraryInteraction::handled();
        };

        match modal {
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
                        self.modal = None;
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
                    self.modal = None;
                    LibraryInteraction::handled()
                }
                _ => {
                    self.modal = Some(LibraryModal::ConfirmDelete(state));
                    LibraryInteraction::handled()
                }
            },
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => self.search_mode = false,
            (KeyCode::Enter, KeyModifiers::NONE) => self.search_mode = false,
            (KeyCode::Up, KeyModifiers::NONE) => self.move_selection(-1),
            (KeyCode::Down, KeyModifiers::NONE) => self.move_selection(1),
            (KeyCode::Left, KeyModifiers::NONE) => self.search.move_left(),
            (KeyCode::Right, KeyModifiers::NONE) => self.search.move_right(),
            (KeyCode::Home, KeyModifiers::NONE) => self.search.move_home(),
            (KeyCode::End, KeyModifiers::NONE) => self.search.move_end(),
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                if self.search.backspace() {
                    self.rebuild_filter();
                }
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                if self.search.delete_at() {
                    self.rebuild_filter();
                }
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search.insert(ch);
                self.rebuild_filter();
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        self.window_anchor = None;
        self.reset_detail_scroll();
        let Some(current) = self.selected_index() else {
            self.selected = 0;
            return;
        };

        let max_index = self.filtered_indices.len().saturating_sub(1) as isize;
        let next = (current as isize + delta).clamp(0, max_index);
        self.selected = next as usize;
    }

    fn rebuild_filter(&mut self) {
        self.window_anchor = None;
        self.reset_detail_scroll();
        let previously_selected = self.selected_item().cloned();
        self.filtered_indices = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.matches_query(self.search_query()).then_some(index))
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

    pub(crate) fn activate_search_at(&mut self, cursor: usize) {
        self.search_mode = true;
        self.search.place(cursor);
    }

    /// Click selects the row and records the click-time window start so the
    /// list does not jump. A pending edit commits first when the click
    /// lands on another row; blank text silently no-ops.
    pub(crate) fn click_item(
        &mut self,
        filtered_position: usize,
        anchor: usize,
    ) -> LibraryInteraction {
        if self.edit.is_some() && Some(filtered_position) != self.selected_index() {
            let interaction = self.commit_edit();
            self.selected = filtered_position;
            self.window_anchor = Some(anchor);
            self.reset_detail_scroll();
            return interaction;
        }
        self.selected = filtered_position;
        self.window_anchor = Some(anchor);
        self.reset_detail_scroll();
        LibraryInteraction::handled()
    }
}
