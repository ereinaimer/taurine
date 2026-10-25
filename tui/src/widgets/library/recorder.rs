//! Full-TKL virtual keyboard overlay for recording a hotkey chord.
//!
//! The overlay is a flat, borderless popup: an instruction header with
//! Cancel/Done chips, a live chord preview, and a six-row keyboard. The
//! matrix and its geometry live in [`matrix`] so rendering and
//! hit-testing share one source of truth.

mod matrix;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier as StyleModifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use taurine_core::keys::{LogicalKey, Modifier, parse_hotkey};

pub(crate) use matrix::{Layout, layout, popup};

use crate::theme::Theme;
use crate::widgets::toggle::ENABLED_KNOB;
use crate::widgets::util::{self, OverlayChrome};

/// Dark label on the active green fill, matching the toggle's ON state.
const ACTIVE_TEXT: Color = Color::Rgb(0x0C, 0x0C, 0x0C);

/// Canonical modifier order used by the preview, the commit string, and
/// the state vector.
pub(crate) const MODIFIER_ORDER: [&str; 4] = ["ctrl", "win", "alt", "shift"];

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

    /// Opens on an existing chord, best effort: an unparsable stored
    /// value just starts empty.
    pub(crate) fn prefilled(target: HotkeyTarget, chord: &str) -> Self {
        let mut state = Self::new(target);
        if let Ok(hotkey) = parse_hotkey(chord) {
            state.modifiers = hotkey
                .modifiers
                .ordered()
                .filter_map(display_modifier)
                .map(str::to_string)
                .collect();
            state.main_key = Some(display_key(hotkey.key));
        }
        state
    }

    /// Preview and commit parts, modifiers first in canonical order.
    pub(crate) fn parts(&self) -> Vec<String> {
        let mut parts: Vec<String> = MODIFIER_ORDER
            .iter()
            .filter(|modifier| self.modifiers.iter().any(|held| held == *modifier))
            .map(|modifier| (*modifier).to_string())
            .collect();
        if let Some(key) = &self.main_key {
            parts.push(key.clone());
        }
        parts
    }

    /// Canonical chord string, or `None` while no non-modifier key is set.
    pub(crate) fn chord(&self) -> Option<String> {
        self.main_key.as_ref()?;
        let parts: Vec<String> = self
            .parts()
            .iter()
            .map(|part| core_alias(part).to_string())
            .collect();
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
        if MODIFIER_ORDER.contains(&id) {
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
        self.modifiers.push(id.to_string());
        self.modifiers.sort_by_key(|held| {
            MODIFIER_ORDER
                .iter()
                .position(|modifier| modifier == held)
                .unwrap_or(MODIFIER_ORDER.len())
        });
    }

    /// Physical keyboard while recording.
    pub(crate) fn press(&mut self, key: KeyEvent) -> RecorderAction {
        if key.kind != KeyEventKind::Press {
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
            self.modifiers = physical;
        }
        self.error = None;
        if id == "menu" {
            self.error = Some(MENU_ERROR.to_string());
            return RecorderAction::None;
        }
        self.main_key = Some(id);
        RecorderAction::None
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
    render_preview(frame, theme, &layout, state);
    render_divider(frame, theme, layout.content, layout.divider_bottom);
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

fn render_preview(frame: &mut Frame, theme: &Theme, layout: &Layout, state: &HotkeyRecorderState) {
    if layout.preview.width == 0 || layout.preview.height == 0 {
        return;
    }
    let parts = state.parts();
    if parts.is_empty() {
        let y = layout
            .preview
            .y
            .saturating_add(layout.preview.height.saturating_sub(1) / 2);
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Waiting for input...",
                Style::default().fg(theme.text_muted),
            )),
            Rect::new(layout.preview.x, y, layout.preview.width, 1),
        );
        return;
    }
    let right = layout.preview.x.saturating_add(layout.preview.width);
    let mut x = layout.preview.x;
    for part in parts {
        let label = format!(" {part} ");
        let width = label.chars().count() as u16;
        if x.saturating_add(width) > right {
            break;
        }
        let rect = Rect::new(x, layout.preview.y, width, layout.preview.height);
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.keycap.bg)),
            rect,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                label,
                Style::default()
                    .fg(theme.text)
                    .bg(theme.keycap.bg)
                    .add_modifier(StyleModifier::BOLD),
            ))
            .alignment(Alignment::Center),
            Rect::new(x, rect.y.saturating_add(rect.height / 2), width, 1),
        );
        x = x.saturating_add(width + 1);
    }
}

