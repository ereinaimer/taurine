use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{app::Page, theme::Theme};

pub fn render_navigation(frame: &mut Frame, area: Rect, theme: &Theme, active_page: Page) {
    let items: Vec<ListItem> = Page::ALL
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let shortcut = char::from_digit((index + 1) as u32, 10).unwrap_or(' ');
            let line = Line::from(vec![
                Span::styled(
                    shortcut.to_string(),
                    Style::default()
                        .fg(theme.text_muted)
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
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(Style::default().fg(theme.border));

    let navigation = List::new(items)
        .block(navigation_block)
        .highlight_symbol("")
        .highlight_style(
            Style::default()
                .bg(theme.surface)
                .fg(theme.text)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    state.select(Some(active_page.nav_index()));
    frame.render_stateful_widget(navigation, area, &mut state);
}
