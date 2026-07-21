use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::util;

#[allow(clippy::too_many_arguments)]
pub fn render_setting_row(
    frame: &mut Frame,
    area: Rect,
    key: &crate::widgets::settings::state::SettingKey,
    settings: &taurine_core::settings::Settings,
    selected: bool,
    spacious: bool,
    control_width: u16,
    theme: &Theme,
) {
    let row_style = if selected {
        Style::default().bg(theme.surface)
    } else {
        Style::default()
    };
    frame.render_widget(Block::default().style(row_style), area);

    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(control_width)])
        .split(area);

    let label_style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    };
    let value_style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };

    frame.render_widget(
        Paragraph::new(key.display_name()).style(label_style),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "[ {} ]",
            util::truncate_to_width(
                &key.display_value(settings),
                control_width.saturating_sub(4)
            )
        ))
        .alignment(Alignment::Right)
        .style(value_style),
        sections[1],
    );

    if spacious && area.height > 1 {
        let description_area = Rect {
            x: sections[0].x,
            y: area.y + 1,
            width: sections[0].width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(key.description()).style(
                Style::default()
                    .fg(theme.text_muted)
                    .bg(if selected {
                        theme.surface
                    } else {
                        ratatui::style::Color::Reset
                    })
                    .add_modifier(Modifier::DIM),
            ),
            description_area,
        );
    }
}
