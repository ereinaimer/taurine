pub mod actions;
pub mod list;
pub mod modals;
pub mod search;
pub mod state;
pub(crate) use actions::*;
pub(crate) use state::*;

#[cfg(test)]
mod tests;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::theme::Theme;

pub fn render_library_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
) {
    if let Some(message) = state.load_error() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(message).style(
                ratatui::style::Style::default()
                    .fg(theme.error)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            area,
        );
        return;
    }

    let has_status = state.status_message().is_some();
    let sections = if has_status {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(area)
    };

    let search_index = if has_status {
        if let Some(message) = state.status_message() {
            frame.render_widget(
                ratatui::widgets::Paragraph::new(message).style(
                    ratatui::style::Style::default()
                        .fg(theme.text)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
                sections[0],
            );
        }
        1
    } else {
        0
    };

    search::render_library_search_bar(
        frame,
        sections[search_index],
        theme,
        state.search_query(),
        state.is_search_active(),
        state.search_query().chars().count(),
    );

    let list_area = sections[search_index + 2];
    list::render_library_list(frame, list_area, theme, state);
}
