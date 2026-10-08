use taurine_core::db::crud::{ActionType, InvocationType, TriggerAliasRow, TriggerListItem};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

use crate::widgets::library::actions::{
    build_search_text, content_from_item, display_target_os, preview_from_item,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryKind {
    TextTrigger,
    HotkeyTrigger,
    RegexTrigger,
    VoiceTrigger,
    TextScript,
    HotkeyScript,
    RegexScript,
    VoiceScript,
}

impl LibraryKind {
    /// Display-alias invocation mapped onto a kind. Entries without
    /// invocations fall back to a plain word trigger.
    pub(crate) fn from_invocations(invocations: &[TriggerAliasRow], action_type: &str) -> Self {
        let invocation = invocations
            .iter()
            .find(|a| a.invocation_type == InvocationType::Word)
            .or_else(|| invocations.first())
            .map(|a| a.invocation_type)
            .unwrap_or(InvocationType::Word);
        Self::from_invocation(invocation, action_type)
    }

    pub(crate) fn from_invocation(invocation: InvocationType, action_type: &str) -> Self {
        let is_script = ActionType::parse_str(action_type) == Some(ActionType::Script);

        match (invocation, is_script) {
            (InvocationType::Word, false) => Self::TextTrigger,
            (InvocationType::Word, true) => Self::TextScript,
            (InvocationType::Hotkey, false) => Self::HotkeyTrigger,
            (InvocationType::Hotkey, true) => Self::HotkeyScript,
            (InvocationType::Regex, false) => Self::RegexTrigger,
            (InvocationType::Regex, true) => Self::RegexScript,
            (InvocationType::Voice, false) => Self::VoiceTrigger,
            (InvocationType::Voice, true) => Self::VoiceScript,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::TextTrigger => "text trigger",
            Self::HotkeyTrigger => "hotkey trigger",
            Self::RegexTrigger => "regex trigger",
            Self::VoiceTrigger => "voice trigger",
            Self::TextScript => "text script",
            Self::HotkeyScript => "hotkey script",
            Self::RegexScript => "regex script",
            Self::VoiceScript => "voice script",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryTrigger {
    id: String,
    trigger: String,
    preview: String,
    content: String,
    kind: LibraryKind,
    pub(crate) target_os: String,
    invocation: InvocationType,
    aliases: Vec<String>,
    description: Option<String>,
    tags: Vec<String>,
    only_apps: Option<String>,
    except_apps: Option<String>,
    auto_case: bool,
    is_enabled: bool,
    require_confirmation: bool,
    usage_count: i64,
    last_used_at: Option<i64>,
    created_at: i64,
    interpreter: Option<ScriptInterpreter>,
    behavior: Option<ScriptBehavior>,
    search_text: String,
}

/// Minimal `["a", "b"]` list parser for stored tags. Falls back to empty
/// on anything unexpected rather than failing the row.
pub(crate) fn parse_tags(tags_json: &str) -> Vec<String> {
    let inner = tags_json.trim();
    let inner = inner.strip_prefix('[').and_then(|s| s.strip_suffix(']'));
    let Some(inner) = inner else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if !in_quotes => in_quotes = true,
            '"' => {
                if chars.peek() == Some(&'"') {
                    current.push(chars.next().expect("peeked quote"));
                } else {
                    in_quotes = false;
                }
            }
            '\\' if in_quotes => {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            ',' if !in_quotes => {
                let tag = current.trim().to_string();
                if !tag.is_empty() {
                    tags.push(tag);
                }
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    let tag = current.trim().to_string();
    if !tag.is_empty() {
        tags.push(tag);
    }
    tags
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
                let kind =
                    LibraryKind::from_invocation(alias.invocation_type, item.action_type.as_str());
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
        // honey: confirmation lives per alias row, mirroring the voice
        // gate; sibling rows keep their own flags.
        let require_confirmation = item
            .invocations
            .iter()
            .find(|alias| alias.invocation == trigger && alias.invocation_type == invocation)
            .map(|alias| alias.require_confirmation)
            .unwrap_or(false);
        Self {
            id: item.id.clone(),
            trigger,
            preview: preview_from_item(item),
            content: content_from_item(item),
            kind,
            target_os: target_os.to_string(),
            invocation,
            aliases: item
                .invocations
                .iter()
                .map(|a| a.invocation.clone())
                .collect(),
            description: item.description.clone(),
            tags: parse_tags(&item.tags),
            only_apps: item.only_apps.clone(),
            except_apps: item.except_apps.clone(),
            auto_case: item.auto_case,
            is_enabled: item.is_enabled,
            require_confirmation,
            usage_count: item.usage_count,
            last_used_at: item.last_used_at,
            created_at: item.created_at,
            interpreter: item.interpreter,
            behavior: item.behavior,
            search_text,
        }
    }

    // honey: unreachable until the shortcut rework lands; kept with tests.
    #[allow(dead_code)]
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn trigger(&self) -> &str {
        &self.trigger
    }

    pub(crate) fn preview(&self) -> &str {
        &self.preview
    }

    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    pub(crate) const fn kind_label(&self) -> &'static str {
        self.kind.label()
    }

    pub(crate) const fn invocation_type_label(&self) -> &'static str {
        match self.invocation {
            InvocationType::Word => "word",
            InvocationType::Hotkey => "hotkey",
            InvocationType::Regex => "regex",
            InvocationType::Voice => "voice",
        }
    }

    pub(crate) const fn is_voice(&self) -> bool {
        matches!(self.invocation, InvocationType::Voice)
    }

    pub(crate) fn aliases(&self) -> &[String] {
        &self.aliases
    }

    pub(crate) fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub(crate) fn tags(&self) -> &[String] {
        &self.tags
    }

    pub(crate) fn only_apps(&self) -> Option<&str> {
        self.only_apps.as_deref()
    }

    pub(crate) fn except_apps(&self) -> Option<&str> {
        self.except_apps.as_deref()
    }

    pub(crate) const fn auto_case(&self) -> bool {
        self.auto_case
    }

    pub(crate) const fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    pub(crate) const fn require_confirmation(&self) -> bool {
        self.require_confirmation
    }

    pub(crate) const fn is_script(&self) -> bool {
        matches!(
            self.kind,
            LibraryKind::TextScript
                | LibraryKind::HotkeyScript
                | LibraryKind::RegexScript
                | LibraryKind::VoiceScript
        )
    }

    pub(crate) const fn interpreter(&self) -> Option<ScriptInterpreter> {
        self.interpreter
    }

    pub(crate) const fn behavior(&self) -> Option<ScriptBehavior> {
        self.behavior
    }

    pub(crate) const fn usage_count(&self) -> i64 {
        self.usage_count
    }

    pub(crate) const fn last_used_at(&self) -> Option<i64> {
        self.last_used_at
    }

    pub(crate) const fn created_at(&self) -> i64 {
        self.created_at
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
