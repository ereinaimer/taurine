use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::library::state::LibraryPageState;
use crate::widgets::util;

const LIBRARY_ITEM_HEIGHT: u16 = 2;
const LIBRARY_ITEM_GAP: u16 = 1;

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
            y: area.y + (visible_index as u16 * (LIBRARY_ITEM_HEIGHT + LIBRARY_ITEM_GAP)),
            width: area.width,
            height: LIBRARY_ITEM_HEIGHT,
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
    item: &crate::widgets::library::state::LibraryAutomation,
    theme: &Theme,
    selected: bool,
) {
    let row_bg = if selected {
        theme.surface
    } else {
        ratatui::style::Color::Reset
    };
    frame.render_widget(Block::default().style(Style::default().bg(row_bg)), area);

    let kind_width = (item.kind_label().chars().count() as u16).min(area.width.saturating_sub(2));
    let metadata = item.metadata_label();
    let metadata_width = (metadata.chars().count() as u16).min(area.width.saturating_sub(2));

    let top_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let bottom_area = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };

    let top_sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(kind_width)])
        .split(top_area);
    let bottom_sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(metadata_width)])
        .split(bottom_area);

    let trigger_style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(row_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    };
    let kind_style = if selected {
        Style::default().fg(theme.text_muted).bg(row_bg)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let preview_style = if selected {
        Style::default()
            .fg(theme.text_muted)
            .bg(row_bg)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    };

    frame.render_widget(
        Paragraph::new(util::truncate_to_width(
            item.trigger(),
            top_sections[0].width,
        ))
        .style(trigger_style),
        top_sections[0],
    );
    frame.render_widget(
        Paragraph::new(util::truncate_to_width(
            item.kind_label(),
            top_sections[1].width,
        ))
        .alignment(ratatui::layout::Alignment::Right)
        .style(kind_style),
        top_sections[1],
    );
    frame.render_widget(
        Paragraph::new(util::truncate_to_width(
            item.preview(),
            bottom_sections[0].width,
        ))
        .style(preview_style),
        bottom_sections[0],
    );
    frame.render_widget(
        Paragraph::new(util::truncate_to_width(&metadata, bottom_sections[1].width))
            .alignment(ratatui::layout::Alignment::Right)
            .style(preview_style),
        bottom_sections[1],
    );
}
