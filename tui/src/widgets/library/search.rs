use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::util;

pub fn render_library_search_bar(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    query: &str,
    is_active: bool,
    cursor: usize,
) {
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface)),
        area,
    );

    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(area);

    let label_style = if is_active {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text_muted)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    };
    frame.render_widget(Paragraph::new("Search").style(label_style), sections[0]);

    let query_style = if query.is_empty() && !is_active {
        Style::default()
            .fg(theme.text_muted)
            .bg(theme.surface)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(theme.text).bg(theme.surface)
    };
    let query_text = if is_active {
        util::input_cursor_line(query, cursor)
    } else if query.is_empty() {
        Line::from(" ")
    } else {
        Line::from(query.to_string())
    };
    frame.render_widget(Paragraph::new(query_text).style(query_style), sections[1]);
}
