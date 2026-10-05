mod app_filter;
mod delete;
mod edit;
mod export;
mod header_menu;
mod import;
mod tags;
mod trigger;

pub(crate) use app_filter::*;
pub(crate) use delete::*;
pub(crate) use edit::*;
pub(crate) use export::*;
pub(crate) use header_menu::*;
pub(crate) use import::*;
pub(crate) use tags::*;
pub(crate) use trigger::*;

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::widgets::field::TextField;
use crate::widgets::library::actions::{
    EditedField, LibraryImportOutcome, LibraryInteraction, PendingLibraryDelete,
    PendingLibraryEdit, PreparedLibraryImport,
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
    HeaderMenu(LibraryHeaderMenuState),
    Tags(LibraryTagsModalState),
    AppFilter(LibraryAppFilterState),
}

/// Discriminant for `LibraryModal`; see `LibraryModal::kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryModalKind {
    Export,
    ExportResult,
    Import,
    ImportResult,
    ConfirmImportRunVariables,
    ConfirmDelete,
    HeaderMenu,
    Tags,
    AppFilter,
}

impl LibraryModal {
    /// Discriminant for overlay bookkeeping (layer tracking, open
    /// transitions): hover, scroll, and live reseed never change it, so
    /// same-kind updates never retrigger an appear sweep.
    pub(crate) const fn kind(&self) -> LibraryModalKind {
        match self {
            Self::Export(_) => LibraryModalKind::Export,
            Self::ExportResult(_) => LibraryModalKind::ExportResult,
            Self::Import(_) => LibraryModalKind::Import,
            Self::ImportResult(_) => LibraryModalKind::ImportResult,
            Self::ConfirmImportRunVariables(_) => LibraryModalKind::ConfirmImportRunVariables,
            Self::ConfirmDelete(_) => LibraryModalKind::ConfirmDelete,
            Self::HeaderMenu(_) => LibraryModalKind::HeaderMenu,
            Self::Tags(_) => LibraryModalKind::Tags,
            Self::AppFilter(_) => LibraryModalKind::AppFilter,
        }
    }

