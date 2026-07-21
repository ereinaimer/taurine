use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;

pub fn render_stat_card(frame: &mut Frame, area: Rect, theme: &Theme, label: &str, value: &str) {
    let block = Block::default().style(Style::default().bg(theme.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sections = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(value).style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(theme.text_muted)),
        sections[1],
    );
}
