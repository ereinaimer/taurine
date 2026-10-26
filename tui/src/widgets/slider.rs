use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Span,
};

use crate::widgets::toggle::{DISABLED_TRACK, ENABLED_KNOB, ENABLED_TRACK};

/// Preferred track width and the "100%" label cells beside it.
pub(crate) const TRACK_WIDTH: u16 = 16;
pub(crate) const PERCENT_WIDTH: u16 = 4;
const KNOB_HALF_CELLS: usize = 4;

/// Right-aligned slider track inside a row's value column. The percent
/// label sits immediately to its left, with one cell of gap.
pub(crate) fn track_rect(value_column: Rect) -> Option<Rect> {
    if value_column.width == 0 || value_column.height == 0 {
        return None;
    }
    let width = TRACK_WIDTH.min(
        value_column
            .width
            .saturating_sub(PERCENT_WIDTH.saturating_add(1)),
    );
    if width == 0 {
        return None;
    }
    Some(Rect {
        x: value_column.right().saturating_sub(width),
        y: value_column.y,
        width,
        height: 1,
    })
}

pub(crate) fn label_rect(track: Rect) -> Rect {
    Rect {
        x: track.x.saturating_sub(PERCENT_WIDTH.saturating_add(1)),
        y: track.y,
        width: PERCENT_WIDTH,
        height: 1,
    }
}

fn track_cell(symbol: &'static str, fg: Option<Color>, bg: Color) -> Span<'static> {
    let style = match fg {
        Some(fg) => Style::default().fg(fg).bg(bg),
        None => Style::default().bg(bg),
    };
    Span::styled(symbol, style)
}

/// Extends the switch to a volume bar: the filled portion uses the enabled
/// track color, the empty tail the disabled track color, and the same
/// two-cell knob slides between them at half-cell precision.
pub(crate) fn spans(volume: u32, width: u16) -> Vec<Span<'static>> {
    let half_cells = usize::from(width) * 2;
    let knob = KNOB_HALF_CELLS.min(half_cells);
    let travel = half_cells.saturating_sub(knob);
    let start = if travel == 0 {
        0
    } else {
        (f64::from(volume.min(100)) / 100.0 * travel as f64).round() as usize
    };
    let end = start.saturating_add(knob);
    (0..usize::from(width))
        .map(|cell| {
            let left = cell * 2;
            let right = left + 1;
            let left_knob = left >= start && left < end;
            let right_knob = right >= start && right < end;
            match (left_knob, right_knob) {
                (true, true) => track_cell(" ", None, ENABLED_KNOB),
                (false, false) => track_cell(
                    " ",
                    None,
                    if left < start {
                        ENABLED_TRACK
                    } else {
                        DISABLED_TRACK
                    },
                ),
                (true, false) => track_cell("▌", Some(ENABLED_KNOB), DISABLED_TRACK),
                (false, true) => track_cell("▐", Some(ENABLED_KNOB), ENABLED_TRACK),
            }
        })
        .collect()
}

/// Volume that places the knob under the clicked cell. Inverse of `spans`,
/// so a click and the next redraw agree on where the knob lands.
pub(crate) fn value_at(track: Rect, column: u16) -> u32 {
    if track.width == 0 {
        return 0;
    }
    let offset = column
        .saturating_sub(track.x)
        .min(track.width.saturating_sub(1));
    let center = usize::from(offset) * 2 + 1;
    let half_cells = usize::from(track.width) * 2;
    let knob = KNOB_HALF_CELLS.min(half_cells);
    let travel = half_cells.saturating_sub(knob);
    if travel == 0 {
        return 100;
    }
    let start = center.saturating_sub(knob / 2).min(travel);
    ((start as f64 / travel as f64) * 100.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backgrounds(spans: &[Span<'_>]) -> Vec<Color> {
        spans.iter().map(|span| span.style.bg.unwrap()).collect()
    }

    #[test]
    fn zero_parks_the_knob_at_the_left_edge() {
        let spans = spans(0, TRACK_WIDTH);
        let bgs = backgrounds(&spans);
        assert_eq!(&bgs[..2], &[ENABLED_KNOB, ENABLED_KNOB]);
        assert!(bgs[2..].iter().all(|bg| *bg == DISABLED_TRACK));
    }

    #[test]
    fn full_parks_the_knob_at_the_right_edge() {
        let spans = spans(100, TRACK_WIDTH);
        let bgs = backgrounds(&spans);
        assert!(bgs[..14].iter().all(|bg| *bg == ENABLED_TRACK));
        assert_eq!(&bgs[14..], &[ENABLED_KNOB, ENABLED_KNOB]);
    }

    #[test]
    fn midpoint_fills_half_the_track() {
        let bgs = backgrounds(&spans(50, TRACK_WIDTH));
        assert_eq!(bgs[0], ENABLED_TRACK);
        assert_eq!(*bgs.last().unwrap(), DISABLED_TRACK);
        assert!(bgs.contains(&ENABLED_KNOB));
    }

    #[test]
    fn clicks_at_the_edges_land_on_the_bounds() {
        let track = track_rect(Rect::new(0, 0, 30, 1)).expect("track");
        assert_eq!(value_at(track, track.x), 0);
        assert_eq!(value_at(track, track.right().saturating_sub(1)), 100);
        assert_eq!(value_at(track, track.x + track.width / 2), 54);
    }

    #[test]
    fn track_leaves_room_for_the_percent_label() {
        let column = Rect::new(0, 0, 30, 1);
        let track = track_rect(column).expect("track");
        let label = label_rect(track);
        assert_eq!(track.width, TRACK_WIDTH);
        assert_eq!(label.width, PERCENT_WIDTH);
        assert!(label.right() < track.x);
        assert_eq!(track.right(), column.right());
    }
}
