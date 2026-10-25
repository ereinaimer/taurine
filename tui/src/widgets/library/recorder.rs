//! Full-TKL virtual keyboard overlay for recording a hotkey chord.
//!
//! The overlay is a flat, borderless popup: an instruction header with
//! Cancel/Done chips and a six-row keyboard. The matrix and its geometry
//! live in [`matrix`] so rendering and hit-testing share one source of
//! truth.

mod matrix;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ModifierKeyCode};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier as StyleModifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use taurine_core::keys::{Modifier, ModifierFamily, ModifierSide};

pub(crate) use matrix::{Layout, layout, popup};

use crate::theme::Theme;
use crate::widgets::toggle::ENABLED_KNOB;
use crate::widgets::util::{self, OverlayChrome};

/// Dark label on the active green fill, matching the toggle's ON state.
const ACTIVE_TEXT: Color = Color::Rgb(0x0C, 0x0C, 0x0C);

/// Where a committed chord lands: the selected row's own invocation or
/// one of its sibling aliases.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum HotkeyTarget {
    #[default]
    PrimaryTrigger,
    AliasIndex(usize),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HotkeyRecorderState {
    pub(crate) target: HotkeyTarget,
    /// Active modifiers in canonical order.
    pub(crate) modifiers: Vec<String>,
    pub(crate) main_key: Option<String>,
    pub(crate) hovered_key: Option<String>,
    pub(crate) keycap_hitboxes: Vec<(String, Rect)>,
    pub(crate) cancel_hitbox: Option<Rect>,
    pub(crate) done_hitbox: Option<Rect>,
    pub(crate) error: Option<String>,
}

/// What a key or click asked the caller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecorderAction {
    None,
    Cancel,
    Done,
}

impl HotkeyRecorderState {
    pub(crate) fn new(target: HotkeyTarget) -> Self {
        Self {
            target,
            ..Self::default()
        }
    }

    /// Canonical chord string, or `None` while no non-modifier key is set.
    pub(crate) fn chord(&self) -> Option<String> {
        let mut parts: Vec<String> = self.modifiers.iter().map(|id| chord_part(id)).collect();
        parts.sort_by_key(|part| modifier_rank(part));
        parts.push(chord_part(self.main_key.as_deref()?));
        Some(parts.join("+"))
    }

    /// Refresh the cached hitboxes from the same geometry the render
    /// path uses. Called once per mouse event.
    pub(crate) fn refresh_hitboxes(&mut self, area: Rect) {
        let layout = layout(area);
        self.keycap_hitboxes = layout
            .keycaps
            .iter()
            .map(|(id, _, rect)| ((*id).to_string(), *rect))
            .collect();
        self.cancel_hitbox = Some(layout.cancel);
        self.done_hitbox = Some(layout.done);
    }

    pub(crate) fn hover(&mut self, area: Rect, column: u16, row: u16) {
        self.refresh_hitboxes(area);
        self.hovered_key = self
            .keycap_hitboxes
            .iter()
            .find(|(_, rect)| crate::terminal::mouse::contains(*rect, column, row))
            .map(|(id, _)| id.clone());
    }

    pub(crate) fn click(&mut self, area: Rect, column: u16, row: u16) -> RecorderAction {
        self.refresh_hitboxes(area);
        if self
            .cancel_hitbox
            .is_some_and(|rect| crate::terminal::mouse::contains(rect, column, row))
        {
            return RecorderAction::Cancel;
        }
        if self
            .done_hitbox
            .is_some_and(|rect| crate::terminal::mouse::contains(rect, column, row))
        {
            return RecorderAction::Done;
        }
        let hit = self
            .keycap_hitboxes
            .iter()
            .find(|(_, rect)| crate::terminal::mouse::contains(*rect, column, row))
            .map(|(id, _)| id.clone());
        if let Some(id) = hit {
            self.press_keycap(&id);
        }
        RecorderAction::None
    }

    fn press_keycap(&mut self, id: &str) {
        if is_modifier(id) {
            self.toggle_modifier(id);
            return;
        }
        if id == "menu" {
            self.error = Some(MENU_ERROR.to_string());
            return;
        }
        self.error = None;
        self.main_key = if self.main_key.as_deref() == Some(id) {
            None
        } else {
            Some(id.to_string())
        };
    }

