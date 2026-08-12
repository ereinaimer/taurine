use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::db::crud::SUPPORTED_TARGET_OS_VALUES;
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

use crate::widgets::library::actions::{
    LibraryInteraction, PendingLibrarySave, PendingLibrarySaveMode, behavior_label,
    char_index_for_line_col, char_index_to_byte_index, default_script_interpreter_for_target_os,
    display_target_os, interpreter_label, line_col_for_char_index, line_lengths,
    line_start_positions, split_lines_with_trailing,
};

use super::trigger::{LibraryKind, LibrarySelectState, LibraryTriggerDetail};
use super::{LibraryMetadataRow, LibraryModalField};

pub(crate) const LIBRARY_EDIT_MODAL_FOOTER: &str =
    "Ctrl+S Save   Esc Cancel   Tab Next   Shift+Tab Prev";
pub(crate) const LIBRARY_CREATE_MODAL_FOOTER: &str =
    "Ctrl+S Save   Esc Cancel   Tab Next   Shift+Tab Prev";
pub(crate) const SCRIPT_LANGUAGE_OPTIONS: [ScriptInterpreter; 5] = [
    ScriptInterpreter::Bash,
    ScriptInterpreter::PowerShell,
    ScriptInterpreter::Python,
    ScriptInterpreter::Node,
    ScriptInterpreter::Cmd,
];
pub(crate) const SCRIPT_MODE_OPTIONS: [ScriptBehavior; 2] = ScriptBehavior::ALL;
pub(crate) const SNIPPET_MODAL_FIELDS: [LibraryModalField; 4] = [
    LibraryModalField::Trigger,
    LibraryModalField::Content,
    LibraryModalField::Kind,
    LibraryModalField::TargetOs,
];
pub(crate) const SCRIPT_MODAL_FIELDS: [LibraryModalField; 6] = [
    LibraryModalField::Trigger,
    LibraryModalField::Content,
    LibraryModalField::Kind,
    LibraryModalField::TargetOs,
    LibraryModalField::Language,
    LibraryModalField::Mode,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryEditorMode {
    Edit,
    Create,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryEditorModalState {
    mode: LibraryEditorMode,
    original: Option<LibraryTriggerDetail>,
    trigger: String,
    trigger_cursor: usize,
    content: String,
    content_cursor: usize,
    content_cursor_goal: Option<usize>,
    kind: LibraryKind,
    target_os: String,
    interpreter: ScriptInterpreter,
    behavior: ScriptBehavior,
    focus: LibraryModalField,
    content_scroll: usize,
    error: Option<String>,
    selector: Option<LibrarySelectState>,
}

impl LibraryEditorModalState {
    pub(crate) fn new_edit(trigger: LibraryTriggerDetail) -> Self {
        let trigger_cursor = trigger.trigger().chars().count();
        let content_cursor = trigger.content().chars().count();
        Self {
            mode: LibraryEditorMode::Edit,
            trigger: trigger.trigger().to_string(),
            trigger_cursor,
            content: trigger.content().to_string(),
            content_cursor,
            content_cursor_goal: None,
            kind: trigger.kind(),
            target_os: trigger.target_os_raw().to_string(),
            interpreter: trigger.interpreter().unwrap_or_else(|| {
                default_script_interpreter_for_target_os(trigger.target_os_raw())
            }),
            behavior: trigger.behavior().unwrap_or(ScriptBehavior::Inline),
            focus: LibraryModalField::Trigger,
            content_scroll: 0,
            error: None,
            original: Some(trigger),
            selector: None,
        }
    }

    pub(crate) fn new_create() -> Self {
        Self {
            mode: LibraryEditorMode::Create,
            original: None,
            trigger: String::new(),
            trigger_cursor: 0,
            content: String::new(),
            content_cursor: 0,
            content_cursor_goal: None,
            kind: LibraryKind::Snippet,
            target_os: SUPPORTED_TARGET_OS_VALUES[0].to_string(),
            interpreter: default_script_interpreter_for_target_os(SUPPORTED_TARGET_OS_VALUES[0]),
            behavior: ScriptBehavior::Inline,
            focus: LibraryModalField::Trigger,
            content_scroll: 0,
            error: None,
            selector: None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn mode(&self) -> LibraryEditorMode {
        self.mode
    }

    pub(crate) const fn focus(&self) -> LibraryModalField {
        self.focus
    }

    pub(crate) fn trigger(&self) -> &str {
        &self.trigger
    }

    pub(crate) const fn trigger_cursor(&self) -> usize {
        self.trigger_cursor
    }

    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    pub(crate) const fn content_cursor(&self) -> usize {
        self.content_cursor
    }

    pub(crate) const fn kind_label(&self) -> &'static str {
        self.kind.label()
    }

    pub(crate) const fn content_label(&self) -> &'static str {
        self.kind.content_label()
    }

    pub(crate) fn target_os(&self) -> &str {
        display_target_os(&self.target_os)
    }

    pub(crate) const fn is_script_kind(&self) -> bool {
        self.kind.is_script()
    }

    pub(crate) const fn language_label(&self) -> &'static str {
        interpreter_label(self.interpreter)
    }

    pub(crate) const fn mode_label(&self) -> &'static str {
        behavior_label(self.behavior)
    }

    #[cfg(test)]
    pub(crate) const fn interpreter(&self) -> ScriptInterpreter {
        self.interpreter
    }

    #[cfg(test)]
    pub(crate) const fn behavior(&self) -> ScriptBehavior {
        self.behavior
    }

    pub(crate) fn metadata_rows(&self) -> &[LibraryMetadataRow] {
        self.original
            .as_ref()
            .map(LibraryTriggerDetail::metadata_rows)
            .unwrap_or(&[])
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub(crate) fn effective_content_scroll(&self, visible_lines: u16) -> usize {
        let max_scroll = self
            .content_line_count()
            .saturating_sub(visible_lines.max(1) as usize);
        self.content_scroll.min(max_scroll)
    }

    pub(crate) fn content_line_indicator(&self, _visible_lines: u16) -> Option<String> {
        let total_lines = self.content_line_count();
        if total_lines <= 1 {
            return None;
        }

        Some(format!(
            "{}/{}",
            self.current_content_line() + 1,
            total_lines
        ))
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        if self.selector.is_some() {
            return self.handle_selector_key(key);
        }

        self.error = None;

        if matches!(key.code, KeyCode::Char('s' | 'S'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return LibraryInteraction::save(self.build_pending_save());
        }

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => LibraryInteraction::close(),
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.advance_focus(true);
                LibraryInteraction::handled()
            }
            (KeyCode::BackTab, _) => {
                self.advance_focus(false);
                LibraryInteraction::handled()
            }
            _ => self.handle_focused_key(key),
        }
    }

    pub(crate) fn visible_content_lines(&self, visible_lines: u16) -> Vec<String> {
        let lines = split_lines_with_trailing(&self.content);
        let scroll = self.effective_content_scroll(visible_lines);
        let mut visible = lines
            .into_iter()
            .skip(scroll)
            .take(visible_lines.max(1) as usize)
            .map(str::to_string)
            .collect::<Vec<_>>();
        if visible.is_empty() {
            visible.push(String::new());
        }
        visible
    }

    pub(crate) fn build_pending_save(&self) -> PendingLibrarySave {
        let mode = match (self.mode, self.original.as_ref()) {
            (LibraryEditorMode::Edit, Some(original)) => PendingLibrarySaveMode::Update {
                id: original.id().to_string(),
                name: original.name().to_string(),
                description: original.description().map(str::to_string),
                tags_json: original.tags_json().to_string(),
                usage_count: original.usage_count(),
                last_used_at: original.last_used_at(),
                interpreter: original.interpreter(),
                behavior: original.behavior(),
            },
            (LibraryEditorMode::Create, _) => PendingLibrarySaveMode::Create,
            (LibraryEditorMode::Edit, None) => PendingLibrarySaveMode::Create,
        };

        PendingLibrarySave {
            mode,
            trigger: self.trigger.clone(),
            content: self.content.clone(),
            kind: self.kind,
            target_os: self.target_os.clone(),
            interpreter: self.kind.is_script().then_some(self.interpreter),
            behavior: self.kind.is_script().then_some(self.behavior),
        }
    }

    fn handle_focused_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match self.focus {
            LibraryModalField::Trigger => self.handle_trigger_key(key),
            LibraryModalField::Content => self.handle_content_key(key),
            LibraryModalField::Kind => self.handle_kind_key(key),
            LibraryModalField::TargetOs => self.handle_target_os_key(key),
            LibraryModalField::Language => self.handle_language_key(key),
            LibraryModalField::Mode => self.handle_mode_key(key),
        }
    }

    fn handle_trigger_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.backspace_trigger();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.delete_trigger();
                LibraryInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.trigger_cursor = self.trigger_cursor.saturating_sub(1);
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.trigger_cursor = (self.trigger_cursor + 1).min(self.trigger.chars().count());
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.trigger_cursor = 0;
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.trigger_cursor = self.trigger.chars().count();
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_trigger_char(ch);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_content_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Enter, KeyModifiers::NONE) => {
                self.insert_content_char('\n');
                LibraryInteraction::handled()
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.backspace_content();
                LibraryInteraction::handled()
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.delete_content();
                LibraryInteraction::handled()
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.move_content_horizontal(-1);
                LibraryInteraction::handled()
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.move_content_horizontal(1);
                LibraryInteraction::handled()
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_content_vertical(-1);
                LibraryInteraction::handled()
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_content_vertical(1);
                LibraryInteraction::handled()
            }
            (KeyCode::PageUp, KeyModifiers::NONE) => {
                self.move_content_vertical(-5);
                LibraryInteraction::handled()
            }
            (KeyCode::PageDown, KeyModifiers::NONE) => {
                self.move_content_vertical(5);
                LibraryInteraction::handled()
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.move_content_to_line_edge(true);
                LibraryInteraction::handled()
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.move_content_to_line_edge(false);
                LibraryInteraction::handled()
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_content_char(ch);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_kind_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                let options = LibraryKind::ALL
                    .iter()
                    .map(|k| k.label().to_string())
                    .collect();
                let selected = LibraryKind::ALL
                    .iter()
                    .position(|&k| k == self.kind)
                    .unwrap_or(0);
                self.selector = Some(LibrarySelectState {
                    title: "Select Kind",
                    options,
                    selected,
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_selector_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        let Some(selector) = self.selector.as_mut() else {
            return LibraryInteraction::handled();
        };

        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.selector = None;
                LibraryInteraction::handled()
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if self.focus == LibraryModalField::Kind {
                    let previous_kind = self.kind;
                    self.kind = LibraryKind::ALL[selector.selected];
                    if !previous_kind.is_script() && self.kind.is_script() {
                        self.initialize_script_defaults();
                    }
                    self.ensure_focus_visible();
                } else if self.focus == LibraryModalField::TargetOs {
                    self.target_os = SUPPORTED_TARGET_OS_VALUES[selector.selected].to_string();
                } else if self.focus == LibraryModalField::Language {
                    self.interpreter = SCRIPT_LANGUAGE_OPTIONS[selector.selected];
                } else if self.focus == LibraryModalField::Mode {
                    self.behavior = SCRIPT_MODE_OPTIONS[selector.selected];
                }
                self.selector = None;
                LibraryInteraction::handled()
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                selector.selected =
                    (selector.selected + 1).min(selector.options.len().saturating_sub(1));
                LibraryInteraction::handled()
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                selector.selected = selector.selected.saturating_sub(1);
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_target_os_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                let options = SUPPORTED_TARGET_OS_VALUES
                    .iter()
                    .map(|v| display_target_os(v).to_string())
                    .collect();
                let selected = SUPPORTED_TARGET_OS_VALUES
                    .iter()
                    .position(|&v| v == self.target_os)
                    .unwrap_or(0);
                self.selector = Some(LibrarySelectState {
                    title: "Select Target OS",
                    options,
                    selected,
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_language_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                let options = SCRIPT_LANGUAGE_OPTIONS
                    .iter()
                    .map(|value| interpreter_label(*value).to_string())
                    .collect();
                let selected = SCRIPT_LANGUAGE_OPTIONS
                    .iter()
                    .position(|value| *value == self.interpreter)
                    .unwrap_or(0);
                self.selector = Some(LibrarySelectState {
                    title: "Select Language",
                    options,
                    selected,
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn handle_mode_key(&mut self, key: KeyEvent) -> LibraryInteraction {
        match (key.code, key.modifiers) {
            (KeyCode::Char(' '), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                let options = SCRIPT_MODE_OPTIONS
                    .iter()
                    .map(|value| behavior_label(*value).to_string())
                    .collect();
                let selected = SCRIPT_MODE_OPTIONS
                    .iter()
                    .position(|value| *value == self.behavior)
                    .unwrap_or(0);
                self.selector = Some(LibrarySelectState {
                    title: "Select Mode",
                    options,
                    selected,
                });
                LibraryInteraction::handled()
            }
            _ => LibraryInteraction::handled(),
        }
    }

    fn insert_trigger_char(&mut self, ch: char) {
        let byte_index = char_index_to_byte_index(&self.trigger, self.trigger_cursor);
        self.trigger.insert(byte_index, ch);
        self.trigger_cursor += 1;
    }

    fn backspace_trigger(&mut self) {
        if self.trigger_cursor == 0 {
            return;
        }

        let end = char_index_to_byte_index(&self.trigger, self.trigger_cursor);
        let start = char_index_to_byte_index(&self.trigger, self.trigger_cursor - 1);
        self.trigger.drain(start..end);
        self.trigger_cursor -= 1;
    }

    fn delete_trigger(&mut self) {
        if self.trigger_cursor >= self.trigger.chars().count() {
            return;
        }

        let start = char_index_to_byte_index(&self.trigger, self.trigger_cursor);
        let end = char_index_to_byte_index(&self.trigger, self.trigger_cursor + 1);
        self.trigger.drain(start..end);
    }

    fn insert_content_char(&mut self, ch: char) {
        let byte_index = char_index_to_byte_index(&self.content, self.content_cursor);
        self.content.insert(byte_index, ch);
        self.content_cursor += 1;
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn backspace_content(&mut self) {
        if self.content_cursor == 0 {
            return;
        }

        let end = char_index_to_byte_index(&self.content, self.content_cursor);
        let start = char_index_to_byte_index(&self.content, self.content_cursor - 1);
        self.content.drain(start..end);
        self.content_cursor -= 1;
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn delete_content(&mut self) {
        if self.content_cursor >= self.content.chars().count() {
            return;
        }

        let start = char_index_to_byte_index(&self.content, self.content_cursor);
        let end = char_index_to_byte_index(&self.content, self.content_cursor + 1);
        self.content.drain(start..end);
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn move_content_horizontal(&mut self, delta: isize) {
        let max = self.content.chars().count() as isize;
        let next = (self.content_cursor as isize + delta).clamp(0, max);
        self.content_cursor = next as usize;
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn move_content_vertical(&mut self, delta_lines: isize) {
        let starts = line_start_positions(&self.content);
        let (line_index, column) = line_col_for_char_index(&self.content, self.content_cursor);
        let goal = self.content_cursor_goal.unwrap_or(column);
        let next_line = (line_index as isize + delta_lines)
            .clamp(0, starts.len().saturating_sub(1) as isize) as usize;
        self.content_cursor = char_index_for_line_col(&self.content, next_line, goal);
        self.content_cursor_goal = Some(goal);
        self.follow_content_cursor();
    }

    fn move_content_to_line_edge(&mut self, start: bool) {
        let (line_index, _) = line_col_for_char_index(&self.content, self.content_cursor);
        let target_column = if start {
            0
        } else {
            line_lengths(&self.content)
                .get(line_index)
                .copied()
                .unwrap_or_default()
        };
        self.content_cursor = char_index_for_line_col(&self.content, line_index, target_column);
        self.content_cursor_goal = None;
        self.follow_content_cursor();
    }

    fn current_content_line(&self) -> usize {
        line_col_for_char_index(&self.content, self.content_cursor).0
    }

    fn content_line_count(&self) -> usize {
        line_start_positions(&self.content).len().max(1)
    }

    fn follow_content_cursor(&mut self) {
        self.content_scroll = self.current_content_line().saturating_sub(2);
    }

    pub(crate) fn footer_text(&self) -> &'static str {
        if self.selector.is_some() {
            "j/k Move   ↑/↓ Move   Enter Save   Esc Cancel"
        } else if self.mode == LibraryEditorMode::Edit {
            LIBRARY_EDIT_MODAL_FOOTER
        } else {
            LIBRARY_CREATE_MODAL_FOOTER
        }
    }

    pub(crate) fn selector(&self) -> Option<&LibrarySelectState> {
        self.selector.as_ref()
    }

    pub(crate) fn visible_fields(&self) -> &'static [LibraryModalField] {
        if self.is_script_kind() {
            &SCRIPT_MODAL_FIELDS
        } else {
            &SNIPPET_MODAL_FIELDS
        }
    }

    fn advance_focus(&mut self, forward: bool) {
        let fields = self.visible_fields();
        let current_index = fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        let next_index = if forward {
            (current_index + 1) % fields.len()
        } else if current_index == 0 {
            fields.len().saturating_sub(1)
        } else {
            current_index - 1
        };
        self.focus = fields[next_index];
        self.content_cursor_goal = None;
        if self.focus == LibraryModalField::Content {
            self.follow_content_cursor();
        }
    }

    fn ensure_focus_visible(&mut self) {
        if self.visible_fields().contains(&self.focus) {
            return;
        }

        self.focus = LibraryModalField::TargetOs;
        self.content_cursor_goal = None;
    }

    fn initialize_script_defaults(&mut self) {
        self.interpreter = default_script_interpreter_for_target_os(&self.target_os);
        self.behavior = ScriptBehavior::Inline;
    }
}
