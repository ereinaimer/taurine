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

/// Split the page into left (list) and right (detail) panes with a
/// one-column gutter, shared by rendering and mouse hit-testing.
pub(crate) fn content_halves(area: Rect) -> (Rect, Rect) {
    if area.width < 5 {
        return (area, Rect::default());
    }
    let left_width = area.width.saturating_sub(1) / 2;
    (
        Rect {
            x: area.x,
            y: area.y,
            width: left_width,
            height: area.height,
        },
        Rect {
            x: area.x.saturating_add(left_width).saturating_add(1),
            y: area.y,
            width: area.width.saturating_sub(left_width).saturating_sub(1),
            height: area.height,
        },
    )
}

/// Pane border strip shared by rendering and hit-testing.
fn pane_inner(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

/// List content inside the left pane: pane border plus the page padding.
fn left_content(area: Rect) -> Rect {
    let (left, _) = content_halves(area);
    page_area(pane_inner(left))
}

/// Page inset shared by rendering and mouse hit-testing.
pub(crate) fn page_area(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    }
}

/// Split the page into list and search areas, shared by rendering and
/// mouse hit-testing so clicks land where rows are drawn.
pub(crate) fn content_sections(area: Rect, has_status: bool) -> (Rect, Rect) {
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
    (
        sections[if has_status { 1 } else { 0 }],
        sections[sections.len() - 1],
    )
}

pub fn render_library_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
) {
    let (left, right) = content_halves(area);
    render_pane(frame, left, theme);
    render_pane(frame, right, theme);
    let content = left_content(area);

    if let Some(message) = state.load_error() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(message).style(
                ratatui::style::Style::default()
                    .fg(theme.error)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            content,
        );
        return;
    }

    let has_status = state.status_message().is_some();
    let (list_area, search_area) = content_sections(content, has_status);

    if has_status && let Some(message) = state.status_message() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(message).style(
                ratatui::style::Style::default()
                    .fg(theme.text)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Rect {
                x: content.x,
                y: content.y,
                width: content.width,
                height: 1,
            },
        );
    }

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

/// Bordered pane shell shared by both halves; the right one stays empty.
fn render_pane(frame: &mut Frame, area: Rect, theme: &Theme) {
    use ratatui::symbols::border;
    use ratatui::widgets::{Block, Borders};

    let pane = Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(ratatui::style::Style::default().fg(theme.border));
    frame.render_widget(pane, area);
}