    fn toggle_modifier(&mut self, id: &str) {
        self.error = None;
        if let Some(position) = self.modifiers.iter().position(|held| held == id) {
            self.modifiers.remove(position);
            return;
        }
        self.add_modifier(id);
    }

    /// One modifier per family: picking a side replaces the other side or
    /// a generic press of the same family.
    fn add_modifier(&mut self, id: &str) {
        if let Some(family) = Modifier::from_alias(id).map(Modifier::family) {
            self.modifiers
                .retain(|held| Modifier::from_alias(held).map(Modifier::family) != Some(family));
        }
        self.modifiers.push(id.to_string());
        self.modifiers.sort_by_key(|held| modifier_rank(held));
    }

    /// Physical keyboard while recording.
    pub(crate) fn press(&mut self, key: KeyEvent) -> RecorderAction {
        if key.kind != KeyEventKind::Press {
            return RecorderAction::None;
        }
        // honey: kitty-protocol terminals report which side a modifier came
        // from; Windows terminals only report the family.
        if let KeyCode::Modifier(modifier) = key.code
            && let Some(id) = sided_modifier_id(modifier)
        {
            self.error = None;
            self.add_modifier(id);
            return RecorderAction::None;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => return RecorderAction::Cancel,
            (KeyCode::Enter, KeyModifiers::NONE) => return RecorderAction::Done,
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.modifiers.clear();
                self.main_key = None;
                self.error = None;
                return RecorderAction::None;
            }
            _ => {}
        }
        let Some((id, implied_shift)) = key_id(key.code) else {
            return RecorderAction::None;
        };
        let mut physical = physical_modifiers(key.modifiers);
        if implied_shift && !physical.iter().any(|modifier| modifier == "shift") {
            physical.push("shift".to_string());
        }
        if !physical.is_empty() {
            self.modifiers = physical
                .into_iter()
                .map(|modifier| self.keep_recorded_side(modifier))
                .collect();
            self.modifiers.sort_by_key(|held| modifier_rank(held));
        }
        self.error = None;
        if id == "menu" {
            self.error = Some(MENU_ERROR.to_string());
            return RecorderAction::None;
        }
        self.main_key = Some(id);
        RecorderAction::None
    }

    /// A side recorded by a click or a modifier-key event survives a
    /// generic physical press of the same family.
    fn keep_recorded_side(&self, generic: String) -> String {
        let Some(family) = Modifier::from_alias(&generic).map(Modifier::family) else {
            return generic;
        };
        self.modifiers
            .iter()
            .find(|held| {
                Modifier::from_alias(held).is_some_and(|modifier| {
                    modifier.family() == family && modifier.side().is_some()
                })
            })
            .cloned()
            .unwrap_or(generic)
    }

    /// Chord to save, or a visible nudge when nothing is recorded yet.
    pub(crate) fn commit(&mut self) -> Option<String> {
        match self.chord() {
            Some(chord) => Some(chord),
            None => {
                self.error = Some("Press a key before saving.".to_string());
                None
            }
        }
    }
}

const MENU_ERROR: &str = "The Menu key can't be used in a hotkey.";

pub(crate) fn render(frame: &mut Frame, area: Rect, theme: &Theme, state: &HotkeyRecorderState) {
    let chrome = OverlayChrome::new(popup(area));
    chrome.render(frame, theme, "Record hotkey");
    let layout = layout(area);
    if layout.content.width == 0 || layout.content.height == 0 {
        return;
    }
    render_instructions(frame, theme, &layout, state);
    render_chips(frame, theme, &layout);
    render_divider(frame, theme, layout.content, layout.divider_top);
    render_keycaps(frame, theme, &layout, state);
}

fn render_instructions(
    frame: &mut Frame,
    theme: &Theme,
    layout: &Layout,
    state: &HotkeyRecorderState,
) {
    let second = match &state.error {
        Some(error) => Span::styled(error.clone(), Style::default().fg(theme.error)),
        None => Span::styled(
            "click on the keycaps below",
            Style::default().fg(theme.text_muted),
        ),
    };
    let lines = vec![
        Line::from(Span::styled(
            "Press keys on your keyboard or",
            Style::default().fg(theme.text_muted),
        )),
        Line::from(second),
    ];
    frame.render_widget(Paragraph::new(lines), layout.instructions);
}

