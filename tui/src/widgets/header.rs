use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{terminal::status::DaemonStatus, theme::Theme};

pub struct HeaderWidget<'a> {
    pub theme: &'a Theme,
    pub daemon_status: DaemonStatus,
}

impl Widget for HeaderWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let status_width = self.daemon_status.label().len() as u16;
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(status_width)])
            .split(area);

        Paragraph::new(Line::from(vec![
            Span::styled(
                "Taurine",
                Style::default()
                    .fg(self.theme.text)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("v{}", env!("CARGO_PKG_VERSION")),
                Style::default()
                    .fg(self.theme.text_muted)
                    .add_modifier(Modifier::DIM),
            ),
        ]))
        .render(sections[0], buf);

        Paragraph::new(self.daemon_status.label())
            .alignment(ratatui::layout::Alignment::Right)
            .style(self.daemon_status.style().add_modifier(Modifier::BOLD))
            .render(sections[1], buf);
    }
}
