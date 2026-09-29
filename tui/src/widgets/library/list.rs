use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::library::state::LibraryPageState;
use crate::widgets::util;

const LIBRARY_ITEM_HEIGHT: u16 = 2;
const LIBRARY_ITEM_PADDING: u16 = 1;

pub fn render_library_list(frame: &mut Frame, area: Rect, theme: &Theme, state: &LibraryPageState) {
    if let Some(message) = state.load_error() {
        frame.render_widget(
            Paragraph::new(message).style(
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            area,
        );
        return;
    }

    if area.height == 0 {
        return;
    }

    if let Some(message) = state.empty_state_message() {
        frame.render_widget(
            Paragraph::new(message).style(
                Style::default()
                    .fg(theme.text_muted)
                    .add_modifier(Modifier::DIM),
            ),
            area,
        );
        return;
    }

    let visible_count = util::visible_library_item_capacity(area.height);
    if visible_count == 0 {
        return;
    }

    let selected_index = state.selected_index().unwrap_or(0);
    let (start, end) = util::visible_range(state.filtered_len(), selected_index, visible_count);

    for (visible_index, filtered_index) in (start..end).enumerate() {
        let row_area = Rect {
            x: area.x,
            y: area.y + (visible_index as u16 * (LIBRARY_ITEM_HEIGHT + 2 * LIBRARY_ITEM_PADDING)),
            width: area.width,
            height: LIBRARY_ITEM_HEIGHT + 2 * LIBRARY_ITEM_PADDING,
        };

        let Some(item) = state.item_at_filtered(filtered_index) else {
            continue;
        };

        render_library_item(
            frame,
            row_area,
            item,
            theme,
            state.selected_index() == Some(filtered_index),
        );
    }
}

fn render_library_item(
    frame: &mut Frame,
    area: Rect,
    item: &crate::widgets::library::state::LibraryTrigger,
    theme: &Theme,
    selected: bool,
) {
    let row_style = if selected {
        Style::default().bg(theme.surface)
    } else {
        Style::default()
    };
    frame.render_widget(Block::default().style(row_style), area);

    let content = Rect {
        x: area.x.saturating_add(LIBRARY_ITEM_PADDING),
        y: area.y.saturating_add(LIBRARY_ITEM_PADDING),
        width: area.width.saturating_sub(2 * LIBRARY_ITEM_PADDING),
        height: area.height.saturating_sub(2 * LIBRARY_ITEM_PADDING),
    };
    if content.width == 0 || content.height == 0 {
        return;
    }

    let top_area = Rect {
        x: content.x,
        y: content.y,
        width: content.width,
        height: 1,
    };
    let bottom_area = Rect {
        x: content.x,
        y: content.y + 1,
        width: content.width,
        height: 1,
    };

    let trigger_style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    };
    let preview_style = if selected {
        Style::default().fg(theme.description).bg(theme.surface)
    } else {
        Style::default().fg(theme.description)
    };

    frame.render_widget(
        Paragraph::new(util::truncate_to_width(item.trigger(), top_area.width))
            .style(trigger_style),
        top_area,
    );
    frame.render_widget(
        Paragraph::new(util::truncate_to_width(item.preview(), bottom_area.width))
            .style(preview_style),
        bottom_area,
    );
}