fn render_chips(frame: &mut Frame, theme: &Theme, layout: &Layout) {
    let cancel_bg = theme.keycap.bg;
    let cancel = vec![
        Span::styled(" Esc ", Style::default().fg(theme.text_muted).bg(cancel_bg)),
        Span::styled("Cancel ", Style::default().fg(theme.text).bg(cancel_bg)),
    ];
    render_chip(frame, layout.cancel, cancel_bg, cancel);
    let done = vec![Span::styled(
        " ✓ Done ",
        Style::default()
            .fg(ACTIVE_TEXT)
            .bg(ENABLED_KNOB)
            .add_modifier(StyleModifier::BOLD),
    )];
    render_chip(frame, layout.done, ENABLED_KNOB, done);
}

fn render_chip(frame: &mut Frame, rect: Rect, bg: Color, spans: Vec<Span<'static>>) {
    if rect.width == 0 {
        return;
    }
    frame.render_widget(Block::default().style(Style::default().bg(bg)), rect);
    frame.render_widget(Paragraph::new(Line::from(spans)), rect);
}

fn render_divider(frame: &mut Frame, theme: &Theme, content: Rect, y: u16) {
    if content.width == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("─".repeat(content.width as usize)).style(Style::default().fg(theme.border)),
        Rect::new(content.x, y, content.width, 1),
    );
}

fn render_keycaps(frame: &mut Frame, theme: &Theme, layout: &Layout, state: &HotkeyRecorderState) {
    for (id, label, rect) in &layout.keycaps {
        if rect.width == 0 || rect.height == 0 {
            continue;
        }
        let active = keycap_active(state, id);
        let hovered = state.hovered_key.as_deref() == Some(*id);
        let (bg, fg, bold) = if active {
            (ENABLED_KNOB, ACTIVE_TEXT, true)
        } else if hovered {
            (theme.keycap.on_surface, theme.text, false)
        } else {
            (theme.surface, theme.text, false)
        };
        frame.render_widget(Block::default().style(Style::default().bg(bg)), *rect);
        if label.is_empty() {
            continue;
        }
        let mut style = Style::default().fg(fg).bg(bg);
        if bold {
            style = style.add_modifier(StyleModifier::BOLD);
        }
        let text = keycap_text(label, rect.width);
        frame.render_widget(
            Paragraph::new(Span::styled(
                util::truncate_to_width(&text, rect.width),
                style,
            ))
            .alignment(Alignment::Center),
            Rect::new(
                rect.x,
                rect.y.saturating_add(rect.height / 2),
                rect.width,
                1,
            ),
        );
    }
}

/// Word keys breathe with a blank cell on each side; letters, digits, and
/// symbols stay tight. Padding only lands when the stretched key is wide
/// enough to hold it.
fn keycap_text(label: &str, width: u16) -> String {
    if label.chars().count() > 1 {
        let padded = format!(" {label} ");
        if padded.chars().count() as u16 <= width {
            return padded;
        }
    }
    label.to_string()
}

/// True when the keycap should fill with the active green. An exact
/// sided modifier lights only its own cap; a generic physical modifier
/// carries no side, so it lights both caps of the family.
fn keycap_active(state: &HotkeyRecorderState, id: &str) -> bool {
    if state.modifiers.iter().any(|held| held == id) || state.main_key.as_deref() == Some(id) {
        return true;
    }
    let Some(keycap) = Modifier::from_alias(id) else {
        return false;
    };
    keycap.side().is_some()
        && state.modifiers.iter().any(|held| {
            Modifier::from_alias(held).is_some_and(|modifier| {
                modifier.family() == keycap.family() && modifier.side().is_none()
            })
        })
}

fn is_modifier(id: &str) -> bool {
    Modifier::from_alias(id).is_some()
}

/// Canonical chord order: family first, then the generic/sided variant.
fn modifier_rank(id: &str) -> (u8, u8) {
    let Some(modifier) = Modifier::from_alias(id) else {
        return (u8::MAX, 0);
    };
    let family = match modifier.family() {
        ModifierFamily::Ctrl => 0,
        ModifierFamily::Shift => 1,
        ModifierFamily::Alt => 2,
        ModifierFamily::Meta => 3,
    };
    let side = match modifier.side() {
        None => 0,
        Some(ModifierSide::Left) => 1,
        Some(ModifierSide::Right) => 2,
    };
    (family, side)
}

/// Keycap id to the name core stores: sided modifiers keep their side,
/// and abbreviated caps expand to their long names.
fn chord_part(id: &str) -> String {
    Modifier::from_alias(id)
        .map(|modifier| modifier.canonical_name().to_string())
        .unwrap_or_else(|| core_alias(id).to_string())
}

