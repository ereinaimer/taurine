use crate::widgets::field::TextField;
use crate::widgets::library::actions::{EditedField, PendingLibraryEdit};

/// Tags builder views: a chip cloud of the trigger's own tags, or the
/// fuzzy add step behind the `+` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagsView {
    Chips,
    Add,
}

/// One laid-out chip: trigger-tag index plus its cell rect, relative
/// to the cloud origin. The delete icon owns the chip's last cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChipCell {
    pub(crate) index: usize,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) width: u16,
}

/// Visible chip rows in the overlay, mirroring the list cap used by
/// the other overlay menus.
pub(crate) const TAGS_CHIP_ROWS: usize = 8;

/// Tags builder modal. The chips view shows the trigger's own tags as
/// a wrapping cloud with per-chip delete icons and a `+ Add tag` row;
/// the add view fuzzy-matches every other known tag. Every removal
/// and addition writes immediately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryTagsModalState {
    trigger_id: String,
    restore_index: usize,
    all_tags: Vec<String>,
    checked: Vec<String>,
    view: TagsView,
    focus: usize,
    chip_scroll: usize,
    cloud_width: u16,
    filter: TextField,
    cursor: usize,
    error: Option<String>,
}

impl LibraryTagsModalState {
    pub(crate) fn new(
        trigger_id: String,
        restore_index: usize,
        current: Vec<String>,
        all_tags: Vec<String>,
    ) -> Self {
        Self {
            trigger_id,
            restore_index,
            all_tags,
            checked: current,
            view: TagsView::Chips,
            focus: 0,
            chip_scroll: 0,
            cloud_width: 0,
            filter: TextField::new(""),
            cursor: 0,
            error: None,
        }
    }

    /// Render/hit width refresh. Keyboard focus moves fall back to
    /// this when no fresher geometry has arrived (e.g. no mouse yet).
    pub(crate) fn set_cloud_width(&mut self, width: u16) {
        self.cloud_width = width;
    }

    pub(crate) const fn cloud_width(&self) -> u16 {
        self.cloud_width
    }

    pub(crate) const fn view(&self) -> TagsView {
        self.view
    }

    pub(crate) fn checked(&self) -> &[String] {
        &self.checked
    }

    pub(crate) const fn focus(&self) -> usize {
        self.focus
    }

    /// Focus a chip or the `+` row directly, clamping into range.
    pub(crate) fn set_focus(&mut self, focus: usize) {
        let total = self.checked.len().saturating_add(1);
        if total > 0 {
            self.focus = focus.min(total.saturating_sub(1));
        }
    }

    /// True when focus sits on the `+ Add tag` row past the chips.
    pub(crate) fn focus_is_add_row(&self) -> bool {
        self.focus == self.checked.len()
    }

    pub(crate) fn enter_add(&mut self) {
        self.view = TagsView::Add;
        self.cursor = 0;
        self.error = None;
    }

