//! Shared pane geometry and divider chrome.
//!
//! The library workspace and the home dashboard split their content the
//! same way: proportional panes with a one-column gutter that runs the
//! full terminal height, lifts on hover or drag, and resets on a
//! double-click.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Paragraph,
};

use crate::theme::Theme;

pub(crate) const DEFAULT_SPLIT_RATIO: f32 = 2.0 / 7.0;
pub(crate) const DEFAULT_DETAIL_RATIO: f32 = 0.4;
pub(crate) const MIN_SPLIT_RATIO: f32 = 0.0;
pub(crate) const MAX_SPLIT_RATIO: f32 = 1.0;
pub(crate) const MIN_DETAIL_RATIO: f32 = 0.0;
/// Ceiling for the props share of the remainder. Above 1.0 the want
/// exceeds the remainder so deep drags keep stealing toward a full
/// park; ordinary ratios stay below 1.0.
pub(crate) const MAX_DETAIL_RATIO: f32 = 2.0;
/// Divider highlight on hover and while dragging: barely above the
/// `#1a1a1a` border so the affordance stays whisper-quiet.
pub(crate) const DIVIDER_HOVER_COLOR: Color = Color::Rgb(0x22, 0x22, 0x22);

/// Double-click window for divider reset.
pub(crate) const DIVIDER_DOUBLE_CLICK_MS: u64 = 300;

/// Panes have no minimum widths: dragging a divider to an edge parks
/// the pane at zero. The Properties divider stays on the outer edge so
/// it can be dragged back; a collapsed left pane hands its boundary to
/// the navigation rail divider instead of keeping a ghost gutter. Each
/// side pane has its own maximum width; tiny terminals fall back to the
/// list.
pub(crate) const MAX_LIST_WIDTH: u16 = 52;
pub(crate) const MAX_PROPS_WIDTH: u16 = 40;

/// Which gutter divider a pointer action targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DividerSide {
    List,
    Props,
}

/// Pointer state shared by every page with resizable gutters: which
/// divider is hovered, which is being dragged, and the last press for
/// double-click resets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DividerPointer {
    hover: Option<DividerSide>,
    drag: Option<DividerSide>,
    last_click: Option<(DividerSide, u64)>,
}

impl DividerPointer {
    pub(crate) const fn hover(&self) -> Option<DividerSide> {
        self.hover
    }

    pub(crate) const fn drag(&self) -> Option<DividerSide> {
        self.drag
    }

    pub(crate) fn set_hover(&mut self, hover: Option<DividerSide>) {
        self.hover = hover;
    }

    pub(crate) fn set_drag(&mut self, drag: Option<DividerSide>) {
        self.drag = drag;
        if drag.is_some() {
            self.hover = drag;
        }
    }

    pub(crate) const fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Record a press and report whether it lands inside the double-click
    /// window of the previous press on the same divider.
    pub(crate) fn double_click_at(&mut self, side: DividerSide, now: u64) -> bool {
        let double = self.last_click.is_some_and(|(last, at)| {
            last == side && now.saturating_sub(at) <= DIVIDER_DOUBLE_CLICK_MS
        });
        self.last_click = if double { None } else { Some((side, now)) };
        double
    }

    /// Line color: a hovered or dragged gutter lifts above the border.
    pub(crate) fn color(&self, theme: &Theme, side: DividerSide) -> Color {
        if self.drag() == Some(side) || self.hover() == Some(side) {
            DIVIDER_HOVER_COLOR
        } else {
            theme.border
        }
    }
}

/// Three-pane split: list, center content, right properties. Missing
/// panes come back as empty rects with no dividers.
pub(crate) struct PaneSplit {
    pub(crate) list: Rect,
    pub(crate) center: Rect,
    pub(crate) props: Rect,
    pub(crate) list_divider: Option<u16>,
    pub(crate) props_divider: Option<u16>,
}

