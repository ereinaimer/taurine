use taurine_core::db::crud::TriggerLimits;

use crate::widgets::field::TextField;
use crate::widgets::library::actions::LibraryInteraction;
use crate::widgets::textarea::TextArea;

/// Which trigger part an edit session targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditTarget {
    Name,
    Description,
    Content,
}

/// In-progress edit of one trigger part. Clicking a value parks the
/// caret at the click; selection changes commit automatically, Esc
/// discards, and invalid text silently no-ops without touching storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveEdit {
    trigger_id: String,
    target: EditTarget,
    line: TextField,
    body: TextArea,
}

impl ActiveEdit {
    fn begin(trigger_id: &str, target: EditTarget, initial: &str) -> Self {
        Self {
            trigger_id: trigger_id.to_string(),
            target,
            line: TextField::new(initial),
            body: TextArea::new(initial),
        }
    }

    pub(crate) const fn target(&self) -> EditTarget {
        self.target
    }

    pub(crate) fn trigger_id(&self) -> &str {
        &self.trigger_id
    }

    pub(crate) fn line(&self) -> &TextField {
        &self.line
    }

    // honey: line-view alias kept for the header render path.
    pub(crate) fn field(&self) -> &TextField {
        &self.line
    }

    pub(crate) fn line_mut(&mut self) -> &mut TextField {
        &mut self.line
    }

    pub(crate) fn body(&self) -> &TextArea {
        &self.body
    }

    fn line_cap(&self) -> usize {
        match self.target {
            EditTarget::Name => TriggerLimits::MAX_NAME_LENGTH,
            _ => TriggerLimits::MAX_DESCRIPTION_LENGTH,
        }
    }

    pub(crate) fn push_char(&mut self, ch: char) {
        if self.line.len_chars() < self.line_cap() {
            self.line.insert(ch);
        }
    }
}

impl super::LibraryPageState {
    pub(crate) fn edit(&self) -> Option<&ActiveEdit> {
        self.edit.as_ref()
    }

    /// Old name-edit entry point, kept for the header click path.
    pub(crate) fn name_edit(&self) -> Option<&ActiveEdit> {
        self.edit
            .as_ref()
            .filter(|edit| edit.target == EditTarget::Name)
    }

    fn begin_edit(&mut self, target: EditTarget, initial: String) -> LibraryInteraction {
        // honey: one session at a time; opening another commits first.
        let flush = self.commit_edit();
        let Some(selected) = self.selected_index() else {
            return flush;
        };
        let Some(id) = self
            .item_at_filtered(selected)
            .map(|item| item.id().to_string())
        else {
            return flush;
        };
        self.search_mode = false;
        self.edit = Some(ActiveEdit::begin(&id, target, &initial));
        flush
    }

    /// Click on the trigger name starts an edit session with the caret
    /// at the click; the draft starts with the displayed text.
    // honey: exercised by tests; production opens via start_name_edit_at.
    #[allow(dead_code)]
    pub(crate) fn start_name_edit(&mut self) {
        self.start_name_edit_at(usize::MAX);
    }

    /// Click-to-place: caret lands on the clicked character. A session
    /// on another part commits first; the caller persists the result.
    pub(crate) fn start_name_edit_at(&mut self, cursor: usize) -> LibraryInteraction {
        let same_target = self
            .edit
            .as_ref()
            .is_some_and(|edit| edit.target == EditTarget::Name);
        if same_target {
            if let Some(edit) = self.edit.as_mut() {
                edit.line.place(cursor);
            }
            return LibraryInteraction::handled();
        }
        let initial = self
            .selected_item_text(EditTarget::Name)
            .unwrap_or_default();
        let flush = self.begin_edit(EditTarget::Name, initial);
        if let Some(edit) = self.edit.as_mut() {
            edit.line.place(cursor);
        }
        flush
    }

    /// Click on the description starts an edit session.
    pub(crate) fn start_description_edit(&mut self) -> LibraryInteraction {
        if self
            .edit
            .as_ref()
            .is_some_and(|edit| edit.target == EditTarget::Description)
        {
            return LibraryInteraction::handled();
        }
        let initial = self
            .selected_item_text(EditTarget::Description)
            .unwrap_or_default();
        self.begin_edit(EditTarget::Description, initial)
    }

    /// Click in the content box starts a body edit with the caret placed.
    pub(crate) fn start_content_edit_at(&mut self, row: usize, col: usize) -> LibraryInteraction {
        if let Some(edit) = self.edit.as_mut()
            && edit.target == EditTarget::Content
        {
            edit.body.place(row, col);
            return LibraryInteraction::handled();
        }
        let initial = self
            .selected_item_text(EditTarget::Content)
            .unwrap_or_default();
        let flush = self.begin_edit(EditTarget::Content, initial);
        if let Some(edit) = self
            .edit
            .as_mut()
            .filter(|edit| edit.target == EditTarget::Content)
        {
            edit.body.place(row, col);
        }
        flush
    }

