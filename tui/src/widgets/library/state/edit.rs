use taurine_core::db::crud::TriggerLimits;

use crate::widgets::field::TextField;
use crate::widgets::library::actions::{LibraryInteraction, PendingLibraryRename};

/// In-progress trigger-name edit. Clicking the name parks the caret at
/// the click; selection changes commit automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TriggerNameEdit {
    trigger_id: String,
    draft: TextField,
}

impl TriggerNameEdit {
    pub(crate) fn begin(trigger_id: &str, name: &str) -> Self {
        Self {
            trigger_id: trigger_id.to_string(),
            draft: TextField::new(name),
        }
    }

    pub(crate) fn field(&self) -> &TextField {
        &self.draft
    }
    pub(crate) fn push(&mut self, ch: char) {
        if self.draft.len_chars() < TriggerLimits::MAX_NAME_LENGTH {
            self.draft.insert(ch);
        }
    }

    pub(crate) fn pop(&mut self) {
        self.draft.backspace();
    }

    pub(crate) fn delete_at(&mut self) {
        self.draft.delete_at();
    }

    pub(crate) fn move_left(&mut self) {
        self.draft.move_left();
    }

    pub(crate) fn move_right(&mut self) {
        self.draft.move_right();
    }

    pub(crate) fn move_home(&mut self) {
        self.draft.move_home();
    }

    pub(crate) fn move_end(&mut self) {
        self.draft.move_end();
    }

    pub(crate) fn place(&mut self, index: usize) {
        self.draft.place(index);
    }
}

impl super::LibraryPageState {
    pub(crate) fn name_edit(&self) -> Option<&TriggerNameEdit> {
        self.edit.as_ref()
    }

    /// Click on the trigger name starts an edit session; typing captures
    /// all keys until Enter commits or Esc cancels. The draft starts
    /// with the displayed text so nothing is ever erased on click.
    // honey: exercised by tests; production opens via start_name_edit_at.
    #[allow(dead_code)]
    pub(crate) fn start_name_edit(&mut self) {
        self.start_name_edit_at(usize::MAX);
    }

    /// Click-to-place: caret lands on the clicked character. Clicking
    /// while already editing just moves the caret.
    pub(crate) fn start_name_edit_at(&mut self, cursor: usize) {
        if let Some(edit) = self.edit.as_mut() {
            edit.place(cursor);
            return;
        }
        let Some(selected) = self.selected_index() else {
            return;
        };
        let Some((id, name)) = self
            .item_at_filtered(selected)
            .map(|item| (item.id().to_string(), item.display_name().to_string()))
        else {
            return;
        };
        self.search_mode = false;
        let mut edit = TriggerNameEdit::begin(&id, &name);
        edit.place(cursor);
        self.edit = Some(edit);
    }

    pub(crate) fn cancel_name_edit(&mut self) {
        self.edit = None;
    }

    /// Validate + persist the draft. Unchanged or invalid text is a
    /// silent no-op: the existing value is left untouched and no
    /// warning is shown anywhere.
    pub(crate) fn commit_name_edit(&mut self) -> LibraryInteraction {
        let Some(edit) = self.edit.take() else {
            return LibraryInteraction::handled();
        };
        let name = edit.draft.text().trim().to_string();
        let Some(selected) = self.selected_index() else {
            return LibraryInteraction::handled();
        };
        let Some(item) = self.item_at_filtered(selected) else {
            return LibraryInteraction::handled();
        };
        if TriggerLimits::validate_name(&name).is_err() || name == item.name() {
            return LibraryInteraction::handled();
        }
        LibraryInteraction::rename(PendingLibraryRename {
            trigger_id: edit.trigger_id,
            trigger: item.trigger().to_string(),
            name,
            restore_index: selected,
        })
    }

    /// Reopen an edit after a failed save so typed text is never lost.
    pub(crate) fn restore_name_edit(&mut self, trigger_id: &str, draft: String) -> bool {
        if !self.select_by_id(trigger_id) {
            return false;
        }
        self.edit = Some(TriggerNameEdit {
            trigger_id: trigger_id.to_string(),
            draft: TextField::new(draft),
        });
        true
    }
}
