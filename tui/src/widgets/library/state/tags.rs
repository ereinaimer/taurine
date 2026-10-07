use crate::theme::Theme;
use crate::widgets::field::TextField;
use crate::widgets::library::actions::{EditedField, PendingLibraryEdit};
use crate::widgets::library::props::tag_color;

/// One laid-out chip: trigger-tag index plus its cell rect, relative
/// to the cloud origin. The focused chip swaps its `#` for the close
/// icon in the chip's first cell.
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

/// Empty-menu line shown when the trigger has no tags yet. The `+`
/// button renders on the same row, right after this text.
pub(crate) const TAGS_EMPTY_LINE: &str = "No tags available";

/// Chip text style: theme color by tag position, no background —
/// the same color the Properties pane shows for that tag.
pub(crate) fn tag_style(theme: &Theme, index: usize) -> ratatui::style::Style {
    ratatui::style::Style::default().fg(tag_color(theme, index))
}

/// Outcome of confirming the inline input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InputConfirm {
    /// Persist these tags; the menu clears and closes its input.
    Save(PendingLibraryEdit),
    /// Empty input: close the input, keep the menu.
    Cancel,
    /// Keep the input open with this footer error.
    Invalid(&'static str),
}

/// Tags builder modal: the trigger's own tags as a wrapping chip
/// cloud with a trailing `+` action. The `+` opens an inline input
/// (`#` plus caret); Enter saves it, creating the tag when new. The
/// focused chip swaps its `#` for the close icon; the icon cell or
/// the Delete key removes it. Every write hits the database live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryTagsModalState {
    trigger_id: String,
    restore_index: usize,
    checked: Vec<String>,
    input_active: bool,
    focus: usize,
    hover: Option<usize>,
    chip_scroll: usize,
    cloud_width: u16,
    input: TextField,
    error: Option<String>,
    return_to_create: Option<Box<super::create::LibraryCreateModalState>>,
}

impl LibraryTagsModalState {
    pub(crate) fn new(trigger_id: String, restore_index: usize, current: Vec<String>) -> Self {
        Self {
            trigger_id,
            restore_index,
            checked: current,
            input_active: false,
            focus: 0,
            hover: None,
            chip_scroll: 0,
            cloud_width: 0,
            input: TextField::new(""),
            error: None,
            return_to_create: None,
        }
    }

    /// Attach a create draft for the return trip: toggles collect
    /// into the draft instead of persisting to the database.
    pub(crate) fn with_create_draft(
        mut self,
        draft: super::create::LibraryCreateModalState,
    ) -> Self {
        self.return_to_create = Some(Box::new(draft));
        self
    }

    pub(crate) fn return_to_create(&self) -> Option<&super::create::LibraryCreateModalState> {
        self.return_to_create.as_deref()
    }

    pub(crate) fn draft_mut(&mut self) -> Option<&mut super::create::LibraryCreateModalState> {
        self.return_to_create.as_deref_mut()
    }

