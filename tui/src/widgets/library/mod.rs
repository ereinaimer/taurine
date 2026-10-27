pub mod actions;
pub mod detail;
pub mod highlight;
pub mod icons;
pub mod list;
pub mod modals;
pub mod props;
pub mod recorder;
pub mod search;
pub mod state;
pub(crate) use actions::*;
pub(crate) use state::*;

#[cfg(test)]
mod tests;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::theme::Theme;
use crate::widgets::toggle::{ToggleAnimations, ToggleId};

pub(crate) use crate::widgets::panes::{
    DEFAULT_DETAIL_RATIO, DEFAULT_SPLIT_RATIO, DIVIDER_HOVER_COLOR, DividerPointer, DividerSide,
    MAX_DETAIL_RATIO, MAX_SPLIT_RATIO, MIN_DETAIL_RATIO, MIN_SPLIT_RATIO, detail_ratio_for_column,
    divider_glyph, divider_hit, split_panes, split_ratio_for_column, visible_divider_columns,
};

#[cfg(test)]
pub(crate) use crate::widgets::panes::{DIVIDER_DOUBLE_CLICK_MS, divider_columns};

/// List content inside the left pane: two cells of padding on the left
/// (the page margin lives outside the split now), two on the right.
/// Shared by rendering and mouse hit-testing.
pub(crate) fn left_content(area: Rect, list_ratio: f32, props_ratio: f32) -> Rect {
    let split = split_panes(area, list_ratio, props_ratio);
    Rect {
        x: split.list.x.saturating_add(2),
        y: split.list.y.saturating_add(1),
        width: split.list.width.saturating_sub(4),
        height: split.list.height.saturating_sub(1),
    }
}

/// Split the page into list and search areas, shared by rendering and
/// mouse hit-testing so clicks land where rows are drawn. The search
/// block grows with the wrapped query up to its cap.
pub(crate) fn content_sections(
    area: Rect,
    has_status: bool,
    field: &crate::widgets::field::TextField,
    is_active: bool,
) -> (Rect, Rect) {
    let search_height = crate::widgets::util::search_block_height(field, is_active, area.width)
        .min(max_search_height(area.height, has_status));
    let sections = if has_status {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(search_height),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(search_height),
            ])
            .split(area)
    };
    (
        sections[if has_status { 1 } else { 0 }],
        sections[sections.len() - 1],
    )
}

/// Cap the search block so a long query never eats the list above it.
fn max_search_height(area_height: u16, has_status: bool) -> u16 {
    const MIN_LIST_HEIGHT: u16 = 3;
    let reserved = u16::from(has_status) + 1 + MIN_LIST_HEIGHT;
    area_height.saturating_sub(reserved).max(3)
}

pub fn render_library_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
    animations: &ToggleAnimations,
    hovered: Option<ToggleId>,
    hovered_add: Option<props::PropsHit>,
) {
    let content = left_content(area, state.split_ratio(), state.detail_ratio());

    if let Some(message) = state.load_error() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(message).style(
                ratatui::style::Style::default()
                    .fg(theme.error)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            content,
        );
        return;
    }

    let has_status = state.status_message().is_some();
    let (list_area, search_area) = content_sections(
        content,
        has_status,
        state.search_field(),
        state.is_search_active(),
    );

    if has_status && let Some(message) = state.status_message() {
        frame.render_widget(
            ratatui::widgets::Paragraph::new(message).style(
                ratatui::style::Style::default()
                    .fg(theme.text)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Rect {
                x: content.x,
                y: content.y,
                width: content.width,
                height: 1,
            },
        );
    }

    list::render_library_list(frame, list_area, theme, state);
    search::render_library_search_bar(
        frame,
        search_area,
        theme,
        state.search_field(),
        state.is_search_active(),
    );
    detail::render_detail(frame, area, theme, state, animations, hovered);
    props::render_props(frame, area, theme, state, animations, hovered, hovered_add);
}
