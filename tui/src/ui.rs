use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Padding, Paragraph, Row, Table},
};
use taurine_core::metrics::{HomeMetrics, MostUsedAutomation};

use crate::{
    app::{App, Page},
    control,
};

const OUTER_HORIZONTAL_PADDING: u16 = 2;
const OUTER_VERTICAL_PADDING: u16 = 1;
const HEADER_GAP_HEIGHT: u16 = 1;
const FOOTER_GAP_HEIGHT: u16 = 1;
const FOOTER_HEIGHT: u16 = 1;
const PANEL_GAP_WIDTH: u16 = 2;
const NAV_WIDTH: u16 = 22;
const PANEL_PADDING: u16 = 1;
const ACCENT_COLOR: Color = Color::White;
const PANEL_BORDER_COLOR: Color = Color::DarkGray;
const MUTED_TEXT_COLOR: Color = Color::Gray;

pub(crate) fn render(frame: &mut Frame, app: &App) {
    let area = frame.area().inner(Margin {
        vertical: OUTER_VERTICAL_PADDING,
        horizontal: OUTER_HORIZONTAL_PADDING,
    });
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(HEADER_GAP_HEIGHT),
            Constraint::Min(0),
            Constraint::Length(FOOTER_GAP_HEIGHT),
            Constraint::Length(FOOTER_HEIGHT),
        ])
        .split(area);

    render_header(frame, sections[0], app);
    render_body(frame, sections[2], app);
    render_footer(frame, sections[4], app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let status_width = app.daemon_status().label().len() as u16;
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(status_width)])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "taurine",
                Style::default()
                    .fg(ACCENT_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("v{}", env!("CARGO_PKG_VERSION")),
                Style::default()
                    .fg(MUTED_TEXT_COLOR)
                    .add_modifier(Modifier::DIM),
            ),
        ])),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(app.daemon_status().label())
            .alignment(Alignment::Right)
            .style(app.daemon_status().style().add_modifier(Modifier::BOLD)),
        sections[1],
    );
}

fn render_body(frame: &mut Frame, area: Rect, app: &App) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(NAV_WIDTH),
            Constraint::Length(PANEL_GAP_WIDTH),
            Constraint::Min(0),
        ])
        .split(area);

    render_navigation(frame, sections[0], app);
    render_content(frame, sections[2], app);
}

fn render_navigation(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = Page::ALL
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let shortcut = char::from_digit((index + 1) as u32, 10).unwrap_or(' ');
            let line = Line::from(vec![
                Span::styled(
                    shortcut.to_string(),
                    Style::default()
                        .fg(MUTED_TEXT_COLOR)
                        .add_modifier(Modifier::DIM),
                ),
                Span::raw(" "),
                Span::raw(page.title()),
            ]);

            ListItem::new(line)
        })
        .collect();

    let navigation_block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(PANEL_BORDER_COLOR))
        .padding(Padding::new(
            PANEL_PADDING,
            PANEL_PADDING,
            PANEL_PADDING,
            PANEL_PADDING,
        ));

    let navigation = List::new(items)
        .block(navigation_block)
        .highlight_symbol("")
        .highlight_style(
            Style::default()
                .bg(PANEL_BORDER_COLOR)
                .fg(ACCENT_COLOR)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    state.select(Some(app.active_page().nav_index()));
    frame.render_stateful_widget(navigation, area, &mut state);
}

