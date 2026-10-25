use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Span,
};

/// Five-cell switch: a three-cell track and a two-cell knob.
pub(crate) const TOGGLE_WIDTH: u16 = 5;
/// One frame at roughly 60 fps while a switch is sliding.
pub(crate) const TOGGLE_ANIM_FRAME: Duration = Duration::from_millis(16);
/// Slide and color-lerp duration for every switch.
pub(crate) const TOGGLE_ANIM_DURATION: Duration = Duration::from_millis(90);

const ENABLED_TRACK: Color = Color::Rgb(27, 56, 36);
pub(crate) const ENABLED_KNOB: Color = Color::Rgb(62, 207, 102);
const DISABLED_TRACK: Color = Color::Rgb(36, 36, 40);
const DISABLED_KNOB: Color = Color::Rgb(128, 128, 136);

/// The five cells a switch occupies when parked at the right edge of
/// `row`. Every view shares this geometry for rendering and hit-testing.
pub(crate) fn toggle_rect(row: Rect) -> Rect {
    if row.width == 0 || row.height == 0 {
        return Rect::default();
    }
    let width = TOGGLE_WIDTH.min(row.width);
    Rect {
        x: row.right().saturating_sub(width),
        y: row.y,
        width,
        height: 1,
    }
}

pub(crate) fn toggle_contains(rect: Rect, column: u16, row: u16) -> bool {
    crate::terminal::mouse::contains(rect, column, row)
}

/// Stable identity for one switch, independent of the view that paints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ToggleId {
    TriggerEnabled,
    PropAutoCase,
    PropConfirm,
    CreateAutoCase,
    Setting(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ToggleAnimation {
    from_on: bool,
    start: Instant,
    duration: Duration,
}

/// One sampled animation frame. `from_on` keeps the slide direction tied
/// to the state the switch left, not the state it is heading toward.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ToggleFrame {
    pub(crate) progress: f32,
    pub(crate) from_on: bool,
}

/// Active switch animations, keyed by the control they belong to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ToggleAnimations {
    active: HashMap<ToggleId, ToggleAnimation>,
}

impl ToggleAnimations {
    pub(crate) fn start(&mut self, id: ToggleId, from_on: bool) {
        self.start_at(id, from_on, Instant::now());
    }

    pub(crate) fn start_at(&mut self, id: ToggleId, from_on: bool, now: Instant) {
        self.active.insert(
            id,
            ToggleAnimation {
                from_on,
                start: now,
                duration: TOGGLE_ANIM_DURATION,
            },
        );
    }

    /// Normalized progress for one switch, or `None` once it has landed.
    pub(crate) fn progress(&self, id: ToggleId, now: Instant) -> Option<ToggleFrame> {
        let animation = self.active.get(&id)?;
        let elapsed = now.saturating_duration_since(animation.start);
        if elapsed >= animation.duration {
            return None;
        }
        Some(ToggleFrame {
            progress: (elapsed.as_secs_f32() / animation.duration.as_secs_f32()).clamp(0.0, 1.0),
            from_on: animation.from_on,
        })
    }

    pub(crate) fn progress_now(&self, id: ToggleId) -> Option<ToggleFrame> {
        self.progress(id, Instant::now())
    }

    /// Drops landed animations and reports whether any are still running.
    pub(crate) fn has_active_animations(&mut self, now: Instant) -> bool {
        self.active.retain(|_, animation| {
            now.saturating_duration_since(animation.start) < animation.duration
        });
        !self.active.is_empty()
    }
}

fn toggle_cell(color: Color) -> Span<'static> {
    Span::styled(" ", Style::default().bg(color))
}