/// Which side a kitty-protocol modifier event came from.
fn sided_modifier_id(modifier: ModifierKeyCode) -> Option<&'static str> {
    Some(match modifier {
        ModifierKeyCode::LeftControl => "lctrl",
        ModifierKeyCode::RightControl => "rctrl",
        ModifierKeyCode::LeftShift => "lshift",
        ModifierKeyCode::RightShift => "rshift",
        ModifierKeyCode::LeftAlt => "lalt",
        ModifierKeyCode::RightAlt => "ralt",
        ModifierKeyCode::LeftMeta | ModifierKeyCode::LeftSuper => "lmeta",
        ModifierKeyCode::RightMeta | ModifierKeyCode::RightSuper => "rmeta",
        _ => return None,
    })
}

fn physical_modifiers(modifiers: KeyModifiers) -> Vec<String> {
    let mut held = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        held.push("ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        held.push("shift".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        held.push("alt".to_string());
    }
    if modifiers.intersects(KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META) {
        held.push("meta".to_string());
    }
    held
}

/// Terminal key to a virtual keycap id, plus whether the character only
/// exists behind Shift (so a terminal that drops the modifier still
/// records `shift`).
fn key_id(code: KeyCode) -> Option<(String, bool)> {
    let id = match code {
        KeyCode::Char(character) => return char_key(character),
        KeyCode::Enter => "enter",
        KeyCode::Esc => "esc",
        KeyCode::Tab => "tab",
        KeyCode::Backspace => "bksp",
        KeyCode::Up => "up",
        KeyCode::Down => "down",
        KeyCode::Left => "left",
        KeyCode::Right => "right",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::PageUp => "pgup",
        KeyCode::PageDown => "pgdn",
        KeyCode::Insert => "ins",
        KeyCode::Delete => "del",
        KeyCode::CapsLock => "caps",
        KeyCode::ScrollLock => "scrlk",
        KeyCode::PrintScreen => "prtsc",
        KeyCode::Pause => "pause",
        KeyCode::Menu => "menu",
        KeyCode::F(number) if (1..=12).contains(&number) => {
            return Some((format!("f{number}"), false));
        }
        _ => return None,
    };
    Some((id.to_string(), false))
}

fn char_key(character: char) -> Option<(String, bool)> {
    if character.is_ascii_lowercase() || character.is_ascii_digit() {
        return Some((character.to_string(), false));
    }
    if character.is_ascii_uppercase() {
        return Some((character.to_ascii_lowercase().to_string(), true));
    }
    let base = match character {
        ' ' => return Some(("space".to_string(), false)),
        '`' | '-' | '=' | '[' | ']' | '\\' | ';' | '\'' | ',' | '.' | '/' => {
            return Some((character.to_string(), false));
        }
        '~' => '`',
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        '_' => '-',
        '+' => '=',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        _ => return None,
    };
    Some((base.to_string(), true))
}

