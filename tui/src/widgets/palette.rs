//! Command palette: app-wide quick actions on the shared overlay chrome.
//! Opens over any layer, so the same menu works from the library, the
//! settings overlay, and open dialogs.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::field::TextField;
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

/// Palette cursor plus the bottom search field. The cursor indexes the
/// filtered list; the visible window is derived from its capacity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CommandPaletteState {
    selected: usize,
    search: TextField,
    search_active: bool,
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
    /// Commands matching the current query, in menu order. The pause
    /// row matches either label so "pause" and "resume" both find it.
    pub(crate) fn visible_commands(&self) -> Vec<PaletteCommand> {
        let needle = self.search.text().trim().to_ascii_lowercase();
        PaletteCommand::ALL
            .into_iter()
            .filter(|command| {
                needle.is_empty()
                    || command.label(false).to_ascii_lowercase().contains(&needle)
                    || command.label(true).to_ascii_lowercase().contains(&needle)
            })
            .collect()
    }

    pub(crate) fn command(&self) -> Option<PaletteCommand> {
        self.visible_commands().get(self.selected).copied()
    }

    /// Parks the cursor on a clicked row.
    pub(crate) fn select(&mut self, index: usize) {
        if index < self.visible_commands().len() {
            self.selected = index;
        }
    }

    pub(crate) fn search_field(&self) -> &TextField {
        &self.search
    }

    pub(crate) const fn is_search_active(&self) -> bool {
        self.search_active
    }

    pub(crate) fn activate_search_at(&mut self, cursor: usize) {
        self.search_active = true;
        self.search.place(cursor);
    }

    fn reset_selection(&mut self) {
        self.selected = 0;
    }

    fn move_cursor(&mut self, delta: i32) {
        let len = self.visible_commands().len() as i32;
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as i32 + delta).rem_euclid(len) as usize;
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> PaletteOutcome {
        if self.search_active {
            return self.handle_search_key(key);
        }
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
                self.selected = self.visible_commands().len().saturating_sub(1);
                PaletteOutcome::Handled
            }
            (KeyCode::Enter, KeyModifiers::NONE) => self
                .command()
                .map_or(PaletteOutcome::Handled, PaletteOutcome::Run),
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.search_active = true;
                PaletteOutcome::Handled
            }
            // honey: navigation and quit keys never start a search; any
            // other bare character filters the list immediately.
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_active = true;
                self.search.insert(ch);
                self.reset_selection();
                PaletteOutcome::Handled
            }
            _ => PaletteOutcome::Handled,
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) -> PaletteOutcome {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => {
                self.search_active = false;
                PaletteOutcome::Handled
            }
            (KeyCode::Enter, KeyModifiers::NONE) => self
                .command()
                .map_or(PaletteOutcome::Handled, PaletteOutcome::Run),
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_cursor(-1);
                PaletteOutcome::Handled
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_cursor(1);
                PaletteOutcome::Handled
            }
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.search.move_left();
                PaletteOutcome::Handled
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.search.move_right();
                PaletteOutcome::Handled
            }
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.search.move_home();
                PaletteOutcome::Handled
            }
            (KeyCode::End, KeyModifiers::NONE) => {
                self.search.move_end();
                PaletteOutcome::Handled
            }
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                if self.search.backspace() {
                    self.reset_selection();
                }
                PaletteOutcome::Handled
            }
            (KeyCode::Delete, KeyModifiers::NONE) => {
                if self.search.delete_at() {
                    self.reset_selection();
                }
                PaletteOutcome::Handled
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search.insert(ch);
                self.reset_selection();
                PaletteOutcome::Handled
            }
            _ => PaletteOutcome::Handled,
        }
    }
}

/// Palette popup: 90% of the terminal in both axes, centered.
pub(crate) fn popup(area: Rect) -> Rect {
    let width = util::percent_of(area.width, 90);
    let height = util::percent_of(area.height, 90);
    util::overlay_popup_sized(width, height, area)
}

/// Every entry is a three-row block: a blank pad row, the content row,
/// and a blank pad row, stacked with no gap like the settings and
/// library lists. The selected band paints the whole block.
const ITEM_HEIGHT: u16 = 3;

/// Entry blocks that fit the list; the list windows when it cannot
/// show them all.
fn item_capacity(list: Rect) -> usize {
    usize::from(list.height / ITEM_HEIGHT).max(1)
}

/// Visible `[start, end)` window keeping the cursor in view, matching
/// the library list's scrolling.
fn visible_window(list: Rect, total: usize, selected: usize) -> (usize, usize) {
    util::visible_range(total, selected, item_capacity(list))
}