pub(crate) fn split_panes(area: Rect, list_ratio: f32, props_ratio: f32) -> PaneSplit {
    let empty = || PaneSplit {
        list: area,
        center: Rect::default(),
        props: Rect::default(),
        list_divider: None,
        props_divider: None,
    };
    if area.width < 5 {
        return empty();
    }
    // Three panes: list + gutter + center + gutter + props. The side
    // maxima are overflow points, not walls: a side wanting more than
    // its maximum keeps growing by shrinking the far pane first, then
    // the middle, so drags glide past the cap instead of snapping.
    // Extreme ratios flow through the same redistribution, so either
    // side can park fully open with no edge-case snap.
    let span = area.width.saturating_sub(2);
    let list_wanted =
        ((span as f32 * list_ratio.clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)) as u16).min(span);
    let left_capped = list_wanted.min(MAX_LIST_WIDTH);
    let rest = span.saturating_sub(left_capped);
    // honey: props want is intentionally uncapped at rest so deep
    // drags keep stealing toward a full park.
    let props_wanted = (rest as f32 * props_ratio.clamp(MIN_DETAIL_RATIO, MAX_DETAIL_RATIO)) as u16;
    let right_capped = props_wanted.min(MAX_PROPS_WIDTH).min(rest);
    let center_natural = rest.saturating_sub(right_capped);
    // honey: left overflow eats props first, then center.
    let mut left_width = left_capped;
    let mut right_width = right_capped;
    let mut center_width = center_natural;
    let mut overflow = list_wanted.saturating_sub(left_capped);
    let take = overflow.min(right_width);
    right_width = right_width.saturating_sub(take);
    overflow = overflow.saturating_sub(take);
    let take = overflow.min(center_width);
    center_width = center_width.saturating_sub(take);
    overflow = overflow.saturating_sub(take);
    left_width = left_width.saturating_add(
        list_wanted
            .saturating_sub(left_capped)
            .saturating_sub(overflow),
    );
    // honey: right overflow eats list first, then center. Overflow
    // measures against the fair share so it never reclaims cells
    // the left phase already redistributed.
    let mut overflow = props_wanted.saturating_sub(right_capped);
    let take = overflow.min(left_width);
    left_width = left_width.saturating_sub(take);
    overflow = overflow.saturating_sub(take);
    let take = overflow.min(center_width);
    center_width = center_width.saturating_sub(take);
    overflow = overflow.saturating_sub(take);
    right_width = right_width.saturating_add(
        props_wanted
            .saturating_sub(right_capped)
            .saturating_sub(overflow),
    );
    // honey: a lone pane absorbs its dead gutter so the surviving
    // divider sits exactly on the terminal edge instead of leaving
    // a background cell after it.
    let left_parked = center_width == 0 && right_width == 0 && left_width > 0;
    let right_parked = left_width == 0 && center_width == 0 && right_width > 0;
    if left_parked {
        left_width = area.width.saturating_sub(1);
    }
    // honey: a collapsed left pane has no gutter of its own. The nav
    // rail divider is the single boundary, so the center starts flush
    // against it instead of leaving a ghost column.
    if left_width == 0 && center_width > 0 {
        center_width = center_width.saturating_add(1);
    }
    let list = Rect {
        x: area.x,
        y: area.y,
        width: left_width,
        height: area.height,
    };
    let center_x = if left_width == 0 {
        area.x
    } else {
        list.x.saturating_add(list.width).saturating_add(1)
    };
    let center = Rect {
        x: center_x,
        y: area.y,
        width: center_width,
        height: area.height,
    };
    let props = Rect {
        x: center.x.saturating_add(center.width).saturating_add(1),
        y: area.y,
        width: if right_parked {
            area.width.saturating_sub(1)
        } else {
            right_width
        },
        height: area.height,
    };
    let list_divider = (list.width > 0).then_some(list.x.saturating_add(list.width));
    let props_divider =
        (center.width > 0 || props.width > 0).then_some(center.x.saturating_add(center.width));
    PaneSplit {
        list,
        center,
        props,
        list_divider,
        props_divider,
    }
}