    pub(crate) fn take_return_to_create(
        &mut self,
    ) -> Option<Box<super::create::LibraryCreateModalState>> {
        self.return_to_create.take()
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

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub(crate) fn input(&self) -> &TextField {
        &self.input
    }

    pub(crate) fn input_mut(&mut self) -> &mut TextField {
        &mut self.input
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn checked(&self) -> &[String] {
        &self.checked
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.checked.is_empty()
    }

    // honey: keyboard focus has no visual (Delete acts on it);
    // tests alone read it back.
    #[allow(dead_code)]
    pub(crate) const fn focus(&self) -> usize {
        self.focus
    }

    /// Hovered chip (trigger-tag index), from the last mouse move.
    /// Only the hovered chip swaps its `#` for the close icon.
    pub(crate) const fn hover(&self) -> Option<usize> {
        self.hover
    }

    pub(crate) fn set_hover(&mut self, hover: Option<usize>) {
        self.hover = match hover {
            Some(index) if index < self.checked.len() => Some(index),
            _ => None,
        };
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

    /// Render/hit width refresh. Keyboard focus moves fall back to
    /// this when no fresher geometry has arrived (e.g. no mouse yet).
    pub(crate) fn set_cloud_width(&mut self, width: u16) {
        self.cloud_width = width;
    }

    pub(crate) const fn cloud_width(&self) -> u16 {
        self.cloud_width
    }

    /// Flow layout shared by rendering and hit-testing: chips read
    /// `#tag` with a two-cell gap, wrapping at `width`.
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

    /// Row of the `+` action (or the live input replacing it): the
    /// trailing row after the last chip, same row when it fits.
    pub(crate) fn plus_row_y(&self, width: u16) -> u16 {
        self.plus_cell(width).1
    }

    /// Cloud-relative x of the `+` button: after the empty line
    /// when there are no chips yet, else the trailing plus cell.
    /// Empty renders `"{line}  + "`, so the glyph sits two cells
    /// past the text.
    pub(crate) fn plus_hit_x(&self, width: u16) -> u16 {
        if self.checked.is_empty() && !self.input_active {
            TAGS_EMPTY_LINE.chars().count() as u16 + 2
        } else {
            self.plus_cell(width).0
        }
    }

    /// True when a cloud-relative x hits the `+` button. Empty
    /// renders a padded three-cell `" + "` span, so the padding
    /// each side counts; the chip cloud keeps its exact cell.
    pub(crate) fn plus_hit_accepts(&self, width: u16, x: u16) -> bool {
        if self.checked.is_empty() && !self.input_active {
            let center = self.plus_hit_x(width);
            x >= center.saturating_sub(1) && x <= center.saturating_add(1)
        } else {
            x == self.plus_cell(width).0
        }
    }

    /// Cell of the `+` action (or the live input replacing it):
    /// after the last chip when it fits, else the next row.
    pub(crate) fn plus_cell(&self, width: u16) -> (u16, u16) {
        let cells = self.chip_cells(width);
        let (mut x, mut y) = (0u16, 0u16);
        if let Some(last) = cells.last() {
            x = last.x.saturating_add(last.width).saturating_add(2);
            y = last.y;
            if x.saturating_add(1) > width {
                x = 0;
                y = y.saturating_add(1);
            }
        }
        (x, y)
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
        let add_rel = self.plus_row_y(width).checked_sub(self.chip_scroll as u16);
        let add = match add_rel {
            Some(rel) if rel < max_rows => Some(rel),
            _ => None,
        };
        (painted, add)
    }

    /// Chip under a cloud-relative cell, if any. Rendering,
    /// clicks, and hover share this so the `+` and the close icon
    /// land exactly as drawn.
    pub(crate) fn chip_at(
        &self,
        width: u16,
        max_rows: u16,
        rel_y: u16,
        x: u16,
    ) -> Option<ChipCell> {
        if rel_y >= max_rows {
            return None;
        }
        let (painted, _) = self.paint_layout(width, max_rows);
        painted.into_iter().find_map(|(cell, rel)| {
            (rel == rel_y && x >= cell.x && x < cell.x.saturating_add(cell.width)).then_some(cell)
        })
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
            self.plus_cell(width)
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
        let plus_y = self.plus_row_y(width);
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
        } else if target_y as u16 >= plus_y {
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
            self.plus_row_y(width)
        };
        let visible = TAGS_CHIP_ROWS as u16;
        if focus_y < self.chip_scroll as u16 {
            self.chip_scroll = focus_y as usize;
        } else if focus_y >= self.chip_scroll as u16 + visible {
            self.chip_scroll = focus_y.saturating_add(1).saturating_sub(visible) as usize;
        }
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
    /// it open with a footer error, valid text persists.
    pub(crate) fn confirm_input(&mut self) -> InputConfirm {
        let tag = self.input.text().trim().to_lowercase();
        if tag.is_empty() {
            self.cancel_input();
            return InputConfirm::Cancel;
        }
        if tag.chars().count() > 50 {
            self.error = Some("Tags max out at 50 characters.".to_string());
            return InputConfirm::Invalid("Tags max out at 50 characters.");
        }
        if self.checked.iter().any(|checked| checked == &tag) {
            self.error = Some("That tag is already on this trigger.".to_string());
            return InputConfirm::Invalid("That tag is already on this trigger.");
        }
        let mut tags = self.checked.clone();
        tags.push(tag);
        InputConfirm::Save(self.pending(tags))
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

    /// Remove the focused chip, if any. The `+` row has nothing to remove.
    pub(crate) fn remove_focused(&self) -> Option<PendingLibraryEdit> {
        self.remove_at(self.focus)
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
    /// normalization (case, dedupe) never desyncs the menu. A saved
    /// input resets to a closed box; hover never survives.
    pub(crate) fn reseed(&mut self, tags: Vec<String>) {
        self.checked = tags;
        self.focus = self.focus.min(self.checked.len());
        self.hover = None;
        self.cancel_input();
    }
}
