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
    let area = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    };
    if area.width == 0 || area.height == 0 {
        return;
    }
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
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(area)
    };

    let (list_area, search_area) = if has_status {
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
        (sections[1], sections[3])
    } else {
        (sections[0], sections[2])
    };

    list::render_library_list(frame, list_area, theme, state);
    search::render_library_search_bar(
        frame,
        search_area,
        theme,
        state.search_query(),
        state.is_search_active(),
        state.search_query().chars().count(),
    );
}