/// Core's canonical name for a keycap id. Only the abbreviated caps the
/// keyboard draws need translating.
pub(crate) fn core_alias(id: &str) -> &str {
    match id {
        "bksp" => "backspace",
        "caps" => "capslock",
        "scrlk" => "scrolllock",
        "del" => "delete",
        "ins" => "insert",
        "pgdn" => "pgdown",
        "prtsc" => "printscreen",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::default::DARK_THEME;
    use ratatui::{Terminal, backend::TestBackend};

    fn press(code: KeyCode, modifiers: KeyModifiers) -> RecorderAction {
        HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger).press(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn chord_orders_modifiers_canonically() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.modifiers = vec!["shift".to_string(), "meta".to_string(), "ctrl".to_string()];
        state.main_key = Some("a".to_string());
        assert_eq!(state.chord().as_deref(), Some("ctrl+shift+meta+a"));
    }

    #[test]
    fn chord_keeps_modifier_sides() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.modifiers = vec!["rshift".to_string(), "lctrl".to_string()];
        state.main_key = Some("a".to_string());
        assert_eq!(state.chord().as_deref(), Some("lctrl+rshift+a"));
    }

    #[test]
    fn chord_maps_win_aliases_to_meta() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.modifiers = vec!["lwin".to_string()];
        state.main_key = Some("a".to_string());
        assert_eq!(state.chord().as_deref(), Some("lmeta+a"));
    }

    #[test]
    fn chord_needs_a_main_key() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.modifiers = vec!["alt".to_string()];
        assert_eq!(state.chord(), None);
        assert_eq!(state.commit(), None);
        assert!(state.error.is_some());
    }

    #[test]
    fn physical_press_records_modifiers_and_key() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.press(KeyEvent::new(KeyCode::F(6), KeyModifiers::ALT));
        assert_eq!(state.chord().as_deref(), Some("alt+f6"));
    }

    #[test]
    fn modifier_key_event_records_its_side() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.press(KeyEvent::new(
            KeyCode::Modifier(ModifierKeyCode::LeftControl),
            KeyModifiers::CONTROL,
        ));
        state.press(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(state.chord().as_deref(), Some("lctrl+a"));

        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.press(KeyEvent::new(
            KeyCode::Modifier(ModifierKeyCode::RightShift),
            KeyModifiers::SHIFT,
        ));
        state.press(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        assert_eq!(state.chord().as_deref(), Some("rshift+a"));
    }

    #[test]
    fn clicked_side_survives_a_generic_physical_press() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.modifiers = vec!["lctrl".to_string()];
        state.press(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(state.chord().as_deref(), Some("lctrl+a"));
    }

    #[test]
    fn shifted_punctuation_keeps_shift() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.press(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        assert_eq!(state.chord().as_deref(), Some("shift+;"));
    }

    #[test]
    fn release_and_repeat_are_ignored() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        let mut release = KeyEvent::new(KeyCode::F(6), KeyModifiers::ALT);
        release.kind = KeyEventKind::Release;
        state.press(release);
        assert_eq!(state.chord(), None);
    }

    #[test]
    fn standalone_esc_cancels() {
        assert_eq!(
            press(KeyCode::Esc, KeyModifiers::NONE),
            RecorderAction::Cancel
        );
        // With a modifier it records the key instead.
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.press(KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT));
        assert_eq!(state.chord().as_deref(), Some("alt+esc"));
    }

    #[test]
    fn standalone_enter_commits() {
        assert_eq!(
            press(KeyCode::Enter, KeyModifiers::NONE),
            RecorderAction::Done
        );
    }

    #[test]
    fn standalone_backspace_clears() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.press(KeyEvent::new(KeyCode::F(6), KeyModifiers::ALT));
        state.press(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(state.modifiers.is_empty());
        assert_eq!(state.main_key, None);
    }

    #[test]
    fn backspace_with_a_modifier_records_the_key() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.press(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL));
        assert_eq!(state.chord().as_deref(), Some("ctrl+backspace"));
    }

    #[test]
    fn clicking_a_modifier_toggles_it() {
        let area = Rect::new(0, 0, 100, 45);
        let layout = layout(area);
        let (_, _, rect) = layout
            .keycaps
            .iter()
            .find(|(id, _, _)| *id == "lctrl")
            .expect("left ctrl keycap");
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.click(area, rect.x + 1, rect.y);
        assert_eq!(state.modifiers, vec!["lctrl".to_string()]);
        state.click(area, rect.x + 1, rect.y);
        assert!(state.modifiers.is_empty());
    }

    #[test]
    fn each_modifier_side_registers_on_its_own() {
        let area = Rect::new(0, 0, 100, 45);
        let layout = layout(area);
        let click = |state: &mut HotkeyRecorderState, id: &str| {
            let (_, _, rect) = layout
                .keycaps
                .iter()
                .find(|(key, _, _)| *key == id)
                .unwrap_or_else(|| panic!("{id} keycap"));
            state.click(area, rect.x + 1, rect.y);
        };
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        click(&mut state, "lshift");
        assert_eq!(state.modifiers, vec!["lshift".to_string()]);
        click(&mut state, "rshift");
        assert_eq!(state.modifiers, vec!["rshift".to_string()]);
        click(&mut state, "rshift");
        assert!(state.modifiers.is_empty());
    }

    #[test]
    fn clicking_the_menu_key_explains_itself() {
        let area = Rect::new(0, 0, 100, 45);
        let layout = layout(area);
        let (_, _, rect) = layout
            .keycaps
            .iter()
            .find(|(id, _, _)| *id == "menu")
            .expect("menu keycap");
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.click(area, rect.x, rect.y);
        assert_eq!(state.main_key, None);
        assert!(state.error.is_some());
    }

    #[test]
    fn chips_discard_and_commit() {
        let area = Rect::new(0, 0, 100, 45);
        let layout = layout(area);
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        assert_eq!(
            state.click(area, layout.cancel.x + 1, layout.cancel.y),
            RecorderAction::Cancel
        );
        assert_eq!(
            state.click(area, layout.done.x + 1, layout.done.y),
            RecorderAction::Done
        );
    }

    #[test]
    fn hover_tracks_the_keycap_under_the_pointer() {
        let area = Rect::new(0, 0, 100, 45);
        let layout = layout(area);
        let (_, _, rect) = layout
            .keycaps
            .iter()
            .find(|(id, _, _)| *id == "f6")
            .expect("f6 keycap");
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.hover(area, rect.x, rect.y);
        assert_eq!(state.hovered_key.as_deref(), Some("f6"));
        state.hover(area, 0, 0);
        assert_eq!(state.hovered_key, None);
    }

    fn draw(state: &HotkeyRecorderState) -> (Terminal<TestBackend>, Rect) {
        let area = Rect::new(0, 0, 100, 45);
        let mut terminal =
            Terminal::new(TestBackend::new(area.width, area.height)).expect("test terminal");
        terminal
            .draw(|frame| render(frame, area, &DARK_THEME, state))
            .expect("test draw");
        (terminal, area)
    }

    fn bg_at(terminal: &Terminal<TestBackend>, area: Rect, x: u16, y: u16) -> Color {
        terminal.backend().buffer().content[(y * area.width + x) as usize].bg
    }

    #[test]
    fn generic_modifier_lights_both_side_caps() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.modifiers = vec!["shift".to_string()];
        state.main_key = Some("f6".to_string());
        let (terminal, area) = draw(&state);
        let layout = layout(area);
        for id in ["lshift", "rshift", "f6"] {
            let (_, _, rect) = layout
                .keycaps
                .iter()
                .find(|(key, _, _)| *key == id)
                .expect(id);
            assert_eq!(bg_at(&terminal, area, rect.x, rect.y), ENABLED_KNOB, "{id}");
        }
    }

    #[test]
    fn sided_modifier_lights_only_its_own_cap() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.modifiers = vec!["lshift".to_string()];
        let (terminal, area) = draw(&state);
        let layout = layout(area);
        let rect_of = |id: &str| {
            let (_, _, rect) = layout
                .keycaps
                .iter()
                .find(|(key, _, _)| *key == id)
                .expect(id);
            *rect
        };
        assert_eq!(
            bg_at(&terminal, area, rect_of("lshift").x, rect_of("lshift").y),
            ENABLED_KNOB
        );
        assert_eq!(
            bg_at(&terminal, area, rect_of("rshift").x, rect_of("rshift").y),
            DARK_THEME.surface
        );
    }

    #[test]
    fn idle_keys_use_the_surface_and_hover_lifts_them() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.hovered_key = Some("f6".to_string());
        let (terminal, area) = draw(&state);
        let layout = layout(area);
        let rect_of = |id: &str| {
            layout
                .keycaps
                .iter()
                .find(|(key, _, _)| *key == id)
                .map(|(_, _, rect)| *rect)
                .expect(id)
        };
        assert_eq!(
            bg_at(&terminal, area, rect_of("f6").x, rect_of("f6").y),
            DARK_THEME.keycap.on_surface
        );
        assert_eq!(
            bg_at(&terminal, area, rect_of("f1").x, rect_of("f1").y),
            DARK_THEME.surface
        );
    }

    #[test]
    fn done_chip_is_the_toggle_green() {
        let state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        let (terminal, area) = draw(&state);
        let layout = layout(area);
        assert_eq!(
            bg_at(&terminal, area, layout.done.x, layout.done.y),
            ENABLED_KNOB
        );
        assert_eq!(
            bg_at(&terminal, area, layout.cancel.x, layout.cancel.y),
            DARK_THEME.keycap.bg
        );
    }

    #[test]
    fn word_keys_get_breathing_room_but_symbols_do_not() {
        assert_eq!(keycap_text("Esc", 5), " Esc ");
        assert_eq!(keycap_text("Shift", 7), " Shift ");
        // Too tight for padding: draw the bare label instead of clipping.
        assert_eq!(keycap_text("Esc", 3), "Esc");
        assert_eq!(keycap_text("A", 5), "A");
        assert_eq!(keycap_text("↑", 5), "↑");
        assert_eq!(keycap_text("", 5), "");
    }
}
