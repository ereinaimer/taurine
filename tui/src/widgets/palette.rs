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
use crate::widgets::hotkey;
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
    /// One state-aware row: the label and icon flip with the daemon's
    /// pause flag instead of listing pause and resume separately.
    TogglePause,
    RestartTaurine,
    Quit,
}

impl PaletteCommand {
    /// Menu order, top to bottom.
    pub(crate) const ALL: [Self; 10] = [
        Self::ToggleLeftPanel,
        Self::ToggleRightPanel,
        Self::ResetLayout,
        Self::NewTrigger,
        Self::ImportTriggers,
        Self::ExportTriggers,
        Self::OpenSettings,
        Self::TogglePause,
        Self::RestartTaurine,
        Self::Quit,
    ];

    pub(crate) const fn label(self, paused: bool) -> &'static str {
        match self {
            Self::ToggleLeftPanel => "Toggle left panel",
            Self::ToggleRightPanel => "Toggle right panel",
            Self::ResetLayout => "Reset layout to defaults",
            Self::NewTrigger => "New Trigger",
            Self::ImportTriggers => "Import triggers",
            Self::ExportTriggers => "Export triggers",
            Self::OpenSettings => "Open settings",
            Self::TogglePause if paused => "Resume Taurine",
            Self::TogglePause => "Pause Taurine",
            Self::RestartTaurine => "Restart Taurine",
            Self::Quit => "Quit TUI",
        }
    }

    /// Existing shortcut shown flush right; empty when the command has
    /// no keybinding yet. The pause toggle shows the configured global
    /// pause hotkey, read from the settings database.
    fn hint(self, pause_hotkey: &str) -> &str {
        match self {
            Self::NewTrigger => "Ctrl+N",
            Self::OpenSettings => "Ctrl+,",
            Self::TogglePause => pause_hotkey,
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

/// Palette popup: 80% of the terminal in both axes, centered.
pub(crate) fn popup(area: Rect) -> Rect {
    let width = (u32::from(area.width) * 8 / 10) as u16;
    let height = (u32::from(area.height) * 8 / 10) as u16;
    util::overlay_popup_sized(width, height, area)
}

/// One blank row between entries; dense when the popup is too short to
/// fit the spaced list, so no command falls off the bottom.
fn row_step(content: Rect, count: usize) -> u16 {
    let required = (count as u16).saturating_mul(2).saturating_sub(1);
    if content.height >= required { 2 } else { 1 }
}

/// Scrim or title-hint click, matching every other overlay.
pub(crate) fn dismiss_hit(area: Rect, column: u16, row: u16) -> bool {
    util::overlay_dismiss_hit(popup(area), column, row)
}

/// Command index under a click, if any. Mirrors `render`: one-line
/// rows step by the same gap, and the gap rows themselves never select.
pub(crate) fn option_hit(area: Rect, column: u16, row: u16) -> Option<usize> {
    let chrome = OverlayChrome::new(popup(area));
    let count = PaletteCommand::ALL.len();
    if column < chrome.popup.x
        || column >= chrome.popup.x.saturating_add(chrome.popup.width)
        || row < chrome.content.y
        || row >= chrome.content.y.saturating_add(chrome.content.height)
    {
        return None;
    }
    let step = row_step(chrome.content, count);
    let offset = row - chrome.content.y;
    if !offset.is_multiple_of(step) {
        return None;
    }
    let index = usize::from(offset / step);
    (index < count).then_some(index)
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &CommandPaletteState,
    paused: bool,
    pause_hotkey: &str,
) {
    let chrome = OverlayChrome::new(popup(area));
    chrome.render(frame, theme, PALETTE_TITLE);
    if chrome.body.width == 0 || chrome.body.height == 0 {
        return;
    }
    let bottom = chrome.content.y.saturating_add(chrome.content.height);
    let step = row_step(chrome.content, PaletteCommand::ALL.len());
    for (index, command) in PaletteCommand::ALL.iter().enumerate() {
        let y = chrome
            .content
            .y
            .saturating_add((index as u16).saturating_mul(step));
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
            paused,
            pause_hotkey,
            chrome.popup.width.saturating_sub(3),
            selected,
        );
        frame.render_widget(Paragraph::new(line), row);
    }
}

/// One row: label left, shortcut right as split keycaps. The highlight
/// band is painted full-bleed, so padding stays inside it.
fn row_line(
    theme: &Theme,
    command: PaletteCommand,
    paused: bool,
    pause_hotkey: &str,
    width: u16,
    selected: bool,
) -> Line<'static> {
    let label = format!("   {}", command.label(paused));
    let label_width = label.chars().count();
    let hint = command.hint(pause_hotkey);
    let label_style = if selected {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    };
    let mut spans = vec![Span::styled(label, label_style)];
    if !hint.is_empty() {
        // Pills sit on the row fill, lightened on the selected band.
        let (keycap_bg, base_bg) = if selected {
            (theme.keycap.on_surface, theme.surface)
        } else {
            (theme.keycap.bg, util::OVERLAY_BG)
        };
        let keys = hotkey::render_hotkey_spans(
            hint,
            (width as usize).saturating_sub(label_width),
            keycap_bg,
            theme.text,
            base_bg,
        );
        let keys_width = hotkey::spans_width(&keys);
        if keys_width > 0 {
            spans.push(Span::raw(
                " ".repeat((width as usize).saturating_sub(label_width + keys_width)),
            ));
            spans.extend(keys);
        }
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
        render_buffer_with(state, false, "Alt + `")
    }

    fn render_buffer_with(state: &CommandPaletteState, paused: bool, pause_hotkey: &str) -> Buffer {
        render_buffer_in(AREA, state, paused, pause_hotkey)
    }

    fn render_buffer_in(
        area: Rect,
        state: &CommandPaletteState,
        paused: bool,
        pause_hotkey: &str,
    ) -> Buffer {
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    &DARK_THEME,
                    state,
                    paused,
                    pause_hotkey,
                )
            })
            .expect("test draw");
        terminal.backend().buffer().clone()
    }

    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    fn cells(buffer: &Buffer, x: u16, y: u16, width: u16) -> String {
        (x..x.saturating_add(width))
            .map(|column| buffer[(column, y)].symbol())
            .collect()
    }

    #[test]
    fn commands_are_listed_in_order() {
        assert_eq!(
            PaletteCommand::ALL.map(|command| command.label(false)),
            [
                "Toggle left panel",
                "Toggle right panel",
                "Reset layout to defaults",
                "New Trigger",
                "Import triggers",
                "Export triggers",
                "Open settings",
                "Pause Taurine",
                "Restart Taurine",
                "Quit TUI",
            ]
        );
    }

    #[test]
    fn pause_row_flips_with_the_daemon_state() {
        assert_eq!(PaletteCommand::TogglePause.label(false), "Pause Taurine");
        assert_eq!(PaletteCommand::TogglePause.label(true), "Resume Taurine");
    }

    #[test]
    fn every_command_fits_the_popup_with_gap_rows() {
        // honey: the palette renders whole with no scrolling; a new
        // command past the content height would silently vanish.
        let chrome = OverlayChrome::new(popup(AREA));
        let required = PaletteCommand::ALL.len() as u16 * 2 - 1;
        assert!(required <= chrome.content.height);
        assert_eq!(row_step(chrome.content, PaletteCommand::ALL.len()), 2);
    }

    #[test]
    fn popup_takes_eighty_percent_of_the_terminal() {
        let popup = popup(AREA);
        assert_eq!(popup.width, 80);
        assert_eq!(popup.height, 24);
        assert_eq!(popup.x, 10);
        assert_eq!(popup.y, 3);
    }

    #[test]
    fn short_terminal_drops_the_gap_instead_of_the_tail() {
        let short = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 20,
        };
        let chrome = OverlayChrome::new(popup(short));
        assert_eq!(row_step(chrome.content, PaletteCommand::ALL.len()), 1);

        let state = CommandPaletteState::default();
        let buffer = render_buffer_in(short, &state, false, "Alt + `");
        let content_y = chrome.content.y;
        // Consecutive rows: every command stays reachable.
        assert!(row_text(&buffer, content_y + 7).contains("Pause Taurine"));
        assert_eq!(
            option_hit(short, chrome.popup.x + 3, content_y + 7),
            Some(7)
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
        // Plain label: no icon glyph ahead of the text.
        assert_eq!(first.trim(), "Toggle left panel");

        let new_trigger = row_text(&buffer, content_y + 6);
        assert!(new_trigger.contains("New Trigger"));

        let pause = row_text(&buffer, content_y + 14);
        assert!(pause.contains("Pause Taurine"));
        let quit = row_text(&buffer, content_y + 18);
        assert!(quit.contains("Quit TUI"));

        // Every shortcut is split keycaps ending on the same column, one
        // cell of popup padding from the right edge.
        let right = popup.x + popup.width - 4;
        for (row, keys) in [
            (6u16, " Ctrl   N "),
            (12, " Ctrl   , "),
            (14, " Alt   ` "),
            (18, " Ctrl   C "),
        ] {
            let y = content_y + row;
            let keys_width = keys.chars().count() as u16;
            assert_eq!(
                cells(&buffer, right + 1 - keys_width, y, keys_width),
                keys,
                "row {row}"
            );
            assert_eq!(buffer[(right, y)].bg, DARK_THEME.keycap.bg, "row {row}");
        }

        // The cursor row paints the shared highlight band; the gap row
        // and the next entry keep the popup fill.
        assert_eq!(buffer[(popup.x, content_y)].bg, DARK_THEME.surface);
        assert_eq!(buffer[(popup.x, content_y + 1)].bg, util::OVERLAY_BG);
        assert_eq!(buffer[(popup.x, content_y + 2)].bg, util::OVERLAY_BG);
    }

    #[test]
    fn selected_row_lightens_its_keycaps() {
        let mut state = CommandPaletteState::default();
        for _ in 0..3 {
            state.move_cursor(1);
        }
        let buffer = render_buffer(&state);
        let popup = popup(AREA);
        let y = popup.y + 4 + 3 * 2;
        let right = popup.x + popup.width - 4;

        assert_eq!(buffer[(right, y)].bg, DARK_THEME.keycap.on_surface);
        assert_eq!(buffer[(right, y)].fg, DARK_THEME.text);
    }

    #[test]
    fn paused_daemon_swaps_the_pause_row_to_resume() {
        let state = CommandPaletteState::default();
        let buffer = render_buffer_with(&state, true, "Ctrl + Shift + P");
        let popup = popup(AREA);
        let content_y = popup.y + 4;

        let pause = row_text(&buffer, content_y + 14);
        assert!(pause.contains("Resume Taurine"));
        assert!(!pause.contains("Pause Taurine"));
        // Still one row: the next entry is Restart, not a second toggle.
        let restart = row_text(&buffer, content_y + 16);
        assert!(restart.contains("Restart Taurine"));

        // The configured chord splits into three pills, right-aligned
        // with the other shortcut rows.
        let keys = " Ctrl   Shift   P ";
        let keys_width = keys.chars().count() as u16;
        let right = popup.x + popup.width - 4;
        assert_eq!(
            cells(&buffer, right + 1 - keys_width, content_y + 14, keys_width),
            keys
        );
        assert_eq!(buffer[(right, content_y + 14)].bg, DARK_THEME.keycap.bg);
    }

    #[test]
    fn cursor_reaches_the_pause_toggle() {
        let mut state = CommandPaletteState::default();
        for _ in 0..7 {
            state.move_cursor(1);
        }
        assert_eq!(state.command(), PaletteCommand::TogglePause);
    }

    #[test]
    fn hit_maps_each_row() {
        let popup = popup(AREA);
        let content_y = popup.y + 4;
        for index in 0..PaletteCommand::ALL.len() {
            let y = content_y + index as u16 * 2;
            assert_eq!(option_hit(AREA, popup.x + 3, y), Some(index));
            // The gap row under each entry selects nothing.
            assert_eq!(option_hit(AREA, popup.x + 3, y + 1), None);
        }
        assert_eq!(option_hit(AREA, popup.x + 3, content_y + 19), None);
    }

    #[test]
    fn scrim_click_dismisses() {
        assert!(dismiss_hit(AREA, 0, 0));
        assert!(!dismiss_hit(AREA, popup(AREA).x + 3, popup(AREA).y + 4));
    }
}