fn toggle_half_cell(symbol: &'static str, knob: Color, track: Color) -> Span<'static> {
    Span::styled(symbol, Style::default().fg(knob).bg(track))
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let (Color::Rgb(from_red, from_green, from_blue), Color::Rgb(to_red, to_green, to_blue)) =
        (from, to)
    else {
        return to;
    };
    let t = t.clamp(0.0, 1.0);
    let mix =
        |from: u8, to: u8| (f32::from(from) + (f32::from(to) - f32::from(from)) * t).round() as u8;
    Color::Rgb(
        mix(from_red, to_red),
        mix(from_green, to_green),
        mix(from_blue, to_blue),
    )
}

/// Brighten a knob by roughly twelve percent when the pointer is over it.
fn toggle_knob(color: Color, hovered: bool) -> Color {
    if !hovered {
        return color;
    }
    let Color::Rgb(red, green, blue) = color else {
        return color;
    };
    let lift = |channel: u8| channel.saturating_add((u16::from(channel) * 12 / 100) as u8);
    Color::Rgb(lift(red), lift(green), lift(blue))
}

/// Five-cell switch with a 3-cell track and 2-cell knob. Animation slides
/// the knob across ten half-cells while lerping the track and knob colors.
/// `is_on` parks a static switch; an active frame takes its direction from
/// the state the switch left, so turning off starts at the right.
pub(crate) fn toggle_spans(
    is_on: bool,
    hovered: bool,
    anim: Option<ToggleFrame>,
) -> Vec<Span<'static>> {
    let pos = match anim {
        Some(frame) => {
            let t = frame.progress.clamp(0.0, 1.0);
            if frame.from_on { 1.0 - t } else { t }
        }
        None => {
            if is_on {
                1.0
            } else {
                0.0
            }
        }
    };
    let track = lerp_color(DISABLED_TRACK, ENABLED_TRACK, pos);
    let knob = toggle_knob(lerp_color(DISABLED_KNOB, ENABLED_KNOB, pos), hovered);
    let start_half = (pos * 6.0).round() as usize;
    let end_half = start_half + 4;
    (0..TOGGLE_WIDTH)
        .map(|cell_idx| {
            let left = usize::from(cell_idx) * 2;
            let right = left + 1;
            let left_in_knob = left >= start_half && left < end_half;
            let right_in_knob = right >= start_half && right < end_half;
            match (left_in_knob, right_in_knob) {
                (true, true) => toggle_cell(knob),
                (false, false) => toggle_cell(track),
                (true, false) => toggle_half_cell("▌", knob, track),
                (false, true) => toggle_half_cell("▐", knob, track),
            }
        })
        .collect()
}

