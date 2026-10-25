//! Command palette: app-wide quick actions on the shared overlay chrome.
//! Opens over any layer, so the same menu works from the library, the
//! settings overlay, and open dialogs.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::library::icons;
use crate::widgets::util::{self, OverlayChrome};

/// Header title for the palette overlay.
pub(crate) const PALETTE_TITLE: &str = "Commands";

/// Actions the palette can run. Rendering, hit-testing, and execution
/// all key off this list, so adding a command is one enum arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaletteCommand {
    ToggleLeftPanel,
    ToggleRightPanel,
    ResetLayout,
    NewTrigger,
    ImportTriggers,
    ExportTriggers,
    OpenSettings,
    RestartTaurine,
    Quit,
}

impl PaletteCommand {
    /// Menu order, top to bottom.
    pub(crate) const ALL: [Self; 9] = [
        Self::ToggleLeftPanel,
        Self::ToggleRightPanel,
        Self::ResetLayout,
        Self::NewTrigger,
        Self::ImportTriggers,
        Self::ExportTriggers,
        Self::OpenSettings,
        Self::RestartTaurine,
        Self::Quit,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ToggleLeftPanel => "Toggle left panel",
            Self::ToggleRightPanel => "Toggle right panel",
            Self::ResetLayout => "Reset layout to defaults",
            Self::NewTrigger => "New Trigger",
            Self::ImportTriggers => "Import triggers",
            Self::ExportTriggers => "Export triggers",
            Self::OpenSettings => "Open settings",
            Self::RestartTaurine => "Restart Taurine",
            Self::Quit => "Quit TUI",
        }
    }

    const fn icon(self) -> &'static str {
        match self {
            Self::ToggleLeftPanel => icons::PANEL_LEFT_ICON,
            Self::ToggleRightPanel => icons::PANEL_RIGHT_ICON,
            Self::ResetLayout => icons::RESET_LAYOUT_ICON,
            Self::NewTrigger => icons::ADD_ICON,
            Self::ImportTriggers => icons::IMPORT_ICON,
            Self::ExportTriggers => icons::EXPORT_ICON,
            Self::OpenSettings => icons::SETTINGS_ICON,
            Self::RestartTaurine => icons::RESTART_ICON,
            Self::Quit => icons::QUIT_ICON,
        }
    }

    /// Existing shortcut shown flush right; empty when the command has
    /// no keybinding yet.
    const fn hint(self) -> &'static str {
        match self {
            Self::NewTrigger => "Ctrl+N",
            Self::OpenSettings => "Ctrl+,",
            Self::Quit => "Ctrl+C",
            _ => "",
        }
    }
}

/// Palette cursor. The list is short enough to render whole, so no
/// window anchor is needed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CommandPaletteState {
    selected: usize,
}

/// What a key did to the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaletteOutcome {
    /// Consumed; the palette stays open.
    Handled,
    /// Esc: dismiss the palette.
    Close,
    /// Enter: dismiss and run the command.
    Run(PaletteCommand),
}

impl CommandPaletteState {
    pub(crate) const fn command(&self) -> PaletteCommand {
        PaletteCommand::ALL[self.selected]
    }

    /// Parks the cursor on a clicked row.
    pub(crate) fn select(&mut self, index: usize) {
        if index < PaletteCommand::ALL.len() {
            self.selected = index;
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        let len = PaletteCommand::ALL.len() as i32;
        self.selected = (self.selected as i32 + delta).rem_euclid(len) as usize;
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> PaletteOutcome {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => PaletteOutcome::Close,
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_cursor(-1);
                PaletteOutcome::Handled
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_cursor(1);
                PaletteOutcome::Handled
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.selected = 0;
                PaletteOutcome::Handled
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.selected = PaletteCommand::ALL.len() - 1;
                PaletteOutcome::Handled
            }
            (KeyCode::Enter, KeyModifiers::NONE) => PaletteOutcome::Run(self.command()),
            _ => PaletteOutcome::Handled,
        }
    }
}

/// Shared fixed overlay rect; the nine rows fit the standard content
/// area with one spare row.
pub(crate) fn popup(area: Rect) -> Rect {
    util::overlay_popup(area)
}

/// Scrim or title-hint click, matching every other overlay.
pub(crate) fn dismiss_hit(area: Rect, column: u16, row: u16) -> bool {
    util::overlay_dismiss_hit(popup(area), column, row)
}

/// Command index under a click, if any.
pub(crate) fn option_hit(area: Rect, column: u16, row: u16) -> Option<usize> {
    let heights = [1u16; PaletteCommand::ALL.len()];
    util::overlay_option_hit(area, &heights, column, row)
}

