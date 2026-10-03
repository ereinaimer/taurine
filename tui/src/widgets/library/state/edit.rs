use taurine_core::db::crud::TriggerLimits;

use crate::widgets::library::actions::{LibraryInteraction, PendingLibraryRename};

/// In-progress trigger-name edit. Caret always sits at the end of the
/// draft, like the search box; selection changes commit automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TriggerNameEdit {
    trigger_id: String,
    draft: String,
}

impl TriggerNameEdit {
    pub(crate) fn begin(trigger_id: &str, name: &str) -> Self {
        Self {
            trigger_id: trigger_id.to_string(),
            draft: name.to_string(),
        }
    }

    pub(crate) fn draft(&self) -> &str {
        &self.draft
    }

    pub(crate) fn push(&mut self, ch: char) {
        if self.draft.chars().count() < TriggerLimits::MAX_NAME_LENGTH {
            self.draft.push(ch);
        }
    }

    pub(crate) fn pop(&mut self) {
        self.draft.pop();
    }
}

impl super::LibraryPageState {
    pub(crate) fn name_edit(&self) -> Option<&TriggerNameEdit> {
        self.edit.as_ref()
    }

    /// Click on the trigger name starts an edit session; typing captures
    /// all keys until Enter commits or Esc cancels. The draft starts
    /// with the displayed text so nothing is ever erased on click.
    pub(crate) fn start_name_edit(&mut self) {
        if self.edit.is_some() {
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
        self.edit = Some(TriggerNameEdit::begin(&id, &name));
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
        let name = edit.draft.trim().to_string();
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
            draft,
        });
        true
    }
}
