use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
};

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

    frame.render_widget(Paragraph::new("").block(content_block), area);
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