fn render_keycaps(frame: &mut Frame, theme: &Theme, layout: &Layout, state: &HotkeyRecorderState) {
    for (id, label, rect) in &layout.keycaps {
        if rect.width == 0 || rect.height == 0 {
            continue;
        }
        let active =
            state.modifiers.iter().any(|held| held == id) || state.main_key.as_deref() == Some(*id);
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
        frame.render_widget(
            Paragraph::new(Span::styled(
                util::truncate_to_width(label, rect.width),
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

fn physical_modifiers(modifiers: KeyModifiers) -> Vec<String> {
    let mut held = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        held.push("ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        held.push("win".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        held.push("alt".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        held.push("shift".to_string());
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

/// Stored chord name to the keycap id the keyboard draws.
fn display_key(key: LogicalKey) -> String {
    let name = key.canonical_name();
    match name.as_ref() {
        "printscreen" => "prtsc",
        "capslock" => "caps",
        "backspace" => "bksp",
        "delete" => "del",
        "insert" => "ins",
        "pgdown" => "pgdn",
        "scrolllock" => "scrlk",
        other => other,
    }
    .to_string()
}

fn display_modifier(modifier: Modifier) -> Option<&'static str> {
    Some(match modifier.canonical_name() {
        "ctrl" | "lctrl" | "rctrl" => "ctrl",
        "shift" | "lshift" | "rshift" => "shift",
        "alt" | "lalt" | "ralt" => "alt",
        "meta" | "lmeta" | "rmeta" => "win",
        _ => return None,
    })
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
    fn prefill_splits_a_stored_chord() {
        let state = HotkeyRecorderState::prefilled(HotkeyTarget::PrimaryTrigger, "alt+f6");
        assert_eq!(state.modifiers, vec!["alt".to_string()]);
        assert_eq!(state.main_key.as_deref(), Some("f6"));
    }

    #[test]
    fn prefill_translates_abbreviated_caps() {
        let state =
            HotkeyRecorderState::prefilled(HotkeyTarget::PrimaryTrigger, "ctrl+shift+printscreen");
        assert_eq!(
            state.modifiers,
            vec!["ctrl".to_string(), "shift".to_string()]
        );
        assert_eq!(state.main_key.as_deref(), Some("prtsc"));
        assert_eq!(state.chord().as_deref(), Some("ctrl+shift+printscreen"));
    }

    #[test]
    fn prefill_ignores_unparsable_text() {
        let state = HotkeyRecorderState::prefilled(HotkeyTarget::PrimaryTrigger, "hello there");
        assert!(state.modifiers.is_empty());
        assert_eq!(state.main_key, None);
    }

    #[test]
    fn chord_orders_modifiers_canonically() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.modifiers = vec!["shift".to_string(), "win".to_string(), "ctrl".to_string()];
        state.main_key = Some("a".to_string());
        assert_eq!(state.chord().as_deref(), Some("ctrl+win+shift+a"));
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
            .find(|(id, _, _)| *id == "ctrl")
            .expect("ctrl keycap");
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.click(area, rect.x + 1, rect.y);
        assert_eq!(state.modifiers, vec!["ctrl".to_string()]);
        state.click(area, rect.x + 1, rect.y);
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
    fn active_keys_fill_green_on_both_sides() {
        let mut state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        state.modifiers = vec!["shift".to_string()];
        state.main_key = Some("f6".to_string());
        let (terminal, area) = draw(&state);
        let layout = layout(area);
        for id in ["shift", "f6"] {
            let rects: Vec<Rect> = layout
                .keycaps
                .iter()
                .filter(|(key, _, _)| *key == id)
                .map(|(_, _, rect)| *rect)
                .collect();
            assert!(!rects.is_empty(), "{id} rendered");
            for rect in rects {
                assert_eq!(bg_at(&terminal, area, rect.x, rect.y), ENABLED_KNOB, "{id}");
            }
        }
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
    fn preview_waits_muted_until_a_key_lands() {
        let state = HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger);
        let (terminal, area) = draw(&state);
        let layout = layout(area);
        let y = layout.preview.y + layout.preview.height.saturating_sub(1) / 2;
        let row: String = (layout.preview.x..layout.preview.x + 20)
            .map(|x| {
                terminal.backend().buffer().content[(y * area.width + x) as usize]
                    .symbol()
                    .to_string()
            })
            .collect();
        assert_eq!(row, "Waiting for input...");
    }
}
