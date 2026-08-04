use super::editor::LibraryEditorModalState;
use super::trigger::LibraryTrigger;

pub(crate) const LIBRARY_DELETE_MODAL_FOOTER: &str = "Esc Cancel";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryDeleteModalState {
    trigger_id: String,
    name: String,
    selected_yes: bool,
    restore_index: usize,
    pub(crate) return_to_editor: Option<LibraryEditorModalState>,
    error: Option<String>,
}

impl LibraryDeleteModalState {
    pub(crate) fn from_item(item: &LibraryTrigger, restore_index: usize) -> Self {
        Self {
            trigger_id: item.id().to_string(),
            name: item.name().to_string(),
            selected_yes: true,
            restore_index,
            return_to_editor: None,
            error: None,
        }
    }

    pub(crate) fn trigger_id(&self) -> &str {
        &self.trigger_id
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) const fn selected_yes(&self) -> bool {
        self.selected_yes
    }

    pub(crate) fn set_selected_yes(&mut self, selected: bool) {
        self.selected_yes = selected;
    }

    pub(crate) const fn restore_index(&self) -> usize {
        self.restore_index
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }
}