/// Paint the switch directly into a buffer at `(x, y)`.
pub(crate) fn render_toggle(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    is_on: bool,
    hovered: bool,
    anim: Option<ToggleFrame>,
) {
    let area = buf.area;
    if y < area.y || y >= area.bottom() {
        return;
    }
    for (offset, span) in toggle_spans(is_on, hovered, anim).into_iter().enumerate() {
        let column = x.saturating_add(offset as u16);
        if column < area.x || column >= area.right() {
            continue;
        }
        let Some(cell) = buf.cell_mut((column, y)) else {
            continue;
        };
        cell.set_symbol(span.content.as_ref());
        cell.set_style(span.style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn static_states_park_the_knob_at_each_end() {
        let off = toggle_spans(false, false, None);
        assert_eq!(off.len(), 5);
        assert!(
            off[..2]
                .iter()
                .all(|span| span.style.bg == Some(DISABLED_KNOB))
        );
        assert!(
            off[2..]
                .iter()
                .all(|span| span.style.bg == Some(DISABLED_TRACK))
        );

        let on = toggle_spans(true, false, None);
        assert!(
            on[..3]
                .iter()
                .all(|span| span.style.bg == Some(ENABLED_TRACK))
        );
        assert!(
            on[3..]
                .iter()
                .all(|span| span.style.bg == Some(ENABLED_KNOB))
        );
    }

    #[test]
    fn midpoint_uses_half_cells_on_both_sides_of_the_knob() {
        let spans = toggle_spans(
            true,
            false,
            Some(ToggleFrame {
                progress: 0.5,
                from_on: false,
            }),
        );
        assert_eq!(spans[1].content.as_ref(), "▐");
        assert_eq!(spans[3].content.as_ref(), "▌");
        assert_eq!(spans[2].content.as_ref(), " ");
    }

    #[test]
    fn turning_off_starts_at_the_right_and_slides_left() {
        let start = toggle_spans(
            false,
            false,
            Some(ToggleFrame {
                progress: 0.0,
                from_on: true,
            }),
        );
        assert!(
            start[3..]
                .iter()
                .all(|span| span.style.bg == Some(ENABLED_KNOB))
        );
        let end = toggle_spans(
            false,
            false,
            Some(ToggleFrame {
                progress: 1.0,
                from_on: true,
            }),
        );
        assert!(
            end[..2]
                .iter()
                .all(|span| span.style.bg == Some(DISABLED_KNOB))
        );
    }

    #[test]
    fn hover_brightens_the_knob_only() {
        let spans = toggle_spans(true, true, None);
        assert_eq!(spans[3].style.bg, Some(Color::Rgb(69, 231, 114)));
        assert_eq!(spans[0].style.bg, Some(ENABLED_TRACK));
    }

    #[test]
    fn registry_cleans_landed_animations() {
        let mut animations = ToggleAnimations::default();
        let start = Instant::now();
        animations.start_at(ToggleId::TriggerEnabled, false, start);
        assert_eq!(
            animations.progress(ToggleId::TriggerEnabled, start),
            Some(ToggleFrame {
                progress: 0.0,
                from_on: false,
            })
        );
        assert!(animations.has_active_animations(start));

        let half = start + TOGGLE_ANIM_DURATION / 2;
        let frame = animations
            .progress(ToggleId::TriggerEnabled, half)
            .expect("animating");
        assert!((frame.progress - 0.5).abs() < 0.01);
        assert!(!frame.from_on);

        let end = start + TOGGLE_ANIM_DURATION;
        assert!(!animations.has_active_animations(end));
        assert!(animations.progress(ToggleId::TriggerEnabled, end).is_none());
    }

    #[test]
    fn one_finished_switch_does_not_stop_another() {
        let mut animations = ToggleAnimations::default();
        let start = Instant::now();
        animations.start_at(ToggleId::TriggerEnabled, false, start);
        animations.start_at(
            ToggleId::PropAutoCase,
            true,
            start + Duration::from_millis(40),
        );

        let first_done = start + TOGGLE_ANIM_DURATION;
        assert!(animations.has_active_animations(first_done));
        assert!(
            animations
                .progress(ToggleId::TriggerEnabled, first_done)
                .is_none()
        );
        assert!(
            animations
                .progress(ToggleId::PropAutoCase, first_done)
                .is_some()
        );

        let all_done = start + Duration::from_millis(130);
        assert!(!animations.has_active_animations(all_done));
    }

    #[test]
    fn right_aligned_geometry_is_five_cells_wide() {
        let row = Rect::new(10, 4, 40, 3);
        let toggle = toggle_rect(row);
        assert_eq!(toggle, Rect::new(45, 4, 5, 1));
        assert!(toggle_contains(toggle, 45, 4));
        assert!(toggle_contains(toggle, 49, 4));
        assert!(!toggle_contains(toggle, 44, 4));
        assert!(!toggle_contains(toggle, 50, 4));
        assert!(!toggle_contains(toggle, 45, 5));
    }

    #[test]
    fn render_toggle_writes_five_cells() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        render_toggle(&mut buffer, 2, 0, true, false, None);
        assert_eq!(buffer[(2, 0)].bg, ENABLED_TRACK);
        assert_eq!(buffer[(4, 0)].bg, ENABLED_TRACK);
        assert_eq!(buffer[(5, 0)].bg, ENABLED_KNOB);
        assert_eq!(buffer[(6, 0)].bg, ENABLED_KNOB);
        assert_eq!(buffer[(1, 0)].bg, Color::Reset);
    }
}
