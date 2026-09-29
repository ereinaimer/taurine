use taurine_core::db::crud::{
    ActionType, InvocationType, TriggerAliasRow, TriggerListItem, TriggerRow, TriggerType,
};
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

    /// Display-alias type mapped back onto [`TriggerType`] for the grouped
    /// entry (Task 8 owns the full multi-alias redesign; voice displays as word).
    pub(crate) fn from_invocations(invocations: &[TriggerAliasRow], action_type: &str) -> Self {
        let trigger_type = invocations
            .iter()
            .find(|a| a.invocation_type == InvocationType::Word)
            .or_else(|| invocations.first())
            .map(|a| trigger_type_of(a.invocation_type))
            .unwrap_or(TriggerType::Word);
        Self::from_parts(trigger_type, action_type)
    }

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

const fn trigger_type_of(invocation_type: InvocationType) -> TriggerType {
    match invocation_type {
        InvocationType::Hotkey => TriggerType::Hotkey,
        InvocationType::Regex => TriggerType::Regex,
        InvocationType::Word | InvocationType::Voice => TriggerType::Word,
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
}

impl LibraryTrigger {
    /// One list row per invocation alias; entries without invocations fall
    /// back to a single row showing the display string.
    pub(crate) fn expand(item: TriggerListItem) -> Vec<Self> {
        let target_os = display_target_os(&item.target_os).to_string();
        if item.invocations.is_empty() {
            let kind = LibraryKind::from_invocations(&item.invocations, &item.action_type);
            return vec![Self::row(&item, &target_os, item.display.clone(), kind)];
        }
        item.invocations
            .iter()
            .map(|alias| {
                let kind = LibraryKind::from_parts(
                    trigger_type_of(alias.invocation_type),
                    item.action_type.as_str(),
                );
                Self::row(&item, &target_os, alias.invocation.clone(), kind)
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn single(item: TriggerListItem) -> Self {
        Self::expand(item)
            .pop()
            .expect("trigger has at least one row")
    }

    fn row(item: &TriggerListItem, target_os: &str, trigger: String, kind: LibraryKind) -> Self {
        let search_text = build_search_text(item, kind.label(), target_os, &trigger);
        Self {
            id: item.id.clone(),
            name: item.name.clone(),
            trigger,
            preview: preview_from_item(item),
            kind,
            target_os: target_os.to_string(),
            search_text,
        }
    }

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

    pub(crate) fn matches_query(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }

        let needle = query.to_ascii_lowercase();
        self.search_text.contains(&needle)
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
        let kind = LibraryKind::from_invocations(&row.invocations, row.action_type.as_str());
        let content = modal_content_from_row(&row, kind)?;
        let metadata_rows = build_metadata_rows(&row);

        Ok(Self {
            id: row.id,
            name: row.name,
            description: row.description,
            tags_json: row.tags,
            usage_count: row.usage_count,
            last_used_at: row.last_used_at,
            trigger: row.display,
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