/// Divider gutter columns that currently exist, for layout tests.
#[cfg(test)]
pub(crate) fn divider_columns(area: Rect, list_ratio: f32, props_ratio: f32) -> Vec<u16> {
    let split = split_panes(area, list_ratio, props_ratio);
    split
        .list_divider
        .into_iter()
        .chain(split.props_divider)
        .collect()
}

/// Divider columns worth painting, each tagged with its side.
pub(crate) fn visible_divider_columns(
    area: Rect,
    list_ratio: f32,
    props_ratio: f32,
) -> Vec<(DividerSide, u16)> {
    let split = split_panes(area, list_ratio, props_ratio);
    let mut shown = Vec::new();
    if let Some(column) = split.list_divider {
        shown.push((DividerSide::List, column));
    }
    if let Some(column) = split.props_divider {
        shown.push((DividerSide::Props, column));
    }
    shown
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
    if split.list_divider == Some(column) {
        return Some(DividerSide::List);
    }
    if split.props_divider == Some(column) {
        return Some(DividerSide::Props);
    }
    None
}

/// List share that puts the list divider gutter at `column`. At zero the
/// pane collapses, its divider disappears, and the center takes the
/// boundary from the navigation rail.
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

/// Vertical divider glyph for one row. On the content-rule row the
/// divider grows an arm toward the center pane so the two lines meet.
pub(crate) fn divider_glyph(
    side: DividerSide,
    row: u16,
    content_divider_row: Option<u16>,
) -> &'static str {
    if content_divider_row == Some(row) {
        match side {
            DividerSide::List => "├",
            DividerSide::Props => "┤",
        }
    } else {
        "│"
    }
}

/// Paint one gutter column from the terminal's top edge to its bottom
/// edge, so the line never stops at the page margins.
pub(crate) fn render_divider(
    frame: &mut Frame,
    area: Rect,
    column: u16,
    color: Color,
    glyph: impl Fn(u16) -> &'static str,
) {
    let glyphs = (area.y..area.bottom())
        .map(|row| Line::from(glyph(row)))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(glyphs).style(Style::default().fg(color)),
        Rect {
            x: column,
            y: area.y,
            width: 1,
            height: area.height,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_parks_a_side_pane_on_the_terminal_edge() {
        let area = Rect::new(0, 0, 80, 20);
        let parked = split_panes(area, 0.0, 2.0);
        assert_eq!(parked.center.width, 0);
        assert_eq!(parked.props_divider, Some(0));
        assert_eq!(parked.props.width, area.width - 1);
    }

    #[test]
    fn double_click_window_is_per_divider() {
        let mut pointer = DividerPointer::default();
        assert!(!pointer.double_click_at(DividerSide::List, 1_000));
        assert!(!pointer.double_click_at(DividerSide::Props, 1_100));
        assert!(!pointer.double_click_at(DividerSide::List, 1_400));
        assert!(pointer.double_click_at(DividerSide::List, 1_450));
        assert!(!pointer.double_click_at(DividerSide::List, 1_451));
    }

    #[test]
    fn hover_and_drag_lift_only_their_own_divider() {
        let theme = crate::theme::default::DARK_THEME;
        let mut pointer = DividerPointer::default();
        assert_eq!(pointer.color(&theme, DividerSide::List), theme.border);

        pointer.set_hover(Some(DividerSide::List));
        assert_eq!(
            pointer.color(&theme, DividerSide::List),
            DIVIDER_HOVER_COLOR
        );
        assert_eq!(pointer.color(&theme, DividerSide::Props), theme.border);

        pointer.set_drag(Some(DividerSide::Props));
        assert_eq!(pointer.hover(), Some(DividerSide::Props));
        assert_eq!(
            pointer.color(&theme, DividerSide::Props),
            DIVIDER_HOVER_COLOR
        );
    }
}
