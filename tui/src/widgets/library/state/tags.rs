use crate::widgets::field::TextField;
use crate::widgets::library::actions::{EditedField, PendingLibraryEdit};

/// Tags builder modal: fuzzy filter over every known tag, checklist
/// with live persistence. Typing narrows by substring, Enter toggles
/// the cursor row, Enter on an empty match creates the filter text as
/// a new tag, Esc closes. Every toggle writes immediately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryTagsModalState {
    trigger_id: String,
    restore_index: usize,
    all_tags: Vec<String>,
    checked: Vec<String>,
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
            filter: TextField::new(""),
            cursor: 0,
            error: None,
        }
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

    /// Substring matches in tag order, like list search.
    pub(crate) fn visible(&self) -> Vec<&str> {
        let needle = self.filter.text().to_ascii_lowercase();
        self.all_tags
            .iter()
            .map(String::as_str)
            .filter(|tag| tag.to_ascii_lowercase().contains(&needle))
            .collect()
    }

    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn is_checked(&self, tag: &str) -> bool {
        self.checked.iter().any(|checked| checked == tag)
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

    /// Toggle the cursor row, returning the full new tag list with the
    /// trigger's order preserved and additions appended.
    pub(crate) fn toggle_selected(&self) -> Option<PendingLibraryEdit> {
        let tag = self.visible().get(self.cursor)?.to_string();
        let mut tags: Vec<String> = self
            .checked
            .iter()
            .filter(|checked| *checked != &tag)
            .cloned()
            .collect();
        if !self.is_checked(&tag) {
            tags.push(tag);
        }
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
    /// normalization (case, dedupe) never desyncs the menu.
    pub(crate) fn reseed(&mut self, tags: Vec<String>) {
        self.checked = tags;
        self.clamp_cursor();
    }
}
