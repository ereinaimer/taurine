use crate::widgets::field::TextField;
use crate::widgets::library::actions::{EditedField, PendingLibraryEdit};
use crate::widgets::library::state::InputConfirm;
use taurine_core::db::crud::AppFilterPrefix;
use taurine_core::system::foreground_apps::ForegroundApp;

/// Foreground cap: Z-order truncation is free most-recent-first,
/// and stored filters always show regardless of the cap.
pub(crate) const MAX_FOREGROUND_APPS: usize = 8;

/// Reserved body lines: title, blank, blank, search, footer error.
pub(crate) const APP_FILTER_RESERVED_LINES: u16 = 5;

/// Rows start below the title and one blank line.
pub(crate) const APP_FILTER_ROWS_TOP: u16 = 2;

/// Which props row opened the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppFilterSide {
    Allow,
    Block,
}

impl AppFilterSide {
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Allow => "Allow on",
            Self::Block => "Block on",
        }
    }

    pub(crate) const fn opposite_hint(self) -> &'static str {
        match self {
            Self::Allow => "blocked",
            Self::Block => "allowed",
        }
    }

    const fn field(self, apps: Vec<String>) -> EditedField {
        match self {
            Self::Allow => EditedField::OnlyApps(apps),
            Self::Block => EditedField::ExceptApps(apps),
        }
    }
}

/// Gray key: lowercase, known prefix stripped, `.exe` stripped, so
/// `exe:code`, `CODE`, and `code.exe` all collide.
pub(crate) fn gray_key(item: &str) -> String {
    let lower = item.trim().to_lowercase();
    let bare = match lower.split_once(':') {
        Some((prefix, rest)) if AppFilterPrefix::parse_prefix(prefix).is_some() => rest,
        _ => lower.as_str(),
    };
    bare.strip_suffix(".exe").unwrap_or(bare).to_string()
}

/// One pickable row: a stored filter, a foreground app, or the add row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilterRow {
    Checked(usize),
    Foreground(usize),
    Add,
}

impl FilterRow {
    pub(crate) const fn height(self) -> u16 {
        match self {
            Self::Foreground(_) => 2,
            _ => 1,
        }
    }
}

/// App-filter picker: stored filters first (toggle off), then up
/// to eight foreground apps (`exe:` picks with title/class context)
/// narrowed by the search box, then the manual add row. Every write
/// hits the database live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryAppFilterState {
    trigger_id: String,
    restore_index: usize,
    side: AppFilterSide,
    checked: Vec<String>,
    opposite_keys: Vec<String>,
    foreground: Vec<ForegroundApp>,
    search: TextField,
    cursor: usize,
    scroll: usize,
    view_lines: u16,
    input_active: bool,
    input: TextField,
    error: Option<String>,
}

impl LibraryAppFilterState {
    pub(crate) fn new(
        trigger_id: String,
        restore_index: usize,
        side: AppFilterSide,
        checked: Vec<String>,
        opposite: Vec<String>,
        foreground: Vec<ForegroundApp>,
    ) -> Self {
        Self {
            trigger_id,
            restore_index,
            side,
            checked,
            opposite_keys: opposite.iter().map(|item| gray_key(item)).collect(),
            foreground: foreground.into_iter().take(MAX_FOREGROUND_APPS).collect(),
            search: TextField::new(""),
            cursor: 0,
            scroll: 0,
            view_lines: 7,
            input_active: false,
            input: TextField::new(""),
            error: None,
        }
    }

