use taurine_core::db::crud::{ActionType, TriggerListItem, TriggerRow, TriggerType};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

use crate::widgets::library::actions::{
    build_metadata_rows, build_search_text, display_target_os, modal_content_from_row,
    preview_from_item,
};

use super::LibraryMetadataRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryKind {
    Snippet,
    Script,
    HotkeySnippet,
    HotkeyScript,
}

impl LibraryKind {
    pub(crate) const ALL: [Self; 4] = [
        Self::Snippet,
        Self::Script,
        Self::HotkeySnippet,
        Self::HotkeyScript,
    ];

    pub(crate) fn from_parts(trigger_type: TriggerType, action_type: &str) -> Self {
        let is_script = ActionType::parse_str(action_type) == Some(ActionType::Script);

        match (trigger_type, is_script) {
            (TriggerType::Hotkey, true) => Self::HotkeyScript,
            (TriggerType::Hotkey, false) => Self::HotkeySnippet,
            (TriggerType::Word, true) => Self::Script,
            (TriggerType::Word, false) => Self::Snippet,
            (TriggerType::Regex, true) => Self::Script,
            (TriggerType::Regex, false) => Self::Snippet,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Snippet => "snippet",
            Self::Script => "script",
            Self::HotkeySnippet => "hotkey snippet",
            Self::HotkeyScript => "hotkey script",
        }
    }

    pub(crate) const fn content_label(self) -> &'static str {
        match self {
            Self::Snippet | Self::HotkeySnippet => "Output",
            Self::Script | Self::HotkeyScript => "Script",
        }
    }

    pub(crate) const fn is_script(self) -> bool {
        matches!(self, Self::Script | Self::HotkeyScript)
    }

    pub(crate) fn trigger_type(self) -> TriggerType {
        match self {
            Self::Snippet | Self::Script => TriggerType::Word,
            Self::HotkeySnippet | Self::HotkeyScript => TriggerType::Hotkey,
        }
    }

    pub(crate) fn action_type(self) -> &'static str {
        if self.is_script() { "script" } else { "text" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryTrigger {
    id: String,
    name: String,
    trigger: String,
    preview: String,
    kind: LibraryKind,
    pub(crate) target_os: String,
    search_text: String,
    uses: u64,
}

impl LibraryTrigger {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn trigger(&self) -> &str {
        &self.trigger
    }

    pub(crate) fn preview(&self) -> &str {
        &self.preview
    }

    pub(crate) const fn kind_label(&self) -> &'static str {
        self.kind.label()
    }

    pub(crate) fn metadata_label(&self) -> String {
        format!("{} // {} uses", self.target_os, self.uses)
    }

    pub(crate) fn matches_query(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }

        let needle = query.to_ascii_lowercase();
        self.search_text.contains(&needle)
    }
}

impl From<TriggerListItem> for LibraryTrigger {
    fn from(item: TriggerListItem) -> Self {
        let kind = LibraryKind::from_parts(item.trigger_type, item.action_type.as_str());
        let preview = preview_from_item(&item);
        let target_os = display_target_os(&item.target_os).to_string();
        let search_text = build_search_text(&item, kind.label(), &target_os);

        Self {
            id: item.id,
            name: item.name,
            trigger: item.trigger,
            preview,
            kind,
            target_os,
            search_text,
            uses: item.usage_count.max(0) as u64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibrarySelectState {
    pub(crate) title: &'static str,
    pub(crate) options: Vec<String>,
    pub(crate) selected: usize,
}

impl LibrarySelectState {
    pub(crate) const fn title(&self) -> &'static str {
        self.title
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryTriggerDetail {
    id: String,
    name: String,
    description: Option<String>,
    tags_json: String,
    usage_count: i64,
    last_used_at: Option<i64>,
    trigger: String,
    kind: LibraryKind,
    content: String,
    target_os_raw: String,
    metadata_rows: Vec<LibraryMetadataRow>,
    interpreter: Option<ScriptInterpreter>,
    behavior: Option<ScriptBehavior>,
}

impl LibraryTriggerDetail {
    pub(crate) fn from_row(row: TriggerRow) -> taurine_core::Result<Self> {
        let kind = LibraryKind::from_parts(row.trigger_type, row.action_type.as_str());
        let content = modal_content_from_row(&row, kind)?;
        let metadata_rows = build_metadata_rows(&row);

        Ok(Self {
            id: row.id,
            name: row.name,
            description: row.description,
            tags_json: row.tags,
            usage_count: row.usage_count,
            last_used_at: row.last_used_at,
            trigger: row.trigger,
            kind,
            content,
            target_os_raw: row.target_os,
            metadata_rows,
            interpreter: row.interpreter,
            behavior: row.behavior,
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub(crate) fn tags_json(&self) -> &str {
        &self.tags_json
    }

    pub(crate) const fn usage_count(&self) -> i64 {
        self.usage_count
    }

    pub(crate) const fn last_used_at(&self) -> Option<i64> {
        self.last_used_at
    }

    pub(crate) fn trigger(&self) -> &str {
        &self.trigger
    }

    pub(crate) const fn kind(&self) -> LibraryKind {
        self.kind
    }

    #[cfg(test)]
    pub(crate) const fn content_label(&self) -> &'static str {
        self.kind.content_label()
    }

    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    pub(crate) fn target_os_raw(&self) -> &str {
        &self.target_os_raw
    }

    pub(crate) fn metadata_rows(&self) -> &[LibraryMetadataRow] {
        &self.metadata_rows
    }

    pub(crate) const fn interpreter(&self) -> Option<ScriptInterpreter> {
        self.interpreter
    }

    pub(crate) const fn behavior(&self) -> Option<ScriptBehavior> {
        self.behavior
    }
}
