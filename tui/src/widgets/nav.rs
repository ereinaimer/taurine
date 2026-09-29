use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{terminal::app::Page, theme::Theme};

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

/// Tab index at terminal cell `(column, row)`, if it lands on a page row
/// inside the navigation borders.
pub fn tab_at(nav_area: Rect, column: u16, row: u16) -> Option<usize> {
    if nav_area.width < 3 {
        return None;
    }
    Page::ALL.iter().enumerate().find_map(|(index, _)| {
        (row == nav_area.y.saturating_add(1 + index as u16)
            && column >= nav_area.x.saturating_add(1)
            && column < nav_area.x.saturating_add(nav_area.width).saturating_sub(1))
        .then_some(index)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    const NAV_AREA: Rect = Rect::new(2, 3, 22, 24);

    #[test]
    fn tab_rows_hit_each_page() {
        assert_eq!(tab_at(NAV_AREA, 5, 4), Some(0));
        assert_eq!(tab_at(NAV_AREA, 5, 5), Some(1));
        assert_eq!(tab_at(NAV_AREA, 5, 6), Some(2));
    }

    #[test]
    fn borders_and_gaps_miss() {
        assert_eq!(tab_at(NAV_AREA, 2, 4), None);
        assert_eq!(tab_at(NAV_AREA, 23, 4), None);
        assert_eq!(tab_at(NAV_AREA, 5, 3), None);
        assert_eq!(tab_at(NAV_AREA, 5, 7), None);
    }

    #[test]
    fn narrow_area_never_hits() {
        assert_eq!(tab_at(Rect::new(2, 3, 2, 24), 3, 4), None);
    }
}
