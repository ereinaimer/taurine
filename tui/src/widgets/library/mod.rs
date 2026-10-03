pub mod actions;
pub mod detail;
pub mod icons;
pub mod list;
pub mod modals;
pub mod props;
pub mod search;
pub mod state;
pub(crate) use actions::*;
pub(crate) use state::*;

#[cfg(test)]
mod tests;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::theme::Theme;

pub(crate) const DEFAULT_SPLIT_RATIO: f32 = 2.0 / 7.0;
pub(crate) const DEFAULT_DETAIL_RATIO: f32 = 0.4;
pub(crate) const MIN_SPLIT_RATIO: f32 = 0.15;
pub(crate) const MAX_SPLIT_RATIO: f32 = 0.85;
pub(crate) const MIN_DETAIL_RATIO: f32 = 0.15;
pub(crate) const MAX_DETAIL_RATIO: f32 = 0.60;
/// Divider highlight on hover and while dragging: one small step above the
/// `#222222` border so the affordance stays subtle.
pub(crate) const DIVIDER_HOVER_COLOR: ratatui::style::Color =
    ratatui::style::Color::Rgb(0x2E, 0x2E, 0x2E);

/// Minimum pane widths. Below 76 columns the props pane drops; below
/// 43 the center drops too, leaving the list alone.
pub(crate) const MIN_LEFT_WIDTH: u16 = 20;
pub(crate) const MIN_CENTER_WIDTH: u16 = 30;
pub(crate) const MIN_COMPACT_CENTER_WIDTH: u16 = 22;
pub(crate) const MIN_PROPS_WIDTH: u16 = 24;

/// Which gutter divider a pointer action targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DividerSide {
    List,
    Props,
}

/// Three-pane split: list, center content, right properties. Missing
/// panes come back as empty rects with no dividers.
pub(crate) struct PaneSplit {
    pub(crate) list: Rect,
    pub(crate) center: Rect,
    pub(crate) props: Rect,
    pub(crate) dividers: Vec<u16>,
}

pub(crate) fn split_panes(area: Rect, list_ratio: f32, props_ratio: f32) -> PaneSplit {
    let empty = || PaneSplit {
        list: area,
        center: Rect::default(),
        props: Rect::default(),
        dividers: Vec::new(),
    };
    if area.width < 5 {
        return empty();
    }
    // Three panes: list + gutter + center + gutter + props.
    if area.width >= MIN_LEFT_WIDTH + MIN_CENTER_WIDTH + MIN_PROPS_WIDTH + 2 {
        let span = area.width.saturating_sub(2);
        let max_left = span.saturating_sub(MIN_CENTER_WIDTH + MIN_PROPS_WIDTH);
        let left_width = ((span as f32 * list_ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO))
            as u16)
            .clamp(MIN_LEFT_WIDTH, max_left.max(MIN_LEFT_WIDTH));
        let rest = span.saturating_sub(left_width);
        let max_right = rest.saturating_sub(MIN_CENTER_WIDTH);
        let right_width = ((rest as f32 * props_ratio.clamp(MIN_DETAIL_RATIO, MAX_DETAIL_RATIO))
            as u16)
            .clamp(MIN_PROPS_WIDTH, max_right.max(MIN_PROPS_WIDTH));
        let center_width = rest.saturating_sub(right_width);
        let list = Rect {
            x: area.x,
            y: area.y,
            width: left_width,
            height: area.height,
        };
        let center = Rect {
            x: area.x.saturating_add(left_width).saturating_add(1),
            y: area.y,
            width: center_width,
            height: area.height,
        };
        let props = Rect {
            x: center.x.saturating_add(center_width).saturating_add(1),
            y: area.y,
            width: right_width,
            height: area.height,
        };
        let dividers = vec![
            list.x.saturating_add(list.width),
            center.x.saturating_add(center.width),
        ];
        return PaneSplit {
            list,
            center,
            props,
            dividers,
        };
    }
    // Two panes: list + center.
    if area.width > MIN_LEFT_WIDTH + MIN_COMPACT_CENTER_WIDTH {
        let span = area.width.saturating_sub(1);
        let max_left = span.saturating_sub(MIN_COMPACT_CENTER_WIDTH);
        let left_width = ((span as f32 * list_ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO))
            as u16)
            .clamp(MIN_LEFT_WIDTH, max_left.max(MIN_LEFT_WIDTH));
        let list = Rect {
            x: area.x,
            y: area.y,
            width: left_width,
            height: area.height,
        };
        let center = Rect {
            x: area.x.saturating_add(left_width).saturating_add(1),
            y: area.y,
            width: area.width.saturating_sub(left_width).saturating_sub(1),
            height: area.height,
        };
        let dividers = vec![list.x.saturating_add(list.width)];
        return PaneSplit {
            list,
            center,
            props: Rect::default(),
            dividers,
        };
    }
    empty()
}

/// Divider gutter columns for a full-frame line. Empty when collapsed
/// to a single pane.
pub(crate) fn divider_columns(area: Rect, list_ratio: f32, props_ratio: f32) -> Vec<u16> {
    split_panes(area, list_ratio, props_ratio).dividers
}

/// Whether a click at `(column, row)` grabs a divider, and which one.
pub(crate) fn divider_hit(
    area: Rect,
    list_ratio: f32,
    props_ratio: f32,
    column: u16,
    row: u16,
) -> Option<DividerSide> {
    if row < area.y || row >= area.y.saturating_add(area.height) {
        return None;
    }
    let split = split_panes(area, list_ratio, props_ratio);
    if split.dividers.first() == Some(&column) {
        return Some(DividerSide::List);
    }
    if split.dividers.get(1) == Some(&column) {
        return Some(DividerSide::Props);
    }
    None
}

/// List share that puts the list divider gutter at `column`, keeping
/// minimum widths while the terminal allows it.
pub(crate) fn split_ratio_for_column(area: Rect, column: u16) -> f32 {
    let gutter = area.width.saturating_sub(1).max(1) as f32;
    let ratio = column.saturating_sub(area.x) as f32 / gutter;
    ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
}

/// Props share of the remainder that puts the props divider at `column`.
pub(crate) fn detail_ratio_for_column(area: Rect, list_ratio: f32, column: u16) -> f32 {
    let split = split_panes(area, list_ratio, DEFAULT_DETAIL_RATIO);
    let rest = split.center.width.saturating_add(split.props.width);
    if rest == 0 {
        return DEFAULT_DETAIL_RATIO;
    }
    let rest_end = split.center.x.saturating_add(rest);
    let right_width = rest_end.saturating_sub(column.saturating_add(1));
    (right_width as f32 / rest as f32).clamp(MIN_DETAIL_RATIO, MAX_DETAIL_RATIO)
}

/// List content inside the left pane: flush left, two cells of padding on
/// the right. Shared by rendering and mouse hit-testing.
pub(crate) fn left_content(area: Rect, list_ratio: f32, props_ratio: f32) -> Rect {
    let split = split_panes(area, list_ratio, props_ratio);
    Rect {
        x: split.list.x,
        y: split.list.y.saturating_add(1),
        width: split.list.width.saturating_sub(2),
        height: split.list.height.saturating_sub(1),
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
        state.search_field(),
        state.is_search_active(),
    );
    detail::render_detail(frame, area, theme, state);
    props::render_props(frame, area, theme, state);
}
