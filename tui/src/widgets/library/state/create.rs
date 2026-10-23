use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::db::TargetOs;
use taurine_core::db::crud::{ActionType, TriggerLimits, TriggerType, prepare_trigger_with_type};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

use crate::widgets::field::TextField;
use crate::widgets::library::actions::{PendingLibraryCreate, join_app_filters};
use crate::widgets::textarea::TextArea;

use super::HeaderMenuKind;

/// Focus order in the create modal. Interpreter and Behavior only
/// take focus for script actions; the properties rows always do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryCreateModalField {
    TriggerType,
    Trigger,
    Description,
    Content,
    Action,
    Interpreter,
    Behavior,
    Os,
    AutoCase,
    Tags,
    Allow,
    Block,
}

/// Outcome of a key handled by the create modal. Picker rows delegate
/// to the shared overlay menus with the draft attached, so the menus
/// hand the draft back instead of writing to the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CreateKeyOutcome {
    Handled,
    Close,
    OpenHeaderMenu(HeaderMenuKind),
    OpenTags,
    OpenAppFilter(crate::widgets::library::state::AppFilterSide),
}

/// In-memory new-trigger draft: every field of the form, typed text
/// included. Nothing reaches the database until submit builds the
/// pending create.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LibraryCreateModalState {
    trigger_type: TriggerType,
    trigger: TextField,
    description: TextField,
    content: TextArea,
    content_scroll: usize,
    action: ActionType,
    interpreter: ScriptInterpreter,
    behavior: ScriptBehavior,
    target_os: String,
    auto_case: bool,
    tags: Vec<String>,
    allow_apps: Vec<String>,
    block_apps: Vec<String>,
    focus: LibraryCreateModalField,
    error: Option<String>,
    split: f32,
    divider_hover: bool,
    divider_drag: bool,
}

/// Visible content rows in the modal's snippet box.
pub(crate) const CREATE_CONTENT_ROWS: usize = 4;

/// Default split matches the original half-and-half form.
pub(crate) const CREATE_DEFAULT_SPLIT: f32 = 0.5;
/// Keep both panes readable while dragging; the divider stays visible
/// and grabbable at either end.
pub(crate) const CREATE_MIN_SPLIT: f32 = 0.25;
pub(crate) const CREATE_MAX_SPLIT: f32 = 0.75;

impl LibraryCreateModalState {
    pub(crate) fn new() -> Self {
        Self {
            trigger_type: TriggerType::Word,
            trigger: TextField::new(""),
            description: TextField::new(""),
            content: TextArea::new(""),
            content_scroll: 0,
            action: ActionType::Text,
            interpreter: ScriptInterpreter::Bash,
            behavior: ScriptBehavior::Inline,
            target_os: TargetOs::All.to_db_str().to_string(),
            auto_case: false,
            tags: Vec::new(),
            allow_apps: Vec::new(),
            block_apps: Vec::new(),
            focus: LibraryCreateModalField::Trigger,
            error: None,
            split: CREATE_DEFAULT_SPLIT,
            divider_hover: false,
            divider_drag: false,
        }
    }

    pub(crate) const fn focus(&self) -> LibraryCreateModalField {
        self.focus
    }

    /// Left pane's share of the body width.
    pub(crate) const fn split(&self) -> f32 {
        self.split
    }

    pub(crate) const fn divider_hover(&self) -> bool {
        self.divider_hover
    }

    pub(crate) const fn divider_drag(&self) -> bool {
        self.divider_drag
    }

    pub(crate) fn set_divider_hover(&mut self, hover: bool) {
        self.divider_hover = hover;
    }

    pub(crate) fn set_divider_drag(&mut self, drag: bool) {
        self.divider_drag = drag;
        if drag {
            self.divider_hover = true;
        }
    }

    /// Resize from a drag column, clamped so neither pane collapses.
    pub(crate) fn set_split_from_column(&mut self, body_x: u16, body_width: u16, column: u16) {
        let span = body_width.max(1) as f32;
        self.split = ((column.saturating_sub(body_x)) as f32 / span)
            .clamp(CREATE_MIN_SPLIT, CREATE_MAX_SPLIT);
    }

    pub(crate) fn set_focus(&mut self, focus: LibraryCreateModalField) {
        // honey: hidden rows never take focus; the caller lands on
        // the nearest visible row instead.
        if self.visible_fields().contains(&focus) {
            self.focus = focus;
        }
    }

    #[cfg(test)]
    pub(crate) fn trigger_text(&self) -> &str {
        self.trigger.text()
    }

    pub(crate) fn trigger_field(&self) -> &TextField {
        &self.trigger
    }

    #[cfg(test)]
    pub(crate) fn description_text(&self) -> &str {
        self.description.text()
    }