    pub(crate) fn set_error(&mut self, error: String) {
        match self {
            Self::Export(state) => state.set_error(error),
            Self::ExportResult(state) => state.set_error(error),
            Self::Import(state) => state.set_error(error),
            Self::ImportResult(state) => state.set_error(error),
            Self::ConfirmImportRunVariables(state) => state.set_error(error),
            Self::ConfirmDelete(state) => state.set_error(error),
            // honey: menu confirms carry no typed text to restore, so a
            // failed pick only surfaces through the status line.
            Self::HeaderMenu(_) => {}
            Self::Tags(state) => state.set_error(error),
            Self::AppFilter(state) => state.set_error(error),
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
    last_content_click: Option<(u64, String, usize, usize)>,
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
            last_content_click: None,
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

    /// Second click on the same divider within the window toggles that
    /// pane: a default-width pane collapses to the edge, a collapsed
    /// pane resets to its default width, and a custom width resets to
    /// default. Returns true when the width changed.
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
        // honey: drag math rarely lands exactly on a constant, so
        // near-enough counts for both ends of the toggle.
        const NEAR: f32 = 0.0001;
        match side {
            super::DividerSide::List => {
                let ratio = self.split_ratio();
                if ratio <= super::MIN_SPLIT_RATIO + NEAR {
                    self.set_split_ratio(super::DEFAULT_SPLIT_RATIO);
                } else if (ratio - super::DEFAULT_SPLIT_RATIO).abs() <= NEAR {
                    self.set_split_ratio(super::MIN_SPLIT_RATIO);
                } else {
                    self.set_split_ratio(super::DEFAULT_SPLIT_RATIO);
                }
            }
            super::DividerSide::Props => {
                let ratio = self.detail_ratio();
                if ratio <= super::MIN_DETAIL_RATIO + NEAR {
                    self.set_detail_ratio(super::DEFAULT_DETAIL_RATIO);
                } else if (ratio - super::DEFAULT_DETAIL_RATIO).abs() <= NEAR {
                    self.set_detail_ratio(super::MIN_DETAIL_RATIO);
                } else {
                    self.set_detail_ratio(super::DEFAULT_DETAIL_RATIO);
                }
            }
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

    /// Shared bool-toggle builder for the properties pane: picks the
    /// flipped field from the selected row, or nothing when the row
    /// carries no such switch. The caller refreshes on success.
    fn toggle_selected_field(
        &self,
        pick: impl FnOnce(&LibraryTrigger) -> Option<EditedField>,
    ) -> LibraryInteraction {
        let Some(selected) = self.selected_index() else {
            return LibraryInteraction::handled();
        };
        let Some(item) = self.item_at_filtered(selected) else {
            return LibraryInteraction::handled();
        };
        let Some(field) = pick(item) else {
            return LibraryInteraction::handled();
        };
        LibraryInteraction::edit(PendingLibraryEdit {
            trigger_id: item.id().to_string(),
            trigger: item.trigger().to_string(),
            field,
            restore_index: selected,
        })
    }

    /// Auto-case toggle for the selected trigger. Returns a persist
    /// interaction; the caller refreshes the list on success.
    pub(crate) fn toggle_selected_auto_case(&self) -> LibraryInteraction {
        self.toggle_selected_field(|item| Some(EditedField::AutoCase(!item.auto_case())))
    }

    /// Confirmation toggle for the selected voice trigger. Non-voice
    /// rows carry no switch and stay handled.
    pub(crate) fn toggle_selected_require_confirmation(&self) -> LibraryInteraction {
        self.toggle_selected_field(|item| {
            item.is_voice()
                .then(|| EditedField::RequireConfirmation(!item.require_confirmation()))
        })
    }

    /// Current option label for a header menu on the selected row.
    fn header_menu_current(&self, kind: HeaderMenuKind) -> Option<String> {
        let selected = self.selected_index()?;
        let item = self.item_at_filtered(selected)?;
        Some(match kind {
            HeaderMenuKind::InvocationType => item.invocation_type_label().to_string(),
            HeaderMenuKind::Interpreter => item
                .interpreter()
                .map(|interpreter| interpreter.as_str())
                .unwrap_or(super::detail::EMPTY_TOKEN)
                .to_string(),
            HeaderMenuKind::Behavior => item
                .behavior()
                .map(|behavior| behavior.as_str())
                .unwrap_or(super::detail::EMPTY_TOKEN)
                .to_string(),
            HeaderMenuKind::Platform => item.target_os.clone(),
        })
    }

    /// Opens the header-button menu as a centered overlay with the cursor
    /// parked on the current value.
    pub(crate) fn open_header_menu(&mut self, kind: HeaderMenuKind) {
        let Some(current) = self.header_menu_current(kind) else {
            return;
        };
        self.modal = Some(LibraryModal::HeaderMenu(LibraryHeaderMenuState::new(
            kind, &current,
        )));
    }

    pub(crate) fn header_menu_open(&self) -> bool {
        matches!(self.modal, Some(LibraryModal::HeaderMenu(_)))
    }

    pub(crate) fn tags_menu_open(&self) -> bool {
        matches!(self.modal, Some(LibraryModal::Tags(_)))
    }

    pub(crate) fn app_filter_menu_open(&self) -> bool {
        matches!(self.modal, Some(LibraryModal::AppFilter(_)))
    }

    /// Opens the allow/block picker for the selected trigger, seeded
    /// with its stored filters plus the live Alt-Tab list.
    pub(crate) fn open_app_filter_modal(&mut self, side: AppFilterSide) {
        let Some(selected) = self.selected_index() else {
            return;
        };
        let Some(item) = self.item_at_filtered(selected) else {
            return;
        };
        let (stored, other) = match side {
            AppFilterSide::Allow => (item.only_apps(), item.except_apps()),
            AppFilterSide::Block => (item.except_apps(), item.only_apps()),
        };
        let split = |value: Option<&str>| {
            value
                .map(taurine_core::db::crud::split_app_filters)
                .unwrap_or_default()
        };
        let menu = LibraryAppFilterState::new(
            item.id().to_string(),
            selected,
            side,
            split(stored),
            split(other),
            taurine_core::system::foreground_apps::list_foreground_apps(),
        );
        self.modal = Some(LibraryModal::AppFilter(menu));
    }

    /// Re-seeds an open app-filter menu from refreshed rows after a
    /// live write. A vanished trigger closes the menu with it.
    pub(crate) fn sync_app_filter_modal(&mut self, trigger_id: &str) {
        let found = self
            .items
            .iter()
            .find(|item| item.id() == trigger_id)
            .map(|item| (item.only_apps(), item.except_apps()));
        match (self.modal.take(), found) {
            (Some(LibraryModal::AppFilter(mut state)), Some((only, except))) => {
                let split = |value: Option<&str>| {
                    value
                        .map(taurine_core::db::crud::split_app_filters)
                        .unwrap_or_default()
                };
                let (stored, other) = match state.side() {
                    AppFilterSide::Allow => (split(only), split(except)),
                    AppFilterSide::Block => (split(except), split(only)),
                };
                state.reseed(stored, other);
                self.modal = Some(LibraryModal::AppFilter(state));
            }
            (modal, _) => {
                self.modal = modal;
            }
        }
    }

    /// Clicks on an open app-filter menu: rows toggle live, the add
    /// row opens the manual input, outside closes the menu.
    pub(crate) fn click_app_filter_menu(
        &mut self,
        area: ratatui::layout::Rect,
        column: u16,
        row: u16,
    ) -> LibraryInteraction {
        let Some(LibraryModal::AppFilter(mut menu)) = self.modal.take() else {
            return LibraryInteraction::handled();
        };
        let (_, body, max_lines) = app_filter_geometry(area, &menu);
        // honey: an outside click closes by leaving the taken modal
        // dropped; picks keep the menu open across live writes.
        if body.width == 0
            || column < body.x
            || column >= body.x.saturating_add(body.width)
            || row < body.y.saturating_add(1)
        {
            return LibraryInteraction::handled();
        }
        menu.set_view_lines(body.height);
        // honey: the search box sits exactly one blank line below the
        // visible rows; a click there places the caret, exactly like
        // the library search bar.
        let search_row = body
            .y
            .saturating_add(APP_FILTER_ROWS_TOP)
            .saturating_add(menu.search_rel(max_lines));
        if row == search_row {
            let field = menu.search_mut();
            let column = column.saturating_sub(body.x).saturating_sub(APP_FILTER_PAD) as usize;
            field.place(field.index_at(column));
            menu.refilter();
            self.modal = Some(LibraryModal::AppFilter(menu));
            return LibraryInteraction::handled();
        }
        // honey: rows start below the title blank; the outside
        // guard above already closes title-row clicks, and the blank
        // line stays dead so saturating math never folds it onto row 0.
        if row < body.y.saturating_add(APP_FILTER_ROWS_TOP) {
            self.modal = Some(LibraryModal::AppFilter(menu));
            return LibraryInteraction::handled();
        }
        let (_, _, right_x, _) = menu.panes(body.width);
        let rel = row.saturating_sub(body.y.saturating_add(APP_FILTER_ROWS_TOP));
        // honey: the right pane stays empty for now; clicks there are dead.
        if column >= body.x.saturating_add(right_x) {
            self.modal = Some(LibraryModal::AppFilter(menu));
            return LibraryInteraction::handled();
        }
        let Some(hit) = menu.row_at(max_lines, rel) else {
            self.modal = Some(LibraryModal::AppFilter(menu));
            return LibraryInteraction::handled();
        };
        if matches!(hit, FilterRow::Foreground(index) if menu.is_gray(index)) {
            menu.set_error(format!(
                "That app is already {} for this trigger.",
                menu.side().opposite_hint()
            ));
            self.modal = Some(LibraryModal::AppFilter(menu));
            return LibraryInteraction::handled();
        }
        menu.set_cursor(hit);
        let interaction = menu
            .toggle_focused()
            .map(LibraryInteraction::edit)
            .unwrap_or_else(LibraryInteraction::handled);
        self.modal = Some(LibraryModal::AppFilter(menu));
        interaction
    }

    /// Mouse moves over an open app-filter menu focus the hovered row
    /// so the full-row band follows the mouse; gray rows refuse.
    pub(crate) fn hover_app_filter_menu(
        &mut self,
        area: ratatui::layout::Rect,
        column: u16,
        row: u16,
    ) {
        let Some(LibraryModal::AppFilter(menu)) = self.modal.as_mut() else {
            return;
        };
        let (_, body, max_lines) = app_filter_geometry(area, menu);
        if body.width == 0
            || column < body.x
            || column >= body.x.saturating_add(body.width)
            || row < body.y.saturating_add(APP_FILTER_ROWS_TOP)
        {
            return;
        }
        let rel = row.saturating_sub(body.y.saturating_add(APP_FILTER_ROWS_TOP));
        if let Some(hit) = menu.row_at(max_lines, rel) {
            menu.set_cursor(hit);
        }
    }

    /// Wheel scroll over an open app-filter menu walks the cursor.
    pub(crate) fn move_app_filter_cursor(&mut self, down: bool) {
        if let Some(LibraryModal::AppFilter(menu)) = self.modal.as_mut() {
            menu.move_cursor(if down { 1 } else { -1 });
        }
    }

    /// Grabbing the picker divider starts a resize drag. Returns true
    /// when the press lands on the divider inside the rows area.
    pub(crate) fn grab_app_filter_divider(
        &mut self,
        area: ratatui::layout::Rect,
        column: u16,
        row: u16,
    ) -> bool {
        let Some(LibraryModal::AppFilter(menu)) = self.modal.as_mut() else {
            return false;
        };
        let (_, body, max_lines) = app_filter_geometry(area, menu);
        let (_, divider_x, _, _) = menu.panes(body.width);
        let origin = body.y.saturating_add(APP_FILTER_ROWS_TOP);
        let search_row = origin.saturating_add(menu.search_rel(max_lines));
        if column == body.x.saturating_add(divider_x) && row >= origin && row <= search_row {
            menu.set_divider_drag(true);
            return true;
        }
        false
    }

    /// Pointer motion while grabbed resizes the picker panes.
    pub(crate) fn drag_app_filter_divider(&mut self, area: ratatui::layout::Rect, column: u16) {
        let Some(LibraryModal::AppFilter(menu)) = self.modal.as_mut() else {
            return;
        };
        if !menu.divider_drag() {
            return;
        }
        let (_, body, _) = app_filter_geometry(area, menu);
        menu.set_split(body.x, body.width, column);
    }

    /// Pointer release ends a picker divider drag.
    pub(crate) fn release_app_filter_divider(&mut self) {
        if let Some(LibraryModal::AppFilter(menu)) = self.modal.as_mut() {
            menu.set_divider_drag(false);
        }
    }

    /// Clicks on an open tags menu: the delete icon removes that
    /// chip, chip bodies and the `+` row focus and act, outside
    /// closes the menu. The menu stays open across live writes.
    pub(crate) fn click_tags_menu(
        &mut self,
        area: ratatui::layout::Rect,
        column: u16,
        row: u16,
    ) -> LibraryInteraction {
        let Some(LibraryModal::Tags(mut menu)) = self.modal.take() else {
            return LibraryInteraction::handled();
        };
        let popup = crate::widgets::util::overlay_popup(area);
        let body = crate::widgets::util::overlay_body(popup);
        menu.set_cloud_width(body.width);
        // honey: an outside click closes by leaving the taken modal
        // dropped; picks keep the menu open across live writes.
        if body.width == 0
            || column < body.x
            || column >= body.x.saturating_add(body.width)
            || row < body.y.saturating_add(2)
        {
            return LibraryInteraction::handled();
        }
        let max_rows = body.height.saturating_sub(3);
        let (_, add) = menu.paint_layout(body.width, max_rows);
        let rel = row.saturating_sub(body.y.saturating_add(2));
        let x = column.saturating_sub(body.x);
        // honey: chips win over the `+` row; the trailing `+`
        // shares its row with the last chip.
        if let Some(cell) = menu.chip_at(body.width, max_rows, rel, x) {
            if menu.input_active() {
                // honey: clicking a chip while typing cancels the
                // draft, then the click focuses as usual.
                menu.cancel_input();
            }
            // honey: the close X lives in the chip's first cell on
            // the hovered chip; one click removes it, no select
            // step. Anywhere else on the chip parks focus.
            let x_cell = body.x.saturating_add(cell.x);
            menu.set_hover(Some(cell.index));
            let interaction = if column == x_cell {
                menu.remove_at(cell.index)
                    .map(LibraryInteraction::edit)
                    .unwrap_or_else(LibraryInteraction::handled)
            } else {
                menu.set_focus(cell.index);
                LibraryInteraction::handled()
            };
            self.modal = Some(LibraryModal::Tags(menu));
            return interaction;
        }
        if Some(rel) == add && (menu.input_active() || menu.plus_hit_accepts(body.width, x)) {
            if menu.input_active() {
                // honey: a click on the live input row places the
                // caret; the `+` it replaced starts at plus_cell.
                let (plus_x, _) = menu.plus_cell(body.width);
                let column = column.saturating_sub(body.x.saturating_add(plus_x).saturating_add(1));
                menu.place_input(column as usize);
            } else {
                menu.start_input();
            }
            self.modal = Some(LibraryModal::Tags(menu));
            return LibraryInteraction::handled();
        }
        menu.set_hover(None);
        self.modal = Some(LibraryModal::Tags(menu));
        LibraryInteraction::handled()
    }

    /// Mouse moves over an open tags menu reveal the close icon on
    /// the hovered chip only; leaving the cloud hides it again.
    pub(crate) fn hover_tags_menu(&mut self, area: ratatui::layout::Rect, column: u16, row: u16) {
        let Some(LibraryModal::Tags(menu)) = self.modal.as_mut() else {
            return;
        };
        let popup = crate::widgets::util::overlay_popup(area);
        let body = crate::widgets::util::overlay_body(popup);
        if body.width == 0
            || column < body.x
            || column >= body.x.saturating_add(body.width)
            || row < body.y.saturating_add(2)
        {
            menu.set_hover(None);
            return;
        }
        let max_rows = body.height.saturating_sub(3);
        let rel = row.saturating_sub(body.y.saturating_add(2));
        let x = column.saturating_sub(body.x);
        menu.set_hover(
            menu.chip_at(body.width, max_rows, rel, x)
                .map(|cell| cell.index),
        );
    }

    /// Opens the tags builder for the selected trigger, seeded with
    /// its current tags.
    pub(crate) fn open_tags_modal(&mut self, cloud_width: u16) {
        let Some(selected) = self.selected_index() else {
            return;
        };
        let Some(item) = self.item_at_filtered(selected) else {
            return;
        };
        let mut menu =
            LibraryTagsModalState::new(item.id().to_string(), selected, item.tags().to_vec());
        menu.set_cloud_width(cloud_width);
        self.modal = Some(LibraryModal::Tags(menu));
    }

    /// Re-seeds an open tags menu from refreshed rows after a live
    /// toggle, so core-side normalization never desyncs it. A vanished
    /// trigger closes the menu with it.
    pub(crate) fn sync_tags_modal(&mut self, trigger_id: &str) {
        let tags = self
            .items
            .iter()
            .find(|item| item.id() == trigger_id)
            .map(|item| item.tags().to_vec());
        match (self.modal.take(), tags) {
            (Some(LibraryModal::Tags(mut state)), Some(tags)) => {
                state.reseed(tags);
                self.modal = Some(LibraryModal::Tags(state));
            }
            (modal, _) => {
                self.modal = modal;
            }
        }
    }

    /// Wheel scroll over an open header menu walks the cursor.
    pub(crate) fn move_header_menu_cursor(&mut self, down: bool) {
        if let Some(LibraryModal::HeaderMenu(menu)) = self.modal.as_mut() {
            menu.move_cursor(if down { 1 } else { -1 });
        }
    }

    /// Clicks on an open header menu confirm the clicked option and
    /// close anything else.
    pub(crate) fn click_header_menu(
        &mut self,
        area: ratatui::layout::Rect,
        column: u16,
        row: u16,
    ) -> LibraryInteraction {
        let Some(LibraryModal::HeaderMenu(mut menu)) = self.modal.take() else {
            return LibraryInteraction::handled();
        };
        let line_counts: Vec<u16> = menu
            .details()
            .iter()
            .map(|detail| if detail.is_empty() { 1 } else { 2 })
            .collect();
        match crate::widgets::util::overlay_option_hit(area, &line_counts, column, row) {
            Some(index) => {
                menu.set_selected(index);
                self.confirm_header_menu(menu)
            }
            None => LibraryInteraction::handled(),
        }
    }

    /// Commits the menu cursor as a persist interaction. Unchanged values
    /// close silently; the caller refreshes the list on success.
    fn confirm_header_menu(&mut self, menu: LibraryHeaderMenuState) -> LibraryInteraction {
        let Some(selected) = self.selected_index() else {
            return LibraryInteraction::handled();
        };
        let Some(item) = self.item_at_filtered(selected).cloned() else {
            return LibraryInteraction::handled();
        };
        let Some(option) = menu.selected_option().map(str::to_string) else {
            return LibraryInteraction::handled();
        };
        let current = match menu.kind() {
            HeaderMenuKind::InvocationType => item.invocation_type_label().to_string(),
            HeaderMenuKind::Interpreter => item
                .interpreter()
                .map(|interpreter| interpreter.as_str())
                .unwrap_or(super::detail::EMPTY_TOKEN)
                .to_string(),
            HeaderMenuKind::Behavior => item
                .behavior()
                .map(|behavior| behavior.as_str())
                .unwrap_or(super::detail::EMPTY_TOKEN)
                .to_string(),
            HeaderMenuKind::Platform => item.target_os.clone(),
        };
        if option == current {
            return LibraryInteraction::handled();
        }
        let field = match menu.kind() {
            HeaderMenuKind::InvocationType => {
                taurine_core::db::crud::InvocationType::parse_str(&option)
                    .map(EditedField::InvocationType)
            }
            HeaderMenuKind::Interpreter => {
                taurine_core::engine::shell::ScriptInterpreter::parse_str(&option)
                    .map(EditedField::Interpreter)
            }
            HeaderMenuKind::Behavior => {
                taurine_core::engine::shell::ScriptBehavior::parse_str(&option)
                    .map(EditedField::Behavior)
            }
            HeaderMenuKind::Platform => taurine_core::db::TargetOs::parse_str(&option)
                .map(|os| EditedField::TargetOs(os.to_db_str().to_string())),
        };
        let Some(field) = field else {
            return LibraryInteraction::handled();
        };
        LibraryInteraction::edit(PendingLibraryEdit {
            trigger_id: item.id().to_string(),
            trigger: item.trigger().to_string(),
            field,
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
            (KeyCode::Backspace, KeyModifiers::CONTROL) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().delete_word_before();
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
            (KeyCode::Delete, KeyModifiers::CONTROL) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().delete_word_after();
                }
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::CONTROL) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().move_word_left();
                }
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::CONTROL) => {
                if let Some(edit) = self.edit.as_mut() {
                    edit.line_mut().move_word_right();
                }
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
        use crate::widgets::library::actions::{read_clipboard, write_clipboard};
        use crossterm::event::KeyModifiers as M;
        match (key.code, key.modifiers) {
            // honey: autosave owns persistence; Esc only exits the field.
            (KeyCode::Esc, KeyModifiers::NONE) => self.commit_edit(),
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.indent());
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.move_content_caret(|body| body.outdent());
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                self.content_newline();
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Char('a'), M::CONTROL) => {
                self.move_content_caret(|body| body.select_all());
                LibraryInteraction::handled()
            }
            (KeyCode::Char('z'), M::CONTROL) => {
                self.move_content_caret(|body| {
                    body.undo();
                });
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Char('y'), M::CONTROL) => {
                self.move_content_caret(|body| {
                    body.redo();
                });
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Char('x'), M::CONTROL) => {
                self.move_content_caret(|body| {
                    if let Some(text) = body.selected_text() {
                        write_clipboard(&text);
                        body.delete_selection();
                    }
                });
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Char('c'), M::CONTROL) => {
                self.move_content_caret(|body| {
                    if let Some(text) = body.selected_text() {
                        write_clipboard(&text);
                    }
                });
                LibraryInteraction::handled()
            }
            (KeyCode::Char('v'), M::CONTROL) => {
                if let Some(text) = read_clipboard()
                    && !text.is_empty()
                {
                    self.move_content_caret(|body| body.insert_text(&text));
                    self.stamp_edit();
                }
                LibraryInteraction::handled()
            }
            (KeyCode::Backspace, M::CONTROL) => {
                self.move_content_caret(|body| {
                    body.delete_word_before();
                });
                self.stamp_edit();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, M::CONTROL) => {
                self.move_content_caret(|body| {
                    body.delete_word_after();
                });
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
            (KeyCode::Left, M::CONTROL) => {
                self.move_content_caret(|body| body.move_word_left(false));
                LibraryInteraction::handled()
            }
            (KeyCode::Right, M::CONTROL) => {
                self.move_content_caret(|body| body.move_word_right(false));
                LibraryInteraction::handled()
            }
            (KeyCode::Left, modifiers) if modifiers == (M::CONTROL | M::SHIFT) => {
                self.move_content_caret(|body| body.move_word_left(true));
                LibraryInteraction::handled()
            }
            (KeyCode::Right, modifiers) if modifiers == (M::CONTROL | M::SHIFT) => {
                self.move_content_caret(|body| body.move_word_right(true));
                LibraryInteraction::handled()
            }
            (KeyCode::Home, M::CONTROL) => {
                self.move_content_caret(|body| body.move_top(false));
                LibraryInteraction::handled()
            }
            (KeyCode::End, M::CONTROL) => {
                self.move_content_caret(|body| body.move_bottom(false));
                LibraryInteraction::handled()
            }
            (KeyCode::Home, modifiers) if modifiers == (M::CONTROL | M::SHIFT) => {
                self.move_content_caret(|body| body.move_top(true));
                LibraryInteraction::handled()
            }
            (KeyCode::End, modifiers) if modifiers == (M::CONTROL | M::SHIFT) => {
                self.move_content_caret(|body| body.move_bottom(true));
                LibraryInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_left(false));
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_right(false));
                LibraryInteraction::handled()
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_up(false));
                LibraryInteraction::handled()
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_down(false));
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_home(false));
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.move_content_caret(|body| body.move_end(false));
                LibraryInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::SHIFT) => {
                self.move_content_caret(|body| body.move_left(true));
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::SHIFT) => {
                self.move_content_caret(|body| body.move_right(true));
                LibraryInteraction::handled()
            }
            (KeyCode::Up, KeyModifiers::SHIFT) => {
                self.move_content_caret(|body| body.move_up(true));
                LibraryInteraction::handled()
            }
            (KeyCode::Down, KeyModifiers::SHIFT) => {
                self.move_content_caret(|body| body.move_down(true));
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::SHIFT) => {
                self.move_content_caret(|body| body.move_home(true));
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::SHIFT) => {
                self.move_content_caret(|body| body.move_end(true));
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
            // honey: header menus close on confirm so a failed pick
            // surfaces through the status line, not a reopened menu.
            LibraryModal::HeaderMenu(mut state) => match (key.code, key.modifiers) {
                (KeyCode::Up, KeyModifiers::NONE) => {
                    state.move_cursor(-1);
                    self.modal = Some(LibraryModal::HeaderMenu(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Down, KeyModifiers::NONE) => {
                    state.move_cursor(1);
                    self.modal = Some(LibraryModal::HeaderMenu(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Enter, KeyModifiers::NONE) => self.confirm_header_menu(state),
                (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.modal = None;
                    LibraryInteraction::handled()
                }
                _ => {
                    self.modal = Some(LibraryModal::HeaderMenu(state));
                    LibraryInteraction::handled()
                }
            },
            // honey: chips view never captures text; arrows move
            // focus, Delete removes, Enter opens the add step on the
            // `+` row. The add view owns the filter keystrokes.
            LibraryModal::Tags(mut state) => match (key.code, key.modifiers) {
                (KeyCode::Esc, KeyModifiers::NONE) => {
                    if state.input_active() {
                        state.cancel_input();
                        self.modal = Some(LibraryModal::Tags(state));
                    } else {
                        self.modal = None;
                    }
                    LibraryInteraction::handled()
                }
                (KeyCode::Up, KeyModifiers::NONE) => {
                    if !state.input_active() {
                        let width = state.cloud_width();
                        state.move_focus(width, 0, -1);
                    }
                    self.modal = Some(LibraryModal::Tags(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Down, KeyModifiers::NONE) => {
                    if !state.input_active() {
                        let width = state.cloud_width();
                        state.move_focus(width, 0, 1);
                    }
                    self.modal = Some(LibraryModal::Tags(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Left, KeyModifiers::NONE) => {
                    if state.input_active() {
                        state.input_mut().move_left();
                    } else {
                        let width = state.cloud_width();
                        state.move_focus(width, -1, 0);
                    }
                    self.modal = Some(LibraryModal::Tags(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Right, KeyModifiers::NONE) => {
                    if state.input_active() {
                        state.input_mut().move_right();
                    } else {
                        let width = state.cloud_width();
                        state.move_focus(width, 1, 0);
                    }
                    self.modal = Some(LibraryModal::Tags(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    if state.input_active() {
                        match state.confirm_input() {
                            crate::widgets::library::state::InputConfirm::Save(pending) => {
                                self.modal = Some(LibraryModal::Tags(state));
                                LibraryInteraction::edit(pending)
                            }
                            _ => {
                                self.modal = Some(LibraryModal::Tags(state));
                                LibraryInteraction::handled()
                            }
                        }
                    } else if state.focus_is_add_row() {
                        state.start_input();
                        self.modal = Some(LibraryModal::Tags(state));
                        LibraryInteraction::handled()
                    } else {
                        self.modal = Some(LibraryModal::Tags(state));
                        LibraryInteraction::handled()
                    }
                }
                (KeyCode::Delete, KeyModifiers::NONE) => {
                    if state.input_active() {
                        state.delete_input_at();
                        self.modal = Some(LibraryModal::Tags(state));
                        LibraryInteraction::handled()
                    } else {
                        let interaction = state
                            .remove_focused()
                            .map(LibraryInteraction::edit)
                            .unwrap_or_else(LibraryInteraction::handled);
                        self.modal = Some(LibraryModal::Tags(state));
                        interaction
                    }
                }
                (KeyCode::Backspace, KeyModifiers::NONE) => {
                    if state.input_active() {
                        state.backspace_input();
                    }
                    self.modal = Some(LibraryModal::Tags(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Home, KeyModifiers::NONE) => {
                    if state.input_active() {
                        state.input_mut().move_home();
                    }
                    self.modal = Some(LibraryModal::Tags(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::End, KeyModifiers::NONE) => {
                    if state.input_active() {
                        state.input_mut().move_end();
                    }
                    self.modal = Some(LibraryModal::Tags(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Char(ch), modifiers)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if !state.input_active() {
                        state.start_input();
                    }
                    state.push_input(ch);
                    self.modal = Some(LibraryModal::Tags(state));
                    LibraryInteraction::handled()
                }
                _ => {
                    self.modal = Some(LibraryModal::Tags(state));
                    LibraryInteraction::handled()
                }
            },
            // honey: the app-filter picker toggles live; gray rows
            // are skipped by the cursor and refuse clicks.
            LibraryModal::AppFilter(mut state) => match (key.code, key.modifiers) {
                (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.modal = None;
                    LibraryInteraction::handled()
                }
                (KeyCode::Up, KeyModifiers::NONE) => {
                    state.move_cursor(-1);
                    self.modal = Some(LibraryModal::AppFilter(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Down, KeyModifiers::NONE) => {
                    state.move_cursor(1);
                    self.modal = Some(LibraryModal::AppFilter(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Left, KeyModifiers::NONE) => {
                    state.search_mut().move_left();
                    self.modal = Some(LibraryModal::AppFilter(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Right, KeyModifiers::NONE) => {
                    state.search_mut().move_right();
                    self.modal = Some(LibraryModal::AppFilter(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    let interaction = state
                        .toggle_focused()
                        .map(LibraryInteraction::edit)
                        .unwrap_or_else(LibraryInteraction::handled);
                    self.modal = Some(LibraryModal::AppFilter(state));
                    interaction
                }
                (KeyCode::Delete, KeyModifiers::NONE) => {
                    if state.search().cursor() < state.search().len_chars() {
                        // honey: caret inside the query deletes there;
                        // at the end it removes the focused row.
                        state.search_mut().delete_at();
                        state.refilter();
                        self.modal = Some(LibraryModal::AppFilter(state));
                        return LibraryInteraction::handled();
                    }
                    let interaction = state
                        .remove_focused()
                        .map(LibraryInteraction::edit)
                        .unwrap_or_else(LibraryInteraction::handled);
                    self.modal = Some(LibraryModal::AppFilter(state));
                    interaction
                }
                (KeyCode::Backspace, KeyModifiers::NONE) => {
                    state.search_mut().backspace();
                    state.refilter();
                    self.modal = Some(LibraryModal::AppFilter(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Home, KeyModifiers::NONE) => {
                    state.search_mut().move_home();
                    self.modal = Some(LibraryModal::AppFilter(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::End, KeyModifiers::NONE) => {
                    state.search_mut().move_end();
                    self.modal = Some(LibraryModal::AppFilter(state));
                    LibraryInteraction::handled()
                }
                (KeyCode::Char(ch), modifiers)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    // honey: typing always filters the foreground list.
                    state.search_mut().insert(ch);
                    state.refilter();
                    self.modal = Some(LibraryModal::AppFilter(state));
                    LibraryInteraction::handled()
                }
                _ => {
                    self.modal = Some(LibraryModal::AppFilter(state));
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
            (KeyCode::Left, KeyModifiers::CONTROL) => self.search.move_word_left(),
            (KeyCode::Right, KeyModifiers::CONTROL) => self.search.move_word_right(),
            (KeyCode::Home, KeyModifiers::NONE) => self.search.move_home(),
            (KeyCode::End, KeyModifiers::NONE) => self.search.move_end(),
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                if self.search.backspace() {
                    self.rebuild_filter();
                }
            }
            (KeyCode::Backspace, KeyModifiers::CONTROL) => {
                if self.search.delete_word_before() {
                    self.rebuild_filter();
                }
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                if self.search.delete_at() {
                    self.rebuild_filter();
                }
            }
            (KeyCode::Delete, KeyModifiers::CONTROL) => {
                if self.search.delete_word_after() {
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