pub(crate) fn render(frame: &mut Frame, area: Rect, theme: &Theme, state: &CommandPaletteState) {
    let chrome = OverlayChrome::new(popup(area));
    chrome.render(frame, theme, PALETTE_TITLE);
    if chrome.body.width == 0 || chrome.body.height == 0 {
        return;
    }
    let bottom = chrome.content.y.saturating_add(chrome.content.height);
    for (index, command) in PaletteCommand::ALL.iter().enumerate() {
        let y = chrome.content.y.saturating_add(index as u16);
        if y >= bottom {
            break;
        }
        let row = Rect {
            x: chrome.popup.x,
            y,
            width: chrome.popup.width,
            height: 1,
        };
        let selected = index == state.selected;
        if selected {
            frame.render_widget(
                Block::default().style(Style::default().bg(theme.surface)),
                row,
            );
        }
        let line = row_line(
            theme,
            *command,
            chrome.popup.width.saturating_sub(3),
            selected,
        );
        frame.render_widget(Paragraph::new(line), row);
    }
}

/// One row: icon-padded label left, dimmed shortcut flush right. The
/// highlight band is painted full-bleed, so padding stays inside it.
fn row_line(theme: &Theme, command: PaletteCommand, width: u16, selected: bool) -> Line<'static> {
    let label = format!("   {} {}", command.icon(), command.label());
    let hint = command.hint();
    let gap = (width as usize)
        .saturating_sub(label.chars().count())
        .saturating_sub(hint.chars().count());
    let label_style = if selected {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    };
    let mut spans = vec![Span::styled(label, label_style)];
    if !hint.is_empty() {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled(
            hint.to_string(),
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::default::DARK_THEME;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 30,
    };

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn render_buffer(state: &CommandPaletteState) -> Buffer {
        let backend = TestBackend::new(AREA.width, AREA.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, frame.area(), &DARK_THEME, state))
            .expect("test draw");
        terminal.backend().buffer().clone()
    }

    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn commands_are_listed_in_order() {
        assert_eq!(
            PaletteCommand::ALL.map(PaletteCommand::label),
            [
                "Toggle left panel",
                "Toggle right panel",
                "Reset layout to defaults",
                "New Trigger",
                "Import triggers",
                "Export triggers",
                "Open settings",
                "Restart Taurine",
                "Quit TUI",
            ]
        );
    }

    #[test]
    fn cursor_wraps_in_both_directions() {
        let mut state = CommandPaletteState::default();
        state.move_cursor(-1);
        assert_eq!(state.selected, PaletteCommand::ALL.len() - 1);
        state.move_cursor(1);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn enter_runs_the_selected_command() {
        let mut state = CommandPaletteState::default();
        state.handle_key(key(KeyCode::Down));
        state.handle_key(key(KeyCode::Down));

        assert_eq!(
            state.handle_key(key(KeyCode::Enter)),
            PaletteOutcome::Run(PaletteCommand::ResetLayout)
        );
    }

    #[test]
    fn esc_dismisses() {
        let mut state = CommandPaletteState::default();
        assert_eq!(state.handle_key(key(KeyCode::Esc)), PaletteOutcome::Close);
    }

    #[test]
    fn render_shows_labels_and_shortcuts() {
        let state = CommandPaletteState::default();
        let buffer = render_buffer(&state);
        let popup = popup(AREA);
        let content_y = popup.y + 4;

        let first = row_text(&buffer, content_y);
        assert!(first.contains("Toggle left panel"));

        let new_trigger = row_text(&buffer, content_y + 3);
        assert!(new_trigger.contains("New Trigger"));
        assert!(new_trigger.trim_end().ends_with("Ctrl+N"));

        // The cursor row paints the shared highlight band.
        assert_eq!(buffer[(popup.x, content_y)].bg, DARK_THEME.surface);
        assert_ne!(buffer[(popup.x, content_y + 1)].bg, DARK_THEME.surface);
    }

    #[test]
    fn hit_maps_each_row() {
        let popup = popup(AREA);
        let content_y = popup.y + 4;
        for index in 0..PaletteCommand::ALL.len() {
            assert_eq!(
                option_hit(AREA, popup.x + 3, content_y + index as u16),
                Some(index)
            );
        }
        assert_eq!(option_hit(AREA, popup.x + 3, content_y + 9), None);
    }

    #[test]
    fn scrim_click_dismisses() {
        assert!(dismiss_hit(AREA, 0, 0));
        assert!(!dismiss_hit(AREA, popup(AREA).x + 3, popup(AREA).y + 4));
    }
}