    pub(crate) fn description_field(&self) -> &TextField {
        &self.description
    }

    #[cfg(test)]
    pub(crate) fn content_text(&self) -> String {
        self.content.text()
    }

    pub(crate) fn content(&self) -> &TextArea {
        &self.content
    }

    pub(crate) const fn content_scroll(&self) -> usize {
        self.content_scroll
    }

    #[cfg(test)]
    pub(crate) const fn trigger_type(&self) -> TriggerType {
        self.trigger_type
    }

    pub(crate) const fn is_text_action(&self) -> bool {
        matches!(self.action, ActionType::Text)
    }

    pub(crate) fn action_label(&self) -> &'static str {
        self.action.as_str()
    }

    pub(crate) fn type_label(&self) -> &'static str {
        self.trigger_type.as_db_str()
    }

    pub(crate) fn interpreter_label(&self) -> &'static str {
        self.interpreter.as_str()
    }

    pub(crate) fn behavior_label(&self) -> &'static str {
        self.behavior.as_str()
    }

    pub(crate) fn target_os(&self) -> &str {
        &self.target_os
    }

    #[cfg(test)]
    pub(crate) const fn auto_case(&self) -> bool {
        self.auto_case
    }

    pub(crate) fn tags(&self) -> &[String] {
        &self.tags
    }

    pub(crate) fn allow_apps(&self) -> &[String] {
        &self.allow_apps
    }

    pub(crate) fn block_apps(&self) -> &[String] {
        &self.block_apps
    }

    pub(crate) fn set_tags(&mut self, tags: Vec<String>) {
        self.tags = tags;
    }

    pub(crate) fn set_allow_apps(&mut self, apps: Vec<String>) {
        self.allow_apps = apps;
    }

    pub(crate) fn set_block_apps(&mut self, apps: Vec<String>) {
        self.block_apps = apps;
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub(crate) fn toggle_auto_case(&mut self) {
        self.auto_case = !self.auto_case;
        self.error = None;
    }

    /// View model for the shared properties pane. Draft values feed
    /// the same rows saved triggers use; usage stays hidden until the
    /// trigger is created.
    pub(crate) fn props_item(&self) -> super::LibraryTrigger {
        super::LibraryTrigger::props_draft(
            self.trigger.text().to_string(),
            self.target_os.clone(),
            self.tags.clone(),
            (!self.allow_apps.is_empty()).then(|| join_app_filters(&self.allow_apps)),
            (!self.block_apps.is_empty()).then(|| join_app_filters(&self.block_apps)),
            self.auto_case,
            !self.is_text_action(),
        )
    }

    pub(crate) fn toggle_action(&mut self) {
        self.cycle_action();
    }

    /// Focusable rows right now: script rows only for scripts; the
    /// properties rows always, in pane order.
    pub(crate) fn visible_fields(&self) -> Vec<LibraryCreateModalField> {
        use LibraryCreateModalField as Field;
        let mut fields = vec![
            Field::TriggerType,
            Field::Trigger,
            Field::Description,
            Field::Content,
            Field::Action,
        ];
        if !self.is_text_action() {
            fields.push(Field::Interpreter);
            fields.push(Field::Behavior);
        }
        fields.extend([
            Field::AutoCase,
            Field::Allow,
            Field::Block,
            Field::Os,
            Field::Tags,
        ]);
        fields
    }

    fn advance_focus(&mut self, forward: bool) {
        let fields = self.visible_fields();
        let Some(position) = fields.iter().position(|field| *field == self.focus) else {
            self.focus = LibraryCreateModalField::Trigger;
            return;
        };
        let delta = if forward {
            1
        } else {
            fields.len().saturating_sub(1)
        };
        self.focus = fields[(position + delta) % fields.len()];
    }

    /// Apply a shared overlay-menu pick to the draft. Unknown labels
    /// (like the voice-only type) leave the draft untouched.
    pub(crate) fn apply_header_pick(&mut self, kind: HeaderMenuKind, option: &str) {
        match kind {
            HeaderMenuKind::InvocationType => {
                if let Some(trigger_type) = TriggerType::parse_str(option) {
                    self.trigger_type = trigger_type;
                }
            }
            HeaderMenuKind::ActionType => {
                if let Some(action) = ActionType::parse_str(option) {
                    self.action = action;
                }
            }
            HeaderMenuKind::Platform => {
                if let Some(os) = TargetOs::parse_str(option) {
                    self.target_os = os.to_db_str().to_string();
                }
            }
            HeaderMenuKind::Interpreter => {
                if let Some(interpreter) = ScriptInterpreter::parse_str(option) {
                    self.interpreter = interpreter;
                }
            }
            HeaderMenuKind::Behavior => {
                if let Some(behavior) = ScriptBehavior::parse_str(option) {
                    self.behavior = behavior;
                }
            }
        }
        self.error = None;
    }

    fn cycle_action(&mut self) {
        self.action = if self.is_text_action() {
            ActionType::Script
        } else {
            ActionType::Text
        };
        self.error = None;
    }

    fn cycle_field(&mut self, forward: bool) {
        match self.focus {
            LibraryCreateModalField::TriggerType => {
                let order = TriggerType::ALL;
                let position = order
                    .iter()
                    .position(|item| *item == self.trigger_type)
                    .unwrap_or(0);
                let delta = if forward {
                    1
                } else {
                    order.len().saturating_sub(1)
                };
                self.trigger_type = order[(position + delta) % order.len()];
                self.error = None;
            }
            LibraryCreateModalField::Action => self.cycle_action(),
            LibraryCreateModalField::Os => {
                let order = TargetOs::ALL;
                let position = order
                    .iter()
                    .position(|item| item.to_db_str() == self.target_os)
                    .unwrap_or(0);
                let delta = if forward {
                    1
                } else {
                    order.len().saturating_sub(1)
                };
                self.target_os = order[(position + delta) % order.len()]
                    .to_db_str()
                    .to_string();
                self.error = None;
            }
            LibraryCreateModalField::Interpreter => {
                let order = ScriptInterpreter::ALL;
                let position = order
                    .iter()
                    .position(|item| *item == self.interpreter)
                    .unwrap_or(0);
                let delta = if forward {
                    1
                } else {
                    order.len().saturating_sub(1)
                };
                self.interpreter = order[(position + delta) % order.len()];
                self.error = None;
            }
            LibraryCreateModalField::Behavior => {
                let order = ScriptBehavior::ALL;
                let position = order
                    .iter()
                    .position(|item| *item == self.behavior)
                    .unwrap_or(0);
                let delta = if forward {
                    1
                } else {
                    order.len().saturating_sub(1)
                };
                self.behavior = order[(position + delta) % order.len()];
                self.error = None;
            }
            _ => {}
        }
    }

    fn follow_content_caret(&mut self) {
        let (row, _) = self.content.cursor();
        if row < self.content_scroll {
            self.content_scroll = row;
        } else if row >= self.content_scroll.saturating_add(CREATE_CONTENT_ROWS) {
            self.content_scroll = row.saturating_sub(CREATE_CONTENT_ROWS - 1);
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CreateKeyOutcome {
        self.error = None;

        if self.focus == LibraryCreateModalField::Content {
            return self.handle_content_key(key);
        }

        // honey: j/k navigate only outside the single-line editors, so
        // both text fields can still type those letters.
        let text_focus = matches!(
            self.focus,
            LibraryCreateModalField::Trigger | LibraryCreateModalField::Description
        );
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => CreateKeyOutcome::Close,
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Tab, KeyModifiers::NONE) => {
                self.advance_focus(true);
                CreateKeyOutcome::Handled
            }
            (KeyCode::Char('j'), KeyModifiers::NONE) if !text_focus => {
                self.advance_focus(true);
                CreateKeyOutcome::Handled
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::BackTab, _) => {
                self.advance_focus(false);
                CreateKeyOutcome::Handled
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) if !text_focus => {
                self.advance_focus(false);
                CreateKeyOutcome::Handled
            }
            _ => self.handle_focused_key(key),
        }
    }

    fn handle_focused_key(&mut self, key: KeyEvent) -> CreateKeyOutcome {
        match self.focus {
            LibraryCreateModalField::Trigger | LibraryCreateModalField::Description => {
                self.handle_text_key(key)
            }
            LibraryCreateModalField::TriggerType => {
                self.handle_picker_key(key, HeaderMenuKind::InvocationType)
            }
            LibraryCreateModalField::Os => self.handle_picker_key(key, HeaderMenuKind::Platform),
            LibraryCreateModalField::Interpreter => {
                self.handle_picker_key(key, HeaderMenuKind::Interpreter)
            }
            LibraryCreateModalField::Behavior => {
                self.handle_picker_key(key, HeaderMenuKind::Behavior)
            }
            LibraryCreateModalField::Action => match (key.code, key.modifiers) {
                (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => {
                    CreateKeyOutcome::OpenHeaderMenu(HeaderMenuKind::ActionType)
                }
                (KeyCode::Left | KeyCode::Right, KeyModifiers::NONE) => {
                    self.toggle_action();
                    CreateKeyOutcome::Handled
                }
                _ => CreateKeyOutcome::Handled,
            },
            LibraryCreateModalField::AutoCase => match (key.code, key.modifiers) {
                (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE)
                | (KeyCode::Left | KeyCode::Char('h'), KeyModifiers::NONE)
                | (KeyCode::Right | KeyCode::Char('l'), KeyModifiers::NONE) => {
                    self.toggle_auto_case();
                    CreateKeyOutcome::Handled
                }
                _ => CreateKeyOutcome::Handled,
            },
            LibraryCreateModalField::Tags => match (key.code, key.modifiers) {
                (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => {
                    CreateKeyOutcome::OpenTags
                }
                _ => CreateKeyOutcome::Handled,
            },
            LibraryCreateModalField::Allow => match (key.code, key.modifiers) {
                (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => {
                    CreateKeyOutcome::OpenAppFilter(
                        crate::widgets::library::state::AppFilterSide::Allow,
                    )
                }
                _ => CreateKeyOutcome::Handled,
            },
            LibraryCreateModalField::Block => match (key.code, key.modifiers) {
                (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => {
                    CreateKeyOutcome::OpenAppFilter(
                        crate::widgets::library::state::AppFilterSide::Block,
                    )
                }
                _ => CreateKeyOutcome::Handled,
            },
            LibraryCreateModalField::Content => CreateKeyOutcome::Handled,
        }
    }

    /// Picker rows: arrows cycle inline, Enter opens the shared
    /// overlay menu with this draft attached for the return trip.
    fn handle_picker_key(&mut self, key: KeyEvent, kind: HeaderMenuKind) -> CreateKeyOutcome {
        match (key.code, key.modifiers) {
            (KeyCode::Enter | KeyCode::Char(' '), KeyModifiers::NONE) => {
                CreateKeyOutcome::OpenHeaderMenu(kind)
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.cycle_field(false);
                CreateKeyOutcome::Handled
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.cycle_field(true);
                CreateKeyOutcome::Handled
            }
            _ => CreateKeyOutcome::Handled,
        }
    }

    fn handle_text_key(&mut self, key: KeyEvent) -> CreateKeyOutcome {
        // honey: Enter advances instead of submitting so a stray key
        // never creates a trigger; the submit affordance returns later.
        let field = match self.focus {
            LibraryCreateModalField::Trigger => &mut self.trigger,
            LibraryCreateModalField::Description => &mut self.description,
            _ => return CreateKeyOutcome::Handled,
        };
        match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) => {
                field.move_left();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                field.move_right();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                field.move_home();
                CreateKeyOutcome::Handled
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                field.move_end();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                field.backspace();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                field.delete_at();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                self.advance_focus(true);
                CreateKeyOutcome::Handled
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                field.insert(ch);
                CreateKeyOutcome::Handled
            }
            _ => CreateKeyOutcome::Handled,
        }
    }

    fn handle_content_key(&mut self, key: KeyEvent) -> CreateKeyOutcome {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => CreateKeyOutcome::Close,
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.advance_focus(true);
                CreateKeyOutcome::Handled
            }
            (KeyCode::BackTab, _) => {
                self.advance_focus(false);
                CreateKeyOutcome::Handled
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                self.content.insert_newline();
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.content.backspace();
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                self.content.delete_at();
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.content.move_left(false);
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.content.move_right(false);
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.content.move_up(false);
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.content.move_down(false);
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.content.move_home(false);
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.content.move_end(false);
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.content.insert_char(ch);
                self.follow_content_caret();
                CreateKeyOutcome::Handled
            }
            _ => CreateKeyOutcome::Handled,
        }
    }

    // honey: the create action is temporarily buttonless; keep the
    // pending builder ready for the submit affordance that replaces it.
    #[allow(dead_code)]
    pub(crate) fn build_pending_create(&self) -> taurine_core::Result<PendingLibraryCreate> {
        // honey: core validates the trigger rules and canonicalizes
        // hotkeys; the stored form is what gets created. No database
        // is touched here, so conflicts surface at apply time with
        // the draft still open.
        let prepared =
            prepare_trigger_with_type(self.trigger.text(), self.trigger_type, &self.target_os)?;
        let description = self.description.text().trim();
        let description = (!description.is_empty()).then(|| description.to_string());
        TriggerLimits::validate_description(description.as_deref())?;
        let (interpreter, behavior) = if self.is_text_action() {
            (None, None)
        } else {
            (Some(self.interpreter), Some(self.behavior))
        };
        Ok(PendingLibraryCreate {
            trigger: prepared.stored_trigger,
            trigger_type: self.trigger_type,
            description,
            content: self.content.text(),
            action_type: self.action.as_str().to_string(),
            target_os: self.target_os.clone(),
            tags: self.tags.clone(),
            allow_apps: self.allow_apps.clone(),
            block_apps: self.block_apps.clone(),
            auto_case: self.auto_case,
            interpreter,
            behavior,
        })
    }
}
