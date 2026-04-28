use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{List, ListItem, ListState, Paragraph},
};

use crate::app::{App, Page};

const NAV_WIDTH: u16 = 16;

pub(crate) fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, sections[0], app);
    render_body(frame, sections[1], app);
    render_footer_divider(frame, sections[2]);
    render_footer(frame, sections[3]);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let status_width = app.daemon_status().label().len() as u16;
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(status_width)])
        .split(area);

    frame.render_widget(
        Paragraph::new(format!("taurine v{}", env!("CARGO_PKG_VERSION")))
            .style(Style::default().add_modifier(Modifier::BOLD)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(app.daemon_status().label())
            .alignment(Alignment::Right)
            .style(app.daemon_status().style()),
        sections[1],
    );
}

fn render_body(frame: &mut Frame, area: Rect, app: &App) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(NAV_WIDTH),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    render_navigation(frame, sections[0], app);
    render_vertical_divider(frame, sections[1]);
    frame.render_widget(Paragraph::new(app.active_page().title()), sections[2]);
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
                    Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
                ),
                Span::raw(" "),
                Span::raw(page.title()),
            ]);

            ListItem::new(line)
        })
        .collect();

    let navigation = List::new(items)
        .highlight_symbol("")
        .highlight_style(Style::default().bg(Color::DarkGray));
    let mut state = ListState::default();
    state.select(Some(app.active_page().nav_index()));
    frame.render_stateful_widget(navigation, area, &mut state);
}

fn render_vertical_divider(frame: &mut Frame, area: Rect) {
    let lines = vec![Line::raw("|"); area.height as usize];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_footer_divider(frame: &mut Frame, area: Rect) {
    let divider = "-".repeat(area.width as usize);
    frame.render_widget(
        Paragraph::new(divider).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new("q Quit").style(Style::default().add_modifier(Modifier::DIM)),
        area,
    );
}
