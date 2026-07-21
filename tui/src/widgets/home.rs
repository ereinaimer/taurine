use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Cell, Paragraph, Row, Table},
};
use taurine_core::stats::HomeStats;

use crate::theme::Theme;
use crate::widgets::stat_card;
use crate::widgets::util;

pub fn render_home_content(frame: &mut Frame, area: Rect, theme: &Theme, stats: &HomeStats) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    render_stat_cards(frame, sections[0], theme, stats);

    let activity_sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(sections[2]);

    render_most_used_list(
        frame,
        activity_sections[0],
        theme,
        "TOP AUTOMATIONS",
        &stats.most_used_words,
    );
    render_most_used_list(
        frame,
        activity_sections[2],
        theme,
        "TOP HOTKEYS",
        &stats.most_used_hotkeys,
    );
}

fn render_stat_cards(frame: &mut Frame, area: Rect, theme: &Theme, stats: &HomeStats) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Length(2),
            Constraint::Percentage(34),
            Constraint::Length(2),
            Constraint::Percentage(33),
        ])
        .split(area);

    stat_card::render_stat_card(
        frame,
        sections[0],
        theme,
        "keystrokes saved",
        &util::format_number(stats.keystrokes_saved),
    );
    stat_card::render_stat_card(
        frame,
        sections[2],
        theme,
        "time saved",
        &util::format_time_saved(stats.time_saved_ms),
    );
    stat_card::render_stat_card(
        frame,
        sections[4],
        theme,
        "expansions run",
        &util::format_number(stats.expansions_run),
    );
}

fn render_most_used_list(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    rows: &[taurine_core::stats::MostUsedAutomation],
) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(title).style(
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::BOLD),
        ),
        sections[0],
    );

    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No activity recorded yet.").style(
                Style::default()
                    .fg(theme.text_muted)
                    .add_modifier(Modifier::DIM),
            ),
            sections[2],
        );
        return;
    }

    let header = Row::new([Cell::from(" TRIGGER"), Cell::from("USES ")])
        .style(
            Style::default()
                .fg(theme.text)
                .bg(theme.surface)
                .add_modifier(Modifier::BOLD),
        )
        .height(1);

    let table_rows = rows.iter().take(8).map(|automation| {
        Row::new([
            Cell::from(format!(" {}", automation.trigger)).style(Style::default().fg(theme.text)),
            Cell::from(format!("{} ", util::format_number(automation.uses)))
                .style(Style::default().fg(theme.text_muted)),
        ])
    });

    let table = Table::new(table_rows, [Constraint::Min(15), Constraint::Length(8)])
        .header(header)
        .column_spacing(1);

    frame.render_widget(table, sections[2]);
}