/// List above the bottom search bar, mirroring the settings overlay:
/// a one-row gap then the rounded search block.
fn content_sections(area: Rect, state: &CommandPaletteState) -> (Rect, Rect) {
    let search_height =
        util::search_block_height(state.search_field(), state.is_search_active(), area.width)
            .min(max_search_height(area.height));
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(search_height),
        ])
        .split(area);
    (sections[0], sections[2])
}

/// Cap the search block so a long query never eats the list above it.
fn max_search_height(area_height: u16) -> u16 {
    const MIN_LIST_HEIGHT: u16 = 3;
    area_height.saturating_sub(1 + MIN_LIST_HEIGHT).max(3)
}

/// Scrim or title-hint click, matching every other overlay.
pub(crate) fn dismiss_hit(area: Rect, column: u16, row: u16) -> bool {
    util::overlay_dismiss_hit(popup(area), column, row)
}

/// Command index under a click, if any. Mirrors `render`: a whole block
/// selects, pad rows included, and the window follows the cursor.
pub(crate) fn option_hit(
    area: Rect,
    state: &CommandPaletteState,
    column: u16,
    row: u16,
) -> Option<usize> {
    let chrome = OverlayChrome::new(popup(area));
    let (list, _) = content_sections(chrome.content, state);
    if !crate::terminal::mouse::contains(list, column, row) {
        return None;
    }
    let (start, end) = visible_window(list, state.visible_commands().len(), state.selected);
    let offset = usize::from(row - list.y) / usize::from(ITEM_HEIGHT);
    let index = start.saturating_add(offset);
    (index < end).then_some(index)
}

/// Caret index under a click inside the bottom search box, if any.
pub(crate) fn search_hit(
    area: Rect,
    state: &CommandPaletteState,
    column: u16,
    row: u16,
) -> Option<usize> {
    let chrome = OverlayChrome::new(popup(area));
    let (_, search) = content_sections(chrome.content, state);
    crate::terminal::mouse::contains(search, column, row).then(|| {
        util::search_block_index_at(
            state.search_field(),
            state.is_search_active(),
            search,
            column,
            row,
        )
    })
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
    let (list, search_area) = content_sections(chrome.content, state);
    if list.height > 0 {
        let visible = state.visible_commands();
        if visible.is_empty() {
            frame.render_widget(
                Paragraph::new("No commands match your search.")
                    .style(Style::default().fg(theme.description)),
                Rect {
                    x: list.x,
                    y: list.y.saturating_add(list.height.saturating_sub(1)),
                    width: list.width,
                    height: 1.min(list.height),
                },
            );
        } else {
            let bottom = list.y.saturating_add(list.height);
            let (start, end) = visible_window(list, visible.len(), state.selected);
            for (offset, index) in (start..end).enumerate() {
                let y = list
                    .y
                    .saturating_add((offset as u16).saturating_mul(ITEM_HEIGHT));
                if y >= bottom {
                    break;
                }
                let block = Rect {
                    x: chrome.popup.x,
                    y,
                    width: chrome.popup.width,
                    height: ITEM_HEIGHT.min(bottom.saturating_sub(y)),
                };
                let command = visible[index];
                let selected = index == state.selected;
                if selected {
                    frame.render_widget(
                        Block::default().style(Style::default().bg(theme.surface)),
                        block,
                    );
                }
                let row = Rect {
                    x: list.x,
                    y: y.saturating_add(1),
                    width: list.width,
                    height: 1,
                };
                if row.y >= bottom {
                    continue;
                }
                let line = row_line(theme, command, paused, pause_hotkey, row.width, selected);
                frame.render_widget(Paragraph::new(line), row);
            }
        }
    }
    util::render_search_block(
        frame,
        search_area,
        theme,
        state.search_field(),
        state.is_search_active(),
        "Search",
    );
}