fn render_content(frame: &mut Frame, area: Rect, app: &App) {
    let content_block = Block::default()
        .title_alignment(Alignment::Right)
        .title(Span::styled(
            format!(" {} ", app.active_page().title()),
            Style::default()
                .fg(ACCENT_COLOR)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(PANEL_BORDER_COLOR))
        .padding(Padding::new(
            PANEL_PADDING,
            PANEL_PADDING,
            PANEL_PADDING,
            PANEL_PADDING,
        ));

    let inner = content_block.inner(area);
    frame.render_widget(content_block, area);

    match app.active_page() {
        Page::Home => render_home_content(frame, inner, app.home_metrics()),
        Page::Library | Page::Settings => {}
    }
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let footer_text = match app.active_page() {
        Page::Home => control::home_footer_label(app.daemon_status()),
        Page::Library | Page::Settings => "q Quit",
    };

    let footer_line = Line::from(vec![Span::styled(
        footer_text,
        Style::default()
            .fg(MUTED_TEXT_COLOR)
            .add_modifier(Modifier::DIM),
    )]);

    frame.render_widget(Paragraph::new(footer_line).alignment(Alignment::Left), area);
}

fn render_home_content(frame: &mut Frame, area: Rect, metrics: &HomeMetrics) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new("All-time usage").style(
            Style::default()
                .fg(ACCENT_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        sections[0],
    );

    render_metric_cards(frame, sections[2], metrics);

    let tables_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(PANEL_GAP_WIDTH),
            Constraint::Percentage(50),
        ])
        .split(sections[4]);

    render_most_used_table(
        frame,
        tables_layout[0],
        "Most used automations",
        &metrics.most_used_words,
    );
    render_most_used_table(
        frame,
        tables_layout[2],
        "Most used hotkeys",
        &metrics.most_used_hotkeys,
    );
}

fn render_metric_cards(frame: &mut Frame, area: Rect, metrics: &HomeMetrics) {
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

    render_metric_card(
        frame,
        sections[0],
        "Keystrokes saved",
        &format_number(metrics.keystrokes_saved),
    );
    render_metric_card(
        frame,
        sections[2],
        "Time saved",
        &format_time_saved(metrics.time_saved_ms),
    );
    render_metric_card(
        frame,
        sections[4],
        "Expansions run",
        &format_number(metrics.expansions_run),
    );
}

fn render_metric_card(frame: &mut Frame, area: Rect, label: &str, value: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(PANEL_BORDER_COLOR))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(MUTED_TEXT_COLOR)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(value).style(
            Style::default()
                .fg(ACCENT_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        sections[1],
    );
}

fn render_most_used_table(frame: &mut Frame, area: Rect, title: &str, rows: &[MostUsedAutomation]) {
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
                .fg(ACCENT_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        sections[0],
    );

    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No usage recorded yet.").style(Style::default().fg(MUTED_TEXT_COLOR)),
            sections[2],
        );
        return;
    }

    let header = Row::new([Cell::from("Trigger"), Cell::from("Uses")]).style(
        Style::default()
            .fg(MUTED_TEXT_COLOR)
            .add_modifier(Modifier::BOLD),
    );

    let table_rows = rows.iter().take(3).map(|automation| {
        Row::new([
            Cell::from(automation.trigger.clone()),
            Cell::from(format_number(automation.uses)),
        ])
    });

    let table = Table::new(table_rows, [Constraint::Min(10), Constraint::Length(8)])
        .header(header)
        .column_spacing(2);

    frame.render_widget(table, sections[2]);
}

fn format_number(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }

    formatted.chars().rev().collect()
}

fn format_time_saved(time_saved_ms: u64) -> String {
    let total_minutes = time_saved_ms / 60_000;
    let total_hours = total_minutes / 60;
    let total_days = total_hours / 24;

    if total_days > 0 {
        let remaining_hours = total_hours % 24;
        if remaining_hours > 0 {
            format!("{total_days}d {remaining_hours}h")
        } else {
            format!("{total_days}d")
        }
    } else if total_hours > 0 {
        let remaining_minutes = total_minutes % 60;
        if remaining_minutes > 0 {
            format!("{total_hours}h {remaining_minutes}m")
        } else {
            format!("{total_hours}h")
        }
    } else {
        format!("{total_minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_zero_time_saved_as_zero_minutes() {
        assert_eq!(format_time_saved(0), "0m");
    }

    #[test]
    fn formats_one_hour_without_trailing_minutes() {
        assert_eq!(format_time_saved(3_600_000), "1h");
    }

    #[test]
    fn formats_one_minute() {
        assert_eq!(format_time_saved(60_000), "1m");
    }

    #[test]
    fn formats_hours_and_minutes() {
        assert_eq!(format_time_saved(3_660_000), "1h 1m");
    }
}