    pub(crate) const fn side(&self) -> AppFilterSide {
        self.side
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn checked(&self) -> &[String] {
        &self.checked
    }

    pub(crate) fn foreground(&self) -> &[ForegroundApp] {
        &self.foreground
    }

    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Render/hit line budget refresh. Keyboard moves fall back to
    /// this when no fresher geometry has arrived.
    pub(crate) fn set_view_lines(&mut self, body_height: u16) {
        self.view_lines = body_height.saturating_sub(APP_FILTER_RESERVED_LINES).max(1);
    }

    pub(crate) fn search(&self) -> &TextField {
        &self.search
    }

    pub(crate) fn search_mut(&mut self) -> &mut TextField {
        &mut self.search
    }

    /// Foreground indices passing the search query (exe, title, or
    /// class, case-insensitive); empty query passes everything.
    pub(crate) fn matching_indices(&self) -> Vec<usize> {
        let query = self.search.text().trim().to_lowercase();
        if query.is_empty() {
            return (0..self.foreground.len()).collect();
        }
        self.foreground
            .iter()
            .enumerate()
            .filter(|(_, app)| {
                app.exe.to_lowercase().contains(&query)
                    || app.title.to_lowercase().contains(&query)
                    || app.class.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Flat pickable rows: stored filters, matching foreground apps,
    /// add row. Foreground rows carry full-list indices.
    pub(crate) fn rows(&self) -> Vec<FilterRow> {
        let matching = self.matching_indices();
        let mut rows = Vec::with_capacity(self.checked.len() + matching.len() + 1);
        for index in 0..self.checked.len() {
            rows.push(FilterRow::Checked(index));
        }
        for index in matching {
            rows.push(FilterRow::Foreground(index));
        }
        rows.push(FilterRow::Add);
        rows
    }

    /// `exe:` candidate for a foreground row.
    pub(crate) fn candidate(&self, index: usize) -> Option<String> {
        self.foreground
            .get(index)
            .map(|app| format!("exe:{}", app.exe))
    }

    /// True when the foreground row also sits on the opposite list:
    /// rendered dimmed and strictly unclickable.
    pub(crate) fn is_gray(&self, index: usize) -> bool {
        self.candidate(index)
            .is_some_and(|candidate| self.opposite_keys.contains(&gray_key(&candidate)))
    }

    /// Laid-out rows for a `max_lines` body: visible rows with
    /// window-relative y. Rendering and hit-testing share this.
    pub(crate) fn layout(&self, max_lines: u16) -> Vec<(FilterRow, u16)> {
        let mut laid = Vec::new();
        let mut rel = 0u16;
        for (index, row) in self.rows().iter().enumerate() {
            if index < self.scroll {
                continue;
            }
            let height = row.height();
            if rel.saturating_add(height) > max_lines {
                break;
            }
            laid.push((*row, rel));
            rel = rel.saturating_add(height);
        }
        laid
    }

    /// Row under a window-relative line, if any.
    pub(crate) fn row_at(&self, max_lines: u16, rel_y: u16) -> Option<FilterRow> {
        self.layout(max_lines)
            .into_iter()
            .find(|(row, y)| rel_y >= *y && rel_y < y.saturating_add(row.height()))
            .map(|(row, _)| row)
    }

    /// Window-relative line of the search box: exactly one blank line
    /// below the visible rows, pinned above the footer when the list
    /// fills the window. Rendering and hit-testing share this.
    pub(crate) fn search_rel(&self, max_lines: u16) -> u16 {
        let used = self
            .layout(max_lines)
            .into_iter()
            .map(|(row, y)| y.saturating_add(row.height()))
            .max()
            .unwrap_or(0);
        used.saturating_add(1).min(max_lines.saturating_add(1))
    }

    fn ensure_visible(&mut self, max_lines: u16) {
        let rows = self.rows();
        if rows.is_empty() {
            self.scroll = 0;
            return;
        }
        self.cursor = self.cursor.min(rows.len().saturating_sub(1));
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
            return;
        }
        // honey: line offsets, not row counts, so two-line
        // foreground rows never strand the cursor below the window.
        let mut lines = 0u16;
        for row in &rows[self.scroll..=self.cursor] {
            lines = lines.saturating_add(row.height());
        }
        while lines > max_lines && self.scroll < self.cursor {
            lines = lines.saturating_sub(rows[self.scroll].height());
            self.scroll = self.scroll.saturating_add(1);
        }
    }

    /// Arrow-key cursor; gray rows are skipped, never landed on.
    pub(crate) fn move_cursor(&mut self, delta: i32) {
        let max_lines = self.view_lines;
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let mut next = self.cursor as i32;
        for _ in 0..rows.len() {
            next = (next + delta).rem_euclid(rows.len() as i32);
            let gray =
                matches!(rows[next as usize], FilterRow::Foreground(index) if self.is_gray(index));
            if !gray {
                break;
            }
        }
        self.cursor = next as usize;
        self.ensure_visible(max_lines);
    }

    /// Enter toggles the focused row: checked removes, foreground
    /// adds. Gray rows are unreachable (skipped by the cursor).
    pub(crate) fn toggle_focused(&self) -> Option<PendingLibraryEdit> {
        match self.rows().get(self.cursor)? {
            FilterRow::Checked(index) => {
                let mut apps = self.checked.clone();
                apps.remove(*index);
                Some(self.pending(apps))
            }
            FilterRow::Foreground(index) => {
                if self.is_gray(*index) {
                    return None;
                }
                let mut apps = self.checked.clone();
                apps.push(self.candidate(*index)?);
                Some(self.pending(apps))
            }
            FilterRow::Add => None,
        }
    }

    /// Delete removes the focused checked row, if any.
    pub(crate) fn remove_focused(&self) -> Option<PendingLibraryEdit> {
        match self.rows().get(self.cursor)? {
            FilterRow::Checked(index) => {
                let mut apps = self.checked.clone();
                apps.remove(*index);
                Some(self.pending(apps))
            }
            _ => None,
        }
    }

    /// Focus a row directly; gray rows refuse.
    pub(crate) fn set_cursor(&mut self, row: FilterRow) {
        let max_lines = self.view_lines;
        if matches!(row, FilterRow::Foreground(index) if self.is_gray(index)) {
            return;
        }
        if let Some(position) = self.rows().iter().position(|candidate| *candidate == row) {
            self.cursor = position;
            self.ensure_visible(max_lines);
        }
    }

    pub(crate) const fn input_active(&self) -> bool {
        self.input_active
    }

    /// Open the inline input with a fresh buffer.
    pub(crate) fn start_input(&mut self) {
        self.input_active = true;
        self.input = TextField::new("");
        self.error = None;
    }

    /// Close the inline input, discarding its buffer.
    pub(crate) fn cancel_input(&mut self) {
        self.input_active = false;
        self.input = TextField::new("");
    }

    pub(crate) fn input(&self) -> &TextField {
        &self.input
    }

    pub(crate) fn input_mut(&mut self) -> &mut TextField {
        &mut self.input
    }

    /// Input keystrokes; the caret never needs clamping here.
    pub(crate) fn push_input(&mut self, ch: char) {
        self.input.insert(ch);
        self.error = None;
    }

    pub(crate) fn backspace_input(&mut self) {
        self.input.backspace();
    }

    pub(crate) fn delete_input_at(&mut self) {
        self.input.delete_at();
    }

    /// Place the input caret at a text column.
    pub(crate) fn place_input(&mut self, column: usize) {
        self.input.place(column);
    }

    /// Confirm the inline input: empty closes it, invalid text keeps
    /// it open with a footer error, valid text persists live.
    pub(crate) fn confirm_input(&mut self) -> InputConfirm {
        let value = self.input.text().trim().to_string();
        if value.is_empty() {
            self.cancel_input();
            return InputConfirm::Cancel;
        }
        if let Some(pos) = value.find(':')
            && AppFilterPrefix::parse_prefix(&value[..pos]).is_none()
        {
            self.error = Some("Unknown prefix; use exe:, class:, or title:.".to_string());
            return InputConfirm::Invalid("Unknown prefix; use exe:, class:, or title:.");
        }
        if self
            .checked
            .iter()
            .any(|item| gray_key(item) == gray_key(&value))
        {
            self.error = Some("That app is already on this list.".to_string());
            return InputConfirm::Invalid("That app is already on this list.");
        }
        if self.opposite_keys.contains(&gray_key(&value)) {
            let hint = self.side.opposite_hint();
            self.error = Some(format!("That app is already {hint}ed for this trigger."));
            return InputConfirm::Invalid("That app is on the other list.");
        }
        let mut apps = self.checked.clone();
        apps.push(value);
        InputConfirm::Save(self.pending(apps))
    }

    fn pending(&self, apps: Vec<String>) -> PendingLibraryEdit {
        PendingLibraryEdit {
            trigger_id: self.trigger_id.clone(),
            trigger: String::new(),
            field: self.side.field(apps),
            restore_index: self.restore_index,
        }
    }

    /// Re-filter after a search keystroke: restart at the top.
    pub(crate) fn refilter(&mut self) {
        self.scroll = 0;
        self.cursor = self.cursor.min(self.rows().len().saturating_sub(1));
        self.error = None;
    }

    /// Re-seed from refreshed rows after a live write. A saved input
    /// resets to a closed box; the cursor clamps into range.
    pub(crate) fn reseed(&mut self, checked: Vec<String>, opposite: Vec<String>) {
        self.checked = checked;
        self.opposite_keys = opposite.iter().map(|item| gray_key(item)).collect();
        self.scroll = 0;
        self.cursor = self.cursor.min(self.rows().len().saturating_sub(1));
        self.cancel_input();
    }
}