    pub(crate) fn back_to_chips(&mut self) {
        self.view = TagsView::Chips;
        self.error = None;
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub(crate) fn filter(&self) -> &TextField {
        &self.filter
    }

    pub(crate) fn filter_mut(&mut self) -> &mut TextField {
        &mut self.filter
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn is_checked(&self, tag: &str) -> bool {
        self.checked.iter().any(|checked| checked == tag)
    }

    /// Flow layout shared by rendering and hit-testing: chips read
    /// `#tag` with a two-cell gap, wrapping at `width`. The focused
    /// chip swaps its `#` for the close icon at the same cell.
    pub(crate) fn chip_cells(&self, width: u16) -> Vec<ChipCell> {
        let mut cells = Vec::new();
        let (mut x, mut y) = (0u16, 0u16);
        for (index, tag) in self.checked.iter().enumerate() {
            let chip_width = (tag.chars().count() as u16).saturating_add(1);
            if x > 0 && x.saturating_add(chip_width) > width {
                x = 0;
                y = y.saturating_add(1);
            }
            cells.push(ChipCell {
                index,
                x,
                y,
                width: chip_width,
            });
            x = x.saturating_add(chip_width).saturating_add(2);
        }
        cells
    }

    /// Row of the `+ Add tag` action, just below the cloud.
    pub(crate) fn add_row_y(&self, width: u16) -> u16 {
        self.chip_cells(width)
            .iter()
            .map(|cell| cell.y)
            .max()
            .map(|last| last.saturating_add(1))
            .unwrap_or(0)
    }

    /// Painted rows for a body of `max_rows` chip rows: visible chip
    /// cells with window-relative y plus the `+` row when it fits.
    /// Rendering and hit-testing share this so clicks land as drawn.
    pub(crate) fn paint_layout(
        &self,
        width: u16,
        max_rows: u16,
    ) -> (Vec<(ChipCell, u16)>, Option<u16>) {
        let mut painted = Vec::new();
        for cell in self.chip_cells(width) {
            if cell.y < self.chip_scroll as u16 {
                continue;
            }
            let rel = cell.y.saturating_sub(self.chip_scroll as u16);
            if rel >= max_rows {
                break;
            }
            painted.push((cell, rel));
        }
        let add_rel = self.add_row_y(width).checked_sub(self.chip_scroll as u16);
        let add = match add_rel {
            Some(rel) if rel < max_rows => Some(rel),
            _ => None,
        };
        (painted, add)
    }

    /// Focus positions: one per chip plus the `+` row last.
    fn focus_total(&self) -> usize {
        self.checked.len().saturating_add(1)
    }

    fn focus_xy(&self, width: u16) -> (u16, u16) {
        let cells = self.chip_cells(width);
        if self.focus < cells.len() {
            let cell = cells[self.focus];
            (cell.x.saturating_add(cell.width / 2), cell.y)
        } else {
            (0, self.add_row_y(width))
        }
    }

    /// Arrow-key focus across chips and the `+` row. Left/right walk
    /// the order and wrap; up/down move by row to the nearest chip,
    /// treating the `+` row as the row below the cloud.
    pub(crate) fn move_focus(&mut self, width: u16, dx: i32, dy: i32) {
        let total = self.focus_total();
        if total == 0 {
            self.focus = 0;
            return;
        }
        if dx != 0 && dy == 0 {
            let next = (self.focus as i32 + dx).rem_euclid(total as i32);
            self.focus = next as usize;
            self.scroll_to_focus(width);
            return;
        }
        if dy == 0 {
            return;
        }
        let cells = self.chip_cells(width);
        let add_y = self.add_row_y(width);
        let (focus_x, focus_y) = self.focus_xy(width);
        if self.focus == self.checked.len() {
            // honey: from the `+` row, up lands on the last chip and
            // down wraps to the first.
            if dy < 0 {
                self.focus = self.checked.len().saturating_sub(1);
            } else {
                self.focus = 0;
            }
            self.scroll_to_focus(width);
            return;
        }
        let target_y = focus_y as i32 + dy;
        if target_y < 0 {
            // honey: above the first row wraps around to the `+` row.
            self.focus = self.checked.len();
        } else if target_y as u16 >= add_y {
            self.focus = self.checked.len();
        } else {
            let mut best: Option<(usize, u16)> = None;
            for cell in &cells {
                if cell.y == target_y as u16 {
                    let center = cell.x.saturating_add(cell.width / 2);
                    let gap = center.abs_diff(focus_x);
                    if best.is_none_or(|(_, best_gap)| gap < best_gap) {
                        best = Some((cell.index, gap));
                    }
                }
            }
            if let Some((index, _)) = best {
                self.focus = index;
            }
        }
        self.scroll_to_focus(width);
    }

    fn scroll_to_focus(&mut self, width: u16) {
        let cells = self.chip_cells(width);
        let focus_y = if self.focus < cells.len() {
            cells[self.focus].y
        } else {
            self.add_row_y(width)
        };
        let visible = TAGS_CHIP_ROWS as u16;
        if focus_y < self.chip_scroll as u16 {
            self.chip_scroll = focus_y as usize;
        } else if focus_y >= self.chip_scroll as u16 + visible {
            self.chip_scroll = focus_y.saturating_add(1).saturating_sub(visible) as usize;
        }
    }

    /// Remove the focused chip, if any. The `+` row has nothing to remove.
    pub(crate) fn remove_focused(&self) -> Option<PendingLibraryEdit> {
        self.remove_at(self.focus)
    }

    /// Remove one chip by trigger-tag index, returning the full new
    /// tag list. Absent rows resolve to no persist.
    pub(crate) fn remove_at(&self, index: usize) -> Option<PendingLibraryEdit> {
        let tag = self.checked.get(index)?.clone();
        let tags: Vec<String> = self
            .checked
            .iter()
            .filter(|checked| *checked != &tag)
            .cloned()
            .collect();
        Some(self.pending(tags))
    }

    /// Substring matches outside the trigger's own tags, in tag order.
    pub(crate) fn visible(&self) -> Vec<&str> {
        let needle = self.filter.text().to_ascii_lowercase();
        self.all_tags
            .iter()
            .map(String::as_str)
            .filter(|tag| !self.is_checked(tag) && tag.to_ascii_lowercase().contains(&needle))
            .collect()
    }

    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn move_cursor(&mut self, delta: i32) {
        let total = self.visible().len();
        if total == 0 {
            self.cursor = 0;
            return;
        }
        let next = (self.cursor as i32 + delta).rem_euclid(total as i32);
        self.cursor = next as usize;
    }

    fn clamp_cursor(&mut self) {
        let total = self.visible().len();
        if total == 0 {
            self.cursor = 0;
        } else {
            self.cursor = self.cursor.min(total.saturating_sub(1));
        }
    }

    /// Filter keystrokes; cursor resets so it never points past the
    /// narrowed list.
    pub(crate) fn push_filter(&mut self, ch: char) {
        self.filter.insert(ch);
        self.error = None;
        self.clamp_cursor();
    }

    pub(crate) fn backspace_filter(&mut self) {
        self.filter.backspace();
        self.clamp_cursor();
    }

    pub(crate) fn delete_filter_at(&mut self) {
        self.filter.delete_at();
        self.clamp_cursor();
    }

    /// Add the cursor match to the trigger.
    pub(crate) fn add_selected(&self) -> Option<PendingLibraryEdit> {
        let tag = self.visible().get(self.cursor)?.to_string();
        if self.is_checked(&tag) {
            return None;
        }
        let mut tags = self.checked.clone();
        tags.push(tag);
        Some(self.pending(tags))
    }

    /// Create the filter text as a new checked tag. Blank, over-long,
    /// and duplicate text never reaches the database.
    pub(crate) fn create_from_filter(&self) -> Option<PendingLibraryEdit> {
        let tag = self.filter.text().trim().to_lowercase();
        if tag.is_empty() || tag.chars().count() > 50 {
            return None;
        }
        if self.checked.iter().any(|checked| checked == &tag) {
            return None;
        }
        let mut tags = self.checked.clone();
        tags.push(tag);
        Some(self.pending(tags))
    }

    fn pending(&self, tags: Vec<String>) -> PendingLibraryEdit {
        PendingLibraryEdit {
            trigger_id: self.trigger_id.clone(),
            trigger: String::new(),
            field: EditedField::Tags(tags),
            restore_index: self.restore_index,
        }
    }

    /// Re-seed the checklist from freshly saved rows so core-side
    /// normalization (case, dedupe) never desyncs the menu. Focus
    /// stays valid; filter text survives across live toggles.
    pub(crate) fn reseed(&mut self, tags: Vec<String>) {
        self.checked = tags;
        self.focus = self.focus.min(self.checked.len());
        self.clamp_cursor();
    }
}