/// One row: label left, shortcut right as split keycaps. The selected
/// band is painted over the whole three-row block, so the pads stay
/// inside it.
fn row_line(
    theme: &Theme,
    command: PaletteCommand,
    paused: bool,
    pause_hotkey: &str,
    width: u16,
    selected: bool,
) -> Line<'static> {
    let label = command.label(paused);
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
    fn every_command_scrolls_into_view() {
        let chrome = OverlayChrome::new(popup(AREA));
        let state = CommandPaletteState::default();
        let (list, _) = content_sections(chrome.content, &state);
        let capacity = item_capacity(list);
        assert_eq!(capacity, 6);
        for selected in 0..PaletteCommand::ALL.len() {
            let (start, end) = visible_window(list, PaletteCommand::ALL.len(), selected);
            assert!(
                start <= selected && selected < end,
                "selected {selected} outside {start}..{end}"
            );
            assert!(end - start <= capacity);
        }
    }

    #[test]
    fn popup_takes_ninety_percent_of_the_terminal() {
        let popup = popup(AREA);
        assert_eq!(popup.width, 90);
        assert_eq!(popup.height, 27);
        assert_eq!(popup.x, 5);
        assert_eq!(popup.y, 1);
    }

    #[test]
    fn short_popup_windows_the_list() {
        let short = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 20,
        };
        let chrome = OverlayChrome::new(popup(short));
        let mut state = CommandPaletteState::default();
        let (list, _) = content_sections(chrome.content, &state);
        assert_eq!(item_capacity(list), 3);

        state.handle_key(key(KeyCode::End));
        let buffer = render_buffer_in(short, &state, false, "Alt + `");
        let content_y = list.y;
        // The window follows the cursor: the last three entries render.
        assert!(row_text(&buffer, content_y + 1).contains("Pause Taurine"));
        assert!(row_text(&buffer, content_y + 7).contains("Quit TUI"));
        assert_eq!(
            option_hit(short, &state, chrome.popup.x + 3, content_y + 7),
            Some(9)
        );
    }

    #[test]
    fn typing_filters_commands_and_enter_runs_the_match() {
        let mut state = CommandPaletteState::default();
        assert_eq!(
            state.handle_key(key(KeyCode::Char('q'))),
            PaletteOutcome::Handled
        );
        assert!(state.is_search_active());
        assert_eq!(state.search_field().text(), "q");
        assert_eq!(state.visible_commands(), vec![PaletteCommand::Quit]);
        assert_eq!(state.command(), Some(PaletteCommand::Quit));
        assert_eq!(
            state.handle_key(key(KeyCode::Enter)),
            PaletteOutcome::Run(PaletteCommand::Quit)
        );

        let mut state = CommandPaletteState::default();
        for ch in "resume".chars() {
            state.handle_key(key(KeyCode::Char(ch)));
        }
        assert_eq!(state.visible_commands(), vec![PaletteCommand::TogglePause]);
    }

    #[test]
    fn esc_leaves_search_before_closing() {
        let mut state = CommandPaletteState::default();
        state.handle_key(key(KeyCode::Char('q')));
        assert!(state.is_search_active());

        assert_eq!(state.handle_key(key(KeyCode::Esc)), PaletteOutcome::Handled);
        assert!(!state.is_search_active());
        assert_eq!(state.handle_key(key(KeyCode::Esc)), PaletteOutcome::Close);
    }

    #[test]
    fn search_bar_renders_above_the_popup_bottom() {
        let state = CommandPaletteState::default();
        let buffer = render_buffer(&state);
        let chrome = OverlayChrome::new(popup(AREA));
        let (list, search) = content_sections(chrome.content, &state);
        assert!(search.y > list.y);
        assert_eq!(buffer[(search.x, search.y)].symbol(), "╭");
        assert!(row_text(&buffer, search.y + 1).contains("Search"));
    }

    #[test]
    fn search_hit_targets_the_field() {
        let state = CommandPaletteState::default();
        let chrome = OverlayChrome::new(popup(AREA));
        let (_, search) = content_sections(chrome.content, &state);
        assert_eq!(
            search_hit(AREA, &state, search.x + 2, search.y + 1),
            Some(0)
        );

        let mut state = CommandPaletteState::default();
        state.activate_search_at(0);
        assert!(state.is_search_active());
    }

    #[test]
    fn empty_result_shows_the_no_matches_message() {
        let mut state = CommandPaletteState::default();
        for ch in "zzz".chars() {
            state.handle_key(key(KeyCode::Char(ch)));
        }
        assert!(state.visible_commands().is_empty());
        assert_eq!(
            state.handle_key(key(KeyCode::Enter)),
            PaletteOutcome::Handled
        );

        let buffer = render_buffer(&state);
        let chrome = OverlayChrome::new(popup(AREA));
        let (list, _) = content_sections(chrome.content, &state);
        assert!(row_text(&buffer, list.y + list.height - 1).contains("No commands match"));
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

        // First block: blank pad, content, blank pad.
        let first = row_text(&buffer, content_y + 1);
        // Plain label: no icon glyph ahead of the text.
        assert_eq!(first.trim(), "Toggle left panel");

        let new_trigger = row_text(&buffer, content_y + 10);
        assert!(new_trigger.contains("New Trigger"));

        // Shortcut keycaps end on the same column, inside the block.
        let right = popup.x + popup.width - 4;
        let keys = " Ctrl   N ";
        let keys_width = keys.chars().count() as u16;
        assert_eq!(
            cells(&buffer, right + 1 - keys_width, content_y + 10, keys_width),
            keys
        );
        assert_eq!(buffer[(right, content_y + 10)].bg, DARK_THEME.keycap.bg);

        // The selected block paints all three rows; the next block's
        // top pad starts immediately with the popup fill.
        for dy in 0..ITEM_HEIGHT {
            assert_eq!(buffer[(popup.x, content_y + dy)].bg, DARK_THEME.surface);
        }
        assert_eq!(
            buffer[(popup.x, content_y + ITEM_HEIGHT)].bg,
            util::OVERLAY_BG
        );
    }

    #[test]
    fn selected_row_lightens_its_keycaps() {
        let mut state = CommandPaletteState::default();
        for _ in 0..3 {
            state.move_cursor(1);
        }
        let buffer = render_buffer(&state);
        let popup = popup(AREA);
        let y = popup.y + 4 + 1 + 3 * ITEM_HEIGHT;
        let right = popup.x + popup.width - 4;

        assert_eq!(buffer[(right, y)].bg, DARK_THEME.keycap.on_surface);
        assert_eq!(buffer[(right, y)].fg, DARK_THEME.text);
    }

    #[test]
    fn paused_daemon_swaps_the_pause_row_to_resume() {
        let mut state = CommandPaletteState::default();
        // Keep the pause and restart blocks in view.
        state.handle_key(key(KeyCode::End));
        state.handle_key(key(KeyCode::Up));
        let buffer = render_buffer_with(&state, true, "Ctrl + Shift + P");
        let popup = popup(AREA);
        let chrome = OverlayChrome::new(popup);
        let (list, _) = content_sections(chrome.content, &state);
        let (start, _) = visible_window(list, state.visible_commands().len(), state.selected);
        let content_y = list.y;
        let pause_y = content_y + (7 - start) as u16 * ITEM_HEIGHT + 1;
        let restart_y = content_y + (8 - start) as u16 * ITEM_HEIGHT + 1;

        let pause = row_text(&buffer, pause_y);
        assert!(pause.contains("Resume Taurine"));
        assert!(!pause.contains("Pause Taurine"));
        // Still one row: the next entry is Restart, not a second toggle.
        let restart = row_text(&buffer, restart_y);
        assert!(restart.contains("Restart Taurine"));

        // The configured chord splits into three pills, right-aligned
        // with the other shortcut rows.
        let keys = " Ctrl   Shift   P ";
        let keys_width = keys.chars().count() as u16;
        let right = popup.x + popup.width - 4;
        assert_eq!(
            cells(&buffer, right + 1 - keys_width, pause_y, keys_width),
            keys
        );
        assert_eq!(buffer[(right, pause_y)].bg, DARK_THEME.keycap.bg);
    }

    #[test]
    fn cursor_reaches_the_pause_toggle() {
        let mut state = CommandPaletteState::default();
        for _ in 0..7 {
            state.move_cursor(1);
        }
        assert_eq!(state.command(), Some(PaletteCommand::TogglePause));
    }

    #[test]
    fn hit_maps_each_block() {
        let mut state = CommandPaletteState::default();
        let popup = popup(AREA);
        let chrome = OverlayChrome::new(popup);
        let (list, _) = content_sections(chrome.content, &state);
        let content_y = list.y;
        let capacity = item_capacity(list);
        for index in 0..capacity as u16 {
            let y = content_y + index * ITEM_HEIGHT;
            // The whole block selects, pad rows included.
            for dy in 0..ITEM_HEIGHT {
                assert_eq!(
                    option_hit(AREA, &state, popup.x + 3, y + dy),
                    Some(index as usize)
                );
            }
        }
        // The block after the default window is outside.
        assert_eq!(
            option_hit(
                AREA,
                &state,
                popup.x + 3,
                content_y + capacity as u16 * ITEM_HEIGHT
            ),
            None
        );

        // Scrolling moves the window with the cursor.
        state.handle_key(key(KeyCode::End));
        let (start, end) = visible_window(list, state.visible_commands().len(), state.selected);
        for (offset, index) in (start..end).enumerate() {
            assert_eq!(
                option_hit(
                    AREA,
                    &state,
                    popup.x + 3,
                    content_y + offset as u16 * ITEM_HEIGHT
                ),
                Some(index)
            );
        }
        assert_eq!(
            option_hit(
                AREA,
                &state,
                popup.x + 3,
                content_y + (end - start) as u16 * ITEM_HEIGHT
            ),
            None
        );
    }

    #[test]
    fn scrim_click_dismisses() {
        assert!(dismiss_hit(AREA, 0, 0));
        assert!(!dismiss_hit(AREA, popup(AREA).x + 3, popup(AREA).y + 4));
    }
}
