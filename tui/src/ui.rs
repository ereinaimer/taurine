use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
};

use crate::app::{App, Page};

const OUTER_HORIZONTAL_PADDING: u16 = 2;
const OUTER_VERTICAL_PADDING: u16 = 1;
const HEADER_GAP_HEIGHT: u16 = 1;
const FOOTER_GAP_HEIGHT: u16 = 1;
const FOOTER_HEIGHT: u16 = 2;
const NAV_WIDTH: u16 = 20;
const CONTENT_LEFT_PADDING: u16 = 2;

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
    render_footer(frame, sections[4]);
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
        .constraints([Constraint::Length(NAV_WIDTH), Constraint::Min(0)])
        .split(area);

    render_navigation(frame, sections[0], app);
    render_content(frame, sections[1], app);
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

fn render_content(frame: &mut Frame, area: Rect, app: &App) {
    let content_block = Block::default()
        .borders(Borders::LEFT)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(Color::DarkGray))
        .padding(Padding::new(CONTENT_LEFT_PADDING, 0, 0, 0));

    frame.render_widget(
        Paragraph::new(app.active_page().title()).block(content_block),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let footer_block = Block::default()
        .borders(Borders::TOP)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(Color::DarkGray));

    frame.render_widget(
        Paragraph::new("q Quit")
            .block(footer_block)
            .style(Style::default().add_modifier(Modifier::DIM)),
        area,
    );
}
