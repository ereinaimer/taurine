pub mod actions;
pub mod detail;
pub mod icons;
pub mod list;
pub mod modals;
pub mod search;
pub mod state;
pub(crate) use actions::*;
pub(crate) use state::*;

#[cfg(test)]
mod tests;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::theme::Theme;

pub(crate) const DEFAULT_SPLIT_RATIO: f32 = 0.5;
pub(crate) const MIN_SPLIT_RATIO: f32 = 0.15;
pub(crate) const MAX_SPLIT_RATIO: f32 = 0.85;
/// Divider highlight on hover and while dragging: one small step above the
/// `#222222` border so the affordance stays subtle.
pub(crate) const DIVIDER_HOVER_COLOR: ratatui::style::Color =
    ratatui::style::Color::Rgb(0x2E, 0x2E, 0x2E);

/// Minimum right-pane width for the detail preview to stay readable.
/// Narrower than this collapses to the list alone.
pub(crate) const MIN_RIGHT_WIDTH: u16 = 22;
/// Minimum left-pane width honored while dragging the divider.
pub(crate) const MIN_LEFT_WIDTH: u16 = 20;

/// Divider column between the two panes for a full-frame line. None when
/// collapsed to a single pane.
pub(crate) fn divider_column(area: Rect, ratio: f32) -> Option<u16> {
    let (left, right) = content_halves(area, ratio);
    if right.width == 0 {
        return None;
    }
    Some(left.x.saturating_add(left.width))
}

/// Whether a click at `(column, row)` grabs the divider for resizing.
pub(crate) fn divider_hit(area: Rect, ratio: f32, column: u16, row: u16) -> bool {
    let Some(divider) = divider_column(area, ratio) else {
        return false;
    };
    column == divider && row >= area.y && row < area.y.saturating_add(area.height)
}

/// Split ratio that puts the divider gutter at `column`, keeping both
/// panes above their minimum widths while the terminal allows it.
pub(crate) fn split_ratio_for_column(area: Rect, column: u16) -> f32 {
    let gutter = area.width.saturating_sub(1).max(1) as f32;
    let ratio = column.saturating_sub(area.x) as f32 / gutter;
    let span = area.width.saturating_sub(1) as f32;
    if span <= 0.0 {
        return DEFAULT_SPLIT_RATIO;
    }
    let low = (MIN_LEFT_WIDTH as f32 / span).max(MIN_SPLIT_RATIO);
    let high = ((span - MIN_RIGHT_WIDTH as f32) / span).min(MAX_SPLIT_RATIO);
    if low > high {
        return DEFAULT_SPLIT_RATIO;
    }
    ratio.clamp(low, high)
}

/// Split the page into left (list) and right (detail) panes with a
/// one-column gutter, shared by rendering and mouse hit-testing.
/// Collapses to the list alone when the detail pane would fall below
/// its readable minimum.
pub(crate) fn content_halves(area: Rect, ratio: f32) -> (Rect, Rect) {
    if area.width < 5 {
        return (area, Rect::default());
    }
    let ratio = ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO);
    let span = area.width.saturating_sub(1);
    let left_width = ((span as f32 * ratio) as u16).clamp(1, span.saturating_sub(1).max(1));
    if span.saturating_sub(left_width) < MIN_RIGHT_WIDTH {
        return (area, Rect::default());
    }
    (
        Rect {
            x: area.x,
            y: area.y,
            width: left_width,
            height: area.height,
        },
        Rect {
            x: area.x.saturating_add(left_width).saturating_add(1),
            y: area.y,
            width: area.width.saturating_sub(left_width).saturating_sub(1),
            height: area.height,
        },
    )
}

/// List content inside the left pane: flush left, two cells of padding on
/// the right. Shared by rendering and mouse hit-testing.
fn left_content(area: Rect, ratio: f32) -> Rect {
    let (left, _) = content_halves(area, ratio);
    Rect {
        x: left.x,
        y: left.y.saturating_add(1),
        width: left.width.saturating_sub(2),
        height: left.height.saturating_sub(1),
    }
}

/// Page inset shared by rendering and mouse hit-testing.
pub(crate) fn page_area(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    }
}

/// Split the page into list and search areas, shared by rendering and
/// mouse hit-testing so clicks land where rows are drawn.
pub(crate) fn content_sections(area: Rect, has_status: bool) -> (Rect, Rect) {
    let sections = if has_status {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(area)
    };
    (
        sections[if has_status { 1 } else { 0 }],
        sections[sections.len() - 1],
    )
}

pub fn render_library_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
) {
    let content = left_content(area, state.split_ratio());

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
    let (list_area, search_area) = content_sections(content, has_status);

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
        state.search_query(),
        state.is_search_active(),
        state.search_query().chars().count(),
    );
    detail::render_detail(frame, area, theme, state);
}
