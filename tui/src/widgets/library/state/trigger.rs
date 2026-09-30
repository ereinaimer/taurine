use taurine_core::db::crud::{
    ActionType, InvocationType, TriggerAliasRow, TriggerListItem, TriggerType,
};

use crate::widgets::library::actions::{build_search_text, display_target_os, preview_from_item};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryKind {
    Snippet,
    Script,
    HotkeySnippet,
    HotkeyScript,
}

impl LibraryKind {
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
    invocation: InvocationType,
    search_text: String,
}

impl LibraryTrigger {
    /// One list row per invocation alias; entries without invocations fall
    /// back to a single row showing the display string.
    pub(crate) fn expand(item: TriggerListItem) -> Vec<Self> {
        let target_os = display_target_os(&item.target_os).to_string();
        if item.invocations.is_empty() {
            let kind = LibraryKind::from_invocations(&item.invocations, &item.action_type);
            return vec![Self::row(
                &item,
                &target_os,
                item.display.clone(),
                kind,
                InvocationType::Word,
            )];
        }
        item.invocations
            .iter()
            .map(|alias| {
                let kind = LibraryKind::from_parts(
                    trigger_type_of(alias.invocation_type),
                    item.action_type.as_str(),
                );
                Self::row(
                    &item,
                    &target_os,
                    alias.invocation.clone(),
                    kind,
                    alias.invocation_type,
                )
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn single(item: TriggerListItem) -> Self {
        Self::expand(item)
            .pop()
            .expect("trigger has at least one row")
    }

    fn row(
        item: &TriggerListItem,
        target_os: &str,
        trigger: String,
        kind: LibraryKind,
        invocation: InvocationType,
    ) -> Self {
        let search_text = build_search_text(item, kind.label(), target_os, &trigger);
        Self {
            id: item.id.clone(),
            name: item.name.clone(),
            trigger,
            preview: preview_from_item(item),
            kind,
            target_os: target_os.to_string(),
            invocation,
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

    pub(crate) const fn is_hotkey(&self) -> bool {
        matches!(self.invocation, InvocationType::Hotkey)
    }

    pub(crate) const fn is_voice(&self) -> bool {
        matches!(self.invocation, InvocationType::Voice)
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