    fn selected_item_text(&self, target: EditTarget) -> Option<String> {
        let selected = self.selected_index()?;
        let item = self.item_at_filtered(selected)?;
        Some(match target {
            EditTarget::Name => item.display_name().to_string(),
            EditTarget::Description => item.description().unwrap_or("").to_string(),
            EditTarget::Content => item.content().to_string(),
        })
    }

    pub(crate) fn cancel_edit(&mut self) {
        self.edit = None;
        self.last_edit_at = None;
    }

    /// Validate + persist the draft without closing the session.
    /// Unchanged or invalid text is a silent no-op. Used by autosave;
    /// explicit exits go through `commit_edit`.
    pub(crate) fn build_edit_interaction(&self) -> LibraryInteraction {
        let Some(edit) = self.edit.as_ref() else {
            return LibraryInteraction::handled();
        };
        let Some(selected) = self.selected_index() else {
            return LibraryInteraction::handled();
        };
        let Some(item) = self.item_at_filtered(selected) else {
            return LibraryInteraction::handled();
        };
        if edit.trigger_id != item.id() {
            return LibraryInteraction::handled();
        };
        match edit.target {
            EditTarget::Name => {
                let name = edit.line.text().trim().to_string();
                if TriggerLimits::validate_name(&name).is_err() || name == item.name() {
                    return LibraryInteraction::handled();
                }
                LibraryInteraction::edit(crate::widgets::library::actions::PendingLibraryEdit {
                    trigger_id: edit.trigger_id.clone(),
                    trigger: item.trigger().to_string(),
                    field: crate::widgets::library::actions::EditedField::Name(name),
                    restore_index: selected,
                })
            }
            EditTarget::Description => {
                let raw = edit.line.text().trim();
                let description = if raw.is_empty() {
                    None
                } else {
                    Some(raw.to_string())
                };
                let current = item.description().map(str::trim).filter(|d| !d.is_empty());
                if description.as_deref() == current {
                    return LibraryInteraction::handled();
                }
                if TriggerLimits::validate_description(description.as_deref()).is_err() {
                    return LibraryInteraction::handled();
                }
                LibraryInteraction::edit(crate::widgets::library::actions::PendingLibraryEdit {
                    trigger_id: edit.trigger_id.clone(),
                    trigger: item.trigger().to_string(),
                    field: crate::widgets::library::actions::EditedField::Description(description),
                    restore_index: selected,
                })
            }
            EditTarget::Content => {
                let body = edit.body.text();
                if body == item.content() {
                    return LibraryInteraction::handled();
                }
                LibraryInteraction::edit(crate::widgets::library::actions::PendingLibraryEdit {
                    trigger_id: edit.trigger_id.clone(),
                    trigger: item.trigger().to_string(),
                    field: crate::widgets::library::actions::EditedField::Content(body),
                    restore_index: selected,
                })
            }
        }
    }

    /// Validate + persist the draft, closing the session. Same silent
    /// no-op policy as the autosave path.
    pub(crate) fn commit_edit(&mut self) -> LibraryInteraction {
        let interaction = self.build_edit_interaction();
        self.edit = None;
        self.last_edit_at = None;
        interaction
    }

    /// Reopen an edit after a failed save so typed text is never lost.
    pub(crate) fn restore_edit(
        &mut self,
        trigger_id: &str,
        target: EditTarget,
        draft: String,
    ) -> bool {
        if !self.select_by_id(trigger_id) {
            return false;
        }
        self.edit = Some(ActiveEdit::begin(trigger_id, target, &draft));
        true
    }

    /// True while the content body holds an active selection, so
    /// Ctrl+C copies instead of quitting the app.
    pub(crate) fn has_content_selection(&self) -> bool {
        self.edit
            .as_ref()
            .is_some_and(|edit| edit.target == EditTarget::Content && edit.body.has_selection())
    }

    /// Move the body caret, keeping it inside the scrolled window.
    /// Scroll math runs in wrapped visual rows at the tracked width;
    /// unknown width (zero) skips adjustment.
    pub(crate) fn move_content_caret(&mut self, apply: impl FnOnce(&mut TextArea)) {
        let Some(edit) = self.edit.as_mut() else {
            return;
        };
        if edit.target != EditTarget::Content {
            return;
        }
        apply(&mut edit.body);
        let width = self.content_width;
        if width == 0 {
            return;
        }
        let (row, col) = edit.body.cursor();
        let (visual, _) = crate::widgets::library::detail::content_visual_cursor(
            edit.body.lines(),
            row,
            col,
            width,
        );
        let height = crate::widgets::library::detail::CONTENT_INNER_HEIGHT;
        if visual < self.detail_scroll {
            self.detail_scroll = visual;
        } else if visual >= self.detail_scroll + height {
            self.detail_scroll = visual + 1 - height;
        }
    }

    pub(crate) fn content_newline(&mut self) {
        self.move_content_caret(|body| body.insert_newline());
    }

    pub(crate) fn insert_content_char(&mut self, ch: char) {
        self.move_content_caret(|body| body.insert_char(ch));
    }
}
