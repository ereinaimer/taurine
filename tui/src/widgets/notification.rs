use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::theme::Theme;

pub fn render_notification(frame: &mut Frame, area: Rect, theme: &Theme, message: &str) {
    let width = message.len().min(60) as u16 + 4;
    let height = 3;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + area.height.saturating_sub(height + 1);
    let popup = Rect {
        x,
        y,
        width,
        height,
    };

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(Span::styled(
            " Notification ",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface));
    frame.render_widget(
        Paragraph::new(message)
            .block(block)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Center),
        popup,
    );
}
