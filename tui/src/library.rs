use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::{
    db::crud::{AutomationListItem, AutomationRow, TriggerType},
    engine::shell::{ScriptBehavior, ScriptInterpreter, decompress},
};

const LIBRARY_FOOTER: &str = "/ Search   n New   e Edit   d Delete   Enter Details   q Quit";
const LIBRARY_MODAL_FOOTER: &str = "Esc Close   Tab Next   Shift+Tab Prev";
const DEFAULT_SCRIPT_FALLBACK: &str = "Script content unavailable.";
const DEFAULT_OUTPUT_FALLBACK: &str = "No output available.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryKind {
    Snippet,
    Script,
    HotkeySnippet,
    HotkeyScript,
}

impl LibraryKind {
    pub(crate) fn from_parts(trigger_type: TriggerType, action_type: &str) -> Self {
        let is_script = action_type.eq_ignore_ascii_case("script");

        match (trigger_type, is_script) {
            (TriggerType::Hotkey, true) => Self::HotkeyScript,
            (TriggerType::Hotkey, false) => Self::HotkeySnippet,
            (TriggerType::Word, true) => Self::Script,
            (TriggerType::Word, false) => Self::Snippet,
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

    fn is_script(self) -> bool {
        matches!(self, Self::Script | Self::HotkeyScript)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryAutomation {
    id: String,
    trigger: String,
    preview: String,
    kind: LibraryKind,
    target_os: String,
    raw_target_os: String,
    search_text: String,
    uses: u64,
}

impl LibraryAutomation {
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

    fn matches_query(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }

        let needle = query.to_ascii_lowercase();
        self.search_text.contains(&needle)
    }
}

impl From<AutomationListItem> for LibraryAutomation {
    fn from(item: AutomationListItem) -> Self {
        let kind = LibraryKind::from_parts(item.trigger_type, item.action_type.as_str());
        let preview = preview_from_item(&item);
        let target_os = display_target_os(&item.target_os).to_string();
        let search_text = build_search_text(&item, kind.label(), &target_os);

        Self {
            id: item.id,
            trigger: item.trigger,
            preview,
            kind,
            target_os,
            raw_target_os: item.target_os,
            search_text,
            uses: item.usage_count.max(0) as u64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryModalField {
    Trigger,
    Content,
    Kind,
    TargetOs,
}

impl LibraryModalField {
    fn next(self) -> Self {
        match self {
            Self::Trigger => Self::Content,
            Self::Content => Self::Kind,
            Self::Kind => Self::TargetOs,
            Self::TargetOs => Self::Trigger,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Trigger => Self::TargetOs,
            Self::Content => Self::Trigger,
            Self::Kind => Self::Content,
            Self::TargetOs => Self::Kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryMetadataRow {
    label: &'static str,
    value: String,
}

impl LibraryMetadataRow {
    fn new(label: &'static str, value: String) -> Self {
        Self { label, value }
    }

    pub(crate) const fn label(&self) -> &'static str {
        self.label
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryAutomationDetail {
    trigger: String,
    content_label: &'static str,
    content: String,
    kind_label: &'static str,
    target_os: String,
    metadata_rows: Vec<LibraryMetadataRow>,
}

impl LibraryAutomationDetail {
    pub(crate) fn from_row(row: AutomationRow) -> taurine_core::Result<Self> {
        let kind = LibraryKind::from_parts(row.trigger_type, row.action_type.as_str());
        let content = modal_content_from_row(&row, kind)?;
        let target_os = display_target_os(&row.target_os).to_string();
        let metadata_rows = build_metadata_rows(&row);

        Ok(Self {
            trigger: row.trigger,
            content_label: kind.content_label(),
            content,
            kind_label: kind.label(),
            target_os,
            metadata_rows,
        })
    }

    pub(crate) fn trigger(&self) -> &str {
        &self.trigger
    }

    pub(crate) const fn content_label(&self) -> &'static str {
        self.content_label
    }

    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    pub(crate) const fn kind_label(&self) -> &'static str {
        self.kind_label
    }

    pub(crate) fn target_os(&self) -> &str {
        &self.target_os
    }

    pub(crate) fn metadata_rows(&self) -> &[LibraryMetadataRow] {
        &self.metadata_rows
    }

    pub(crate) fn content_line_count(&self) -> usize {
        self.content.lines().count().max(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryEditorModalState {
    automation: LibraryAutomationDetail,
    focus: LibraryModalField,
    content_scroll: usize,
}

impl LibraryEditorModalState {
    fn new(automation: LibraryAutomationDetail) -> Self {
        Self {
            automation,
            focus: LibraryModalField::Trigger,
            content_scroll: 0,
        }
    }

    pub(crate) const fn automation(&self) -> &LibraryAutomationDetail {
        &self.automation
    }

    pub(crate) const fn focus(&self) -> LibraryModalField {
        self.focus
    }

    pub(crate) fn effective_content_scroll(&self, visible_lines: u16) -> usize {
        let max_scroll = self
            .automation
            .content_line_count()
            .saturating_sub(visible_lines.max(1) as usize);
        self.content_scroll.min(max_scroll)
    }

    pub(crate) fn content_line_indicator(&self, visible_lines: u16) -> Option<String> {
        let total_lines = self.automation.content_line_count();
        if total_lines <= 1 {
            return None;
        }

        let current_line = self
            .effective_content_scroll(visible_lines)
            .saturating_add(1);
        Some(format!("{current_line}/{total_lines}"))
    }

    fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => LibraryInteraction::CloseModal,
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.focus = self.focus.next();
                LibraryInteraction::None
            }
            (KeyCode::BackTab, _) => {
                self.focus = self.focus.previous();
                LibraryInteraction::None
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE)
                if self.focus == LibraryModalField::Content =>
            {
                self.scroll_content(1);
                LibraryInteraction::None
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE)
                if self.focus == LibraryModalField::Content =>
            {
                self.scroll_content(-1);
                LibraryInteraction::None
            }
            (KeyCode::PageDown, KeyModifiers::NONE) if self.focus == LibraryModalField::Content => {
                self.scroll_content(5);
                LibraryInteraction::None
            }
            (KeyCode::PageUp, KeyModifiers::NONE) if self.focus == LibraryModalField::Content => {
                self.scroll_content(-5);
                LibraryInteraction::None
            }
            (KeyCode::Home, KeyModifiers::NONE) if self.focus == LibraryModalField::Content => {
                self.content_scroll = 0;
                LibraryInteraction::None
            }
            (KeyCode::End, KeyModifiers::NONE) if self.focus == LibraryModalField::Content => {
                self.content_scroll = self.automation.content_line_count().saturating_sub(1);
                LibraryInteraction::None
            }
            _ => LibraryInteraction::None,
        }
    }

    fn scroll_content(&mut self, delta: isize) {
        let max_scroll = self.automation.content_line_count().saturating_sub(1) as isize;
        let next = (self.content_scroll as isize + delta).clamp(0, max_scroll);
        self.content_scroll = next as usize;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LibraryModal {
    Editor(LibraryEditorModalState),
}

impl LibraryModal {
    pub(crate) const fn editor(&self) -> &LibraryEditorModalState {
        match self {
            Self::Editor(state) => state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum LibraryInteraction {
    #[default]
    None,
    OpenSelectedAutomation(String),
    CloseModal,
}

impl LibraryInteraction {
    pub(crate) fn into_open_selected_id(self) -> Option<String> {
        match self {
            Self::OpenSelectedAutomation(id) => Some(id),
            Self::None | Self::CloseModal => None,
        }
    }

    pub(crate) const fn should_close_modal(&self) -> bool {
        matches!(self, Self::CloseModal)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct LibraryPageState {
    items: Vec<LibraryAutomation>,
    filtered_indices: Vec<usize>,
    selected: usize,
    search_query: String,
    search_mode: bool,
    modal: Option<LibraryModal>,
    load_error: Option<String>,
}

impl LibraryPageState {
    pub(crate) fn replace_items(&mut self, mut items: Vec<LibraryAutomation>) {
        sort_items(&mut items);
        self.items = items;
        self.load_error = None;
        self.rebuild_filter();
    }

    pub(crate) fn set_load_error(&mut self, error: String) {
        self.load_error = Some(error);
    }

    pub(crate) fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub(crate) fn footer_text(&self) -> &'static str {
        if self.modal.is_some() {
            LIBRARY_MODAL_FOOTER
        } else {
            LIBRARY_FOOTER
        }
    }

    pub(crate) fn search_query(&self) -> &str {
        &self.search_query
    }

    pub(crate) const fn is_search_active(&self) -> bool {
        self.search_mode
    }

    pub(crate) const fn is_modal_open(&self) -> bool {
        self.modal.is_some()
    }

    pub(crate) const fn modal(&self) -> Option<&LibraryModal> {
        self.modal.as_ref()
    }

    pub(crate) fn open_editor_modal(&mut self, automation: LibraryAutomationDetail) {
        self.modal = Some(LibraryModal::Editor(LibraryEditorModalState::new(
            automation,
        )));
    }

    pub(crate) fn clear_modal(&mut self) {
        self.modal = None;
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        if self.filtered_indices.is_empty() {
            None
        } else {
            Some(
                self.selected
                    .min(self.filtered_indices.len().saturating_sub(1)),
            )
        }
    }

    pub(crate) fn filtered_len(&self) -> usize {
        self.filtered_indices.len()
    }

    pub(crate) fn item_at_filtered(&self, index: usize) -> Option<&LibraryAutomation> {
        self.filtered_indices
            .get(index)
            .and_then(|item_index| self.items.get(*item_index))
    }

    pub(crate) fn empty_state_message(&self) -> Option<&'static str> {
        if self.items.is_empty() {
            Some("No automations yet.")
        } else if self.filtered_indices.is_empty() {
            Some("No automations match your search.")
        } else {
            None
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }

        if self.search_mode {
            self.handle_search_key(key);
            return LibraryInteraction::None;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.search_mode = true;
                LibraryInteraction::None
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_selection(1);
                LibraryInteraction::None
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_selection(-1);
                LibraryInteraction::None
            }
            (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Char('e'), KeyModifiers::NONE) => self
                .selected_item()
                .map(|item| LibraryInteraction::OpenSelectedAutomation(item.id().to_string()))
                .unwrap_or_default(),
            _ => LibraryInteraction::None,
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        let Some(modal) = self.modal.as_mut() else {
            return LibraryInteraction::None;
        };

        match modal {
            LibraryModal::Editor(state) => state.handle_key(key),
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => self.search_mode = false,
            (KeyCode::Enter, KeyModifiers::NONE) => self.search_mode = false,
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.search_query.pop();
                self.rebuild_filter();
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_query.push(ch);
                self.rebuild_filter();
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let Some(current) = self.selected_index() else {
            self.selected = 0;
            return;
        };

        let max_index = self.filtered_indices.len().saturating_sub(1) as isize;
        let next = (current as isize + delta).clamp(0, max_index);
        self.selected = next as usize;
    }

    fn rebuild_filter(&mut self) {
        let previously_selected = self.selected_item().cloned();
        self.filtered_indices = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.matches_query(&self.search_query).then_some(index))
            .collect();

        if self.filtered_indices.is_empty() {
            self.selected = 0;
            return;
        }

        if let Some(previous_item) = previously_selected
            && let Some(position) = self
                .filtered_indices
                .iter()
                .position(|item_index| self.items[*item_index] == previous_item)
        {
            self.selected = position;
            return;
        }

        self.selected = 0;
    }

    fn selected_item(&self) -> Option<&LibraryAutomation> {
        self.selected_index()
            .and_then(|selected| self.item_at_filtered(selected))
    }
}

fn sort_items(items: &mut [LibraryAutomation]) {
    items.sort_by(|left, right| {
        let left_trigger = left.trigger.to_ascii_lowercase();
        let right_trigger = right.trigger.to_ascii_lowercase();

        left_trigger
            .cmp(&right_trigger)
            .then_with(|| left.kind_label().cmp(right.kind_label()))
            .then_with(|| left.target_os.cmp(&right.target_os))
            .then_with(|| left.preview.cmp(&right.preview))
    });
}

fn preview_from_item(item: &AutomationListItem) -> String {
    if let Some(description) = normalized_preview_text(item.description.as_deref())
        && !is_script_placeholder(&description)
    {
        return description;
    }

    if item.action_type.eq_ignore_ascii_case("script") {
        if let Some(script_content) = normalized_preview_text(item.script_content.as_deref()) {
            return script_content;
        }

        if let Some(output) = normalized_preview_text(Some(item.output.as_str()))
            && !is_script_placeholder(&output)
        {
            return output;
        }

        return DEFAULT_SCRIPT_FALLBACK.to_string();
    }

    if let Some(output) = normalized_preview_text(Some(item.output.as_str())) {
        return output;
    }

    if let Some(script_content) = normalized_preview_text(item.script_content.as_deref()) {
        return script_content;
    }

    "No preview available.".to_string()
}

fn modal_content_from_row(row: &AutomationRow, kind: LibraryKind) -> taurine_core::Result<String> {
    if kind.is_script() {
        if let Some(script_content) = load_script_content(row)? {
            return Ok(script_content);
        }

        if let Some(output) = normalized_modal_text(Some(row.output.as_str()))
            && !is_script_placeholder(&output)
        {
            return Ok(output);
        }

        return Ok(DEFAULT_SCRIPT_FALLBACK.to_string());
    }

    Ok(normalized_modal_text(Some(row.output.as_str()))
        .unwrap_or_else(|| DEFAULT_OUTPUT_FALLBACK.to_string()))
}

fn build_metadata_rows(row: &AutomationRow) -> Vec<LibraryMetadataRow> {
    let mut rows = Vec::new();

    if let Some(interpreter) = row.interpreter {
        rows.push(LibraryMetadataRow::new(
            "Language",
            interpreter_label(interpreter).to_string(),
        ));
    }

    if let Some(behavior) = row.behavior {
        rows.push(LibraryMetadataRow::new(
            "Mode",
            behavior_label(behavior).to_string(),
        ));
    }

    rows.push(LibraryMetadataRow::new(
        "Uses",
        format_usage_count(row.usage_count.max(0) as u64),
    ));

    if let Some(last_used_at) = row.last_used_at.and_then(format_relative_time) {
        rows.push(LibraryMetadataRow::new("Last used", last_used_at));
    }

    if let Some(created_at) = format_relative_time(row.created_at) {
        rows.push(LibraryMetadataRow::new("Created", created_at));
    }

    if let Some(updated_at) = format_relative_time(row.updated_at) {
        rows.push(LibraryMetadataRow::new("Updated", updated_at));
    }

    rows
}

fn load_script_content(row: &AutomationRow) -> taurine_core::Result<Option<String>> {
    row.script_binary
        .as_deref()
        .map(decompress)
        .transpose()
        .map(|content| content.and_then(|content| normalized_modal_text(Some(content.as_str()))))
}

fn build_search_text(
    item: &AutomationListItem,
    kind_label: &str,
    display_target_os: &str,
) -> String {
    let mut parts = vec![
        item.trigger.as_str(),
        item.output.as_str(),
        kind_label,
        display_target_os,
        item.target_os.as_str(),
    ];

    if let Some(description) = item.description.as_deref() {
        parts.push(description);
    }

    if let Some(script_content) = item.script_content.as_deref() {
        parts.push(script_content);
    }

    parts
        .into_iter()
        .filter_map(|part| normalized_preview_text(Some(part)))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalized_preview_text(value: Option<&str>) -> Option<String> {
    let value = value?;
    let first_non_empty = value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(value.trim());

    let collapsed = first_non_empty
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

fn normalized_modal_text(value: Option<&str>) -> Option<String> {
    let trimmed = value?.replace("\r\n", "\n");
    let trimmed = trimmed.trim();
    (!trimmed.is_empty()).then_some(trimmed.to_string())
}

fn is_script_placeholder(value: &str) -> bool {
    let normalized = value.trim();
    (normalized.starts_with("[Script:") && normalized.ends_with(']'))
        || normalized
            .to_ascii_lowercase()
            .starts_with("shell script (")
}

fn display_target_os(target_os: &str) -> &str {
    match target_os {
        "all" => "all",
        "win" => "windows",
        "mac" => "macos",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        _ => target_os,
    }
}

fn interpreter_label(interpreter: ScriptInterpreter) -> &'static str {
    match interpreter {
        ScriptInterpreter::Bash => "bash",
        ScriptInterpreter::PowerShell => "powershell",
        ScriptInterpreter::Python => "python",
        ScriptInterpreter::Node => "node",
        ScriptInterpreter::NodeEsm => "node esm",
        ScriptInterpreter::Cmd => "cmd",
    }
}

fn behavior_label(behavior: ScriptBehavior) -> &'static str {
    match behavior {
        ScriptBehavior::Inline => "inline",
        ScriptBehavior::Silent => "silent",
    }
}

fn format_usage_count(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }

    formatted.chars().rev().collect()
}

fn format_relative_time(timestamp: i64) -> Option<String> {
    if timestamp <= 0 {
        return None;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)?;
    let diff = now.saturating_sub(timestamp);

    if diff < 60 {
        Some("just now".to_string())
    } else {
        let minutes = diff / 60;
        if minutes < 60 {
            return Some(format!("{minutes}m ago"));
        }

        let hours = minutes / 60;
        if hours < 24 {
            return Some(format!("{hours}h ago"));
        }

        let days = hours / 24;
        if days < 30 {
            return Some(format!("{days}d ago"));
        }

        let months = days / 30;
        if months < 12 {
            return Some(format!("{months}mo ago"));
        }

        Some(format!("{}y ago", days / 365))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn list_item(
        id: &str,
        description: Option<&str>,
        trigger_type: TriggerType,
        trigger: &str,
        output: &str,
        action_type: &str,
        target_os: &str,
        usage_count: i64,
        script_content: Option<&str>,
    ) -> AutomationListItem {
        AutomationListItem {
            id: id.to_string(),
            description: description.map(str::to_string),
            trigger_type,
            trigger: trigger.to_string(),
            output: output.to_string(),
            action_type: action_type.to_string(),
            target_os: target_os.to_string(),
            usage_count,
            last_used_at: None,
            created_at: 0,
            script_content: script_content.map(str::to_string),
            interpreter: None,
            behavior: None,
        }
    }

    fn automation_row(
        trigger_type: TriggerType,
        trigger: &str,
        output: &str,
        action_type: &str,
        target_os: &str,
        usage_count: i64,
        script_content: Option<&str>,
    ) -> AutomationRow {
        AutomationRow {
            id: format!("automation-{trigger}"),
            name: format!("Automation {trigger}"),
            description: Some("Open Reddit".to_string()),
            trigger_type,
            trigger: trigger.to_string(),
            output: output.to_string(),
            action_type: action_type.to_string(),
            target_os: target_os.to_string(),
            tags: "[]".to_string(),
            usage_count,
            last_used_at: Some(1),
            created_at: 1,
            updated_at: 1,
            version: 1,
            is_deleted: false,
            is_synced: true,
            is_enabled: true,
            interpreter: Some(ScriptInterpreter::PowerShell),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: script_content
                .map(|content| taurine_core::engine::shell::compress(content).unwrap()),
        }
    }

    fn sample_state() -> LibraryPageState {
        let mut state = LibraryPageState::default();
        state.replace_items(vec![
            LibraryAutomation::from(list_item(
                "id-gm",
                None,
                TriggerType::Word,
                "gm",
                "Good Morning",
                "text",
                "all",
                9,
                None,
            )),
            LibraryAutomation::from(list_item(
                "id-deploy",
                None,
                TriggerType::Word,
                "deploy",
                "[Script: bash]",
                "script",
                "linux",
                4,
                Some("npm run build && npm publish"),
            )),
            LibraryAutomation::from(list_item(
                "id-alt+r",
                Some("Open Reddit"),
                TriggerType::Hotkey,
                "alt+r",
                "[Script: powershell]",
                "script",
                "win",
                6,
                Some("Start-Process https://reddit.com"),
            )),
        ]);
        state
    }

    #[test]
    fn word_text_maps_to_snippet() {
        let item = LibraryAutomation::from(list_item(
            "id-gm",
            None,
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ));
        assert_eq!(item.kind_label(), "snippet");
    }

    #[test]
    fn word_script_maps_to_script() {
        let item = LibraryAutomation::from(list_item(
            "id-deploy",
            None,
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "all",
            4,
            Some("npm publish"),
        ));
        assert_eq!(item.kind_label(), "script");
    }

    #[test]
    fn hotkey_text_maps_to_hotkey_snippet() {
        let item = LibraryAutomation::from(list_item(
            "id-thanks",
            None,
            TriggerType::Hotkey,
            "alt+t",
            "Thanks!",
            "text",
            "all",
            12,
            None,
        ));
        assert_eq!(item.kind_label(), "hotkey snippet");
    }

    #[test]
    fn hotkey_script_maps_to_hotkey_script() {
        let item = LibraryAutomation::from(list_item(
            "id-alt+r",
            None,
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ));
        assert_eq!(item.kind_label(), "hotkey script");
    }

    #[test]
    fn preview_prefers_description_before_other_content() {
        let item = LibraryAutomation::from(list_item(
            "id-alt+r",
            Some("Open Reddit"),
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ));

        assert_eq!(item.preview(), "Open Reddit");
    }

    #[test]
    fn placeholder_script_description_does_not_block_real_script_preview() {
        let item = LibraryAutomation::from(list_item(
            "id-alt+r",
            Some("Shell script (CLI argument)"),
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ));

        assert_eq!(item.preview(), "Start-Process https://reddit.com");
    }

    #[test]
    fn preview_falls_back_to_text_output_when_description_is_empty() {
        let item = LibraryAutomation::from(list_item(
            "id-gm",
            Some("   "),
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ));

        assert_eq!(item.preview(), "Good Morning");
    }

    #[test]
    fn preview_falls_back_to_script_content_when_description_is_empty() {
        let item = LibraryAutomation::from(list_item(
            "id-alt+r",
            None,
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ));

        assert_eq!(item.preview(), "Start-Process https://reddit.com");
    }

    #[test]
    fn script_preview_does_not_use_script_language_placeholder() {
        let item = LibraryAutomation::from(list_item(
            "id-deploy",
            None,
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "all",
            4,
            Some("npm run build && npm publish"),
        ));

        assert_ne!(item.preview(), "[Script: bash]");
        assert_eq!(item.preview(), "npm run build && npm publish");
    }

    #[test]
    fn script_preview_does_not_use_shell_script_description_placeholder() {
        let item = LibraryAutomation::from(list_item(
            "id-deploy",
            Some("Shell script (CLI argument)"),
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "all",
            4,
            Some("npm run build && npm publish"),
        ));

        assert_ne!(item.preview(), "Shell script (CLI argument)");
        assert_eq!(item.preview(), "npm run build && npm publish");
    }

    #[test]
    fn empty_script_content_falls_back_safely() {
        let item = LibraryAutomation::from(list_item(
            "id-deploy",
            Some("Shell script (CLI argument)"),
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "all",
            4,
            Some("   "),
        ));

        assert_eq!(item.preview(), DEFAULT_SCRIPT_FALLBACK);
    }

    #[test]
    fn search_matches_trigger() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn search_matches_preview() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "publish".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "deploy");
    }

    #[test]
    fn search_matches_description_when_available() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "open".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
    }

    #[test]
    fn search_matches_script_content_even_when_description_is_visible() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "start-process".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
    }

    #[test]
    fn search_matches_kind_label() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "hotkey".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
    }

    #[test]
    fn search_matches_target_os() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "windows".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().target_os, "windows");
    }

    #[test]
    fn search_is_case_insensitive() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "GOOD".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::SHIFT));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn selection_clamps_at_bounds() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(state.selected_index(), Some(0));

        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

        assert_eq!(state.selected_index(), Some(2));
    }

    #[test]
    fn selection_moves_to_first_match_when_filter_removes_selected_item() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "good".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.selected_index(), Some(0));
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn empty_list_reports_empty_state() {
        let state = LibraryPageState::default();
        assert_eq!(state.empty_state_message(), Some("No automations yet."));
    }

    #[test]
    fn no_match_search_reports_no_match_state() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "zzz".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(
            state.empty_state_message(),
            Some("No automations match your search.")
        );
    }

    #[test]
    fn metadata_uses_double_slash_separator() {
        let item = LibraryAutomation::from(list_item(
            "id-gm",
            None,
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ));

        assert_eq!(item.metadata_label(), "all // 9 uses");
    }

    #[test]
    fn pressing_enter_requests_selected_automation_modal() {
        let mut state = sample_state();
        let expected_id = state
            .selected_index()
            .and_then(|index| state.item_at_filtered(index))
            .map(|item| item.id().to_string());

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(interaction.into_open_selected_id(), expected_id);
    }

    #[test]
    fn pressing_e_requests_modal_for_selected_automation() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        let expected_id = state
            .selected_index()
            .and_then(|index| state.item_at_filtered(index))
            .map(|item| item.id().to_string());

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));

        assert_eq!(interaction.into_open_selected_id(), expected_id);
    }

    #[test]
    fn pressing_escape_closes_open_modal() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap();
        state.open_editor_modal(detail);

        let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(interaction.should_close_modal());
    }

    #[test]
    fn script_modal_uses_actual_script_content_instead_of_description() {
        let mut row = automation_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        );
        row.description = Some("Open Reddit".to_string());

        let detail = LibraryAutomationDetail::from_row(row).unwrap();

        assert_eq!(detail.content_label(), "Script");
        assert_eq!(detail.content(), "Start-Process https://reddit.com");
    }

    #[test]
    fn snippet_modal_uses_actual_output_content() {
        let row = automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        );

        let detail = LibraryAutomationDetail::from_row(row).unwrap();

        assert_eq!(detail.content_label(), "Output");
        assert_eq!(detail.content(), "Good Morning");
    }

    #[test]
    fn modal_footer_replaces_library_actions_while_open() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();

        state.open_editor_modal(detail);

        assert_eq!(state.footer_text(), LIBRARY_MODAL_FOOTER);
    }

    #[test]
    fn modal_owns_input_and_keeps_search_inactive() {
        let mut state = sample_state();
        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap();
        state.open_editor_modal(detail);

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert!(!state.is_search_active());
        assert!(state.is_modal_open());
    }

    #[test]
    fn tab_and_shift_tab_cycle_modal_focus() {
        let mut modal = LibraryEditorModalState::new(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Word,
                "gm",
                "Good Morning",
                "text",
                "all",
                9,
                None,
            ))
            .unwrap(),
        );

        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(modal.focus(), LibraryModalField::Content);

        modal.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(modal.focus(), LibraryModalField::Trigger);
    }

    #[test]
    fn content_focus_supports_scrolling() {
        let mut modal = LibraryEditorModalState::new(
            LibraryAutomationDetail::from_row(automation_row(
                TriggerType::Word,
                "gm",
                "line one\nline two\nline three",
                "text",
                "all",
                9,
                None,
            ))
            .unwrap(),
        );
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert_eq!(modal.content_line_indicator(1).as_deref(), Some("2/3"));
    }

    #[test]
    fn modal_keeps_library_selection_stable_after_close() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        let selected_before = state.selected_index();

        let detail = LibraryAutomationDetail::from_row(automation_row(
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "linux",
            4,
            Some("npm run build && npm publish"),
        ))
        .unwrap();
        state.open_editor_modal(detail);
        state.clear_modal();

        assert_eq!(state.selected_index(), selected_before);
        assert_eq!(state.search_query(), "");
    }
}
