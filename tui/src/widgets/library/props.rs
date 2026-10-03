use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::Paragraph,
};
use taurine_core::stats::{calculate_saved_keystrokes, calculate_time_saved_ms};

use crate::theme::Theme;
use crate::widgets::library::detail::{EMPTY_TOKEN, edge_line, edge_value_width, relative_time};
use crate::widgets::library::state::LibraryTrigger;
use crate::widgets::util;

/// Right-pane content with standard padding. Shared by rendering and
/// mouse hit-testing (read-only for now; editing lands here later).
pub(crate) fn props_content(area: Rect, list_ratio: f32, props_ratio: f32) -> Rect {
    let split = super::split_panes(area, list_ratio, props_ratio);
    Rect {
        x: split.props.x.saturating_add(2),
        y: split.props.y.saturating_add(1),
        width: split.props.width.saturating_sub(3),
        height: split.props.height.saturating_sub(1),
    }
}

pub(crate) fn render_props(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &crate::widgets::library::state::LibraryPageState,
) {
    let content = props_content(area, state.split_ratio(), state.detail_ratio());
    if content.width == 0 || content.height == 0 {
        return;
    }
    let Some(index) = state.selected_index() else {
        return;
    };
    let Some(item) = state.item_at_filtered(index) else {
        return;
    };

    frame.render_widget(
        Paragraph::new(Span::styled(
            "Properties".to_string(),
            Style::default().fg(theme.text).add_modifier(Modifier::DIM),
        )),
        Rect {
            x: content.x,
            y: content.y,
            width: content.width,
            height: 1.min(content.height),
        },
    );
    for (position, (label, value)) in info_rows(item).into_iter().enumerate() {
        // Single blank line between items.
        let offset = (position as u16).saturating_mul(2).saturating_add(2);
        if offset >= content.height {
            break;
        }
        let row = Rect {
            x: content.x,
            y: content.y.saturating_add(offset),
            width: content.width,
            height: 1,
        };
        let value = util::truncate_to_width(&value, edge_value_width(label, row.width));
        let width = value.chars().count();
        frame.render_widget(
            Paragraph::new(edge_line(
                label,
                vec![Span::styled(value, Style::default().fg(theme.text))],
                width,
                row.width,
                theme,
            )),
            row,
        );
    }
}

/// Pane rows: base properties plus usage extras. Text and voice triggers
/// show computed savings; scripts show runs only (no keystrokes saved).
pub(crate) fn info_rows(item: &LibraryTrigger) -> Vec<(&'static str, String)> {
    use crate::widgets::library::detail::property_rows;

    let mut rows = property_rows(item);
    rows.extend(usage_rows(item));
    rows
}

/// Usage section, shown after the base properties behind a divider.
pub(crate) fn usage_rows(item: &LibraryTrigger) -> Vec<(&'static str, String)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let mut rows = vec![
        ("Usage", usage_count_line(item.usage_count())),
        (
            "Last used",
            item.last_used_at()
                .and_then(relative_time)
                .unwrap_or_else(|| EMPTY_TOKEN.to_string()),
        ),
    ];
    rows.push((
        "Created",
        relative_time(item.created_at()).unwrap_or_else(|| EMPTY_TOKEN.to_string()),
    ));
    rows.push(("Frequency", frequency_line(item, now)));
    if !item.is_script() {
        let per_use = calculate_saved_keystrokes(
            item.content().chars().count(),
            item.trigger().chars().count(),
        );
        rows.push((
            "Keystrokes saved",
            if item.usage_count() <= 0 {
                EMPTY_TOKEN.to_string()
            } else {
                format_int(per_use.saturating_mul(item.usage_count()))
            },
        ));
        rows.push((
            "Time saved",
            if item.usage_count() <= 0 {
                EMPTY_TOKEN.to_string()
            } else {
                format_duration(
                    calculate_time_saved_ms(per_use, 0).saturating_mul(item.usage_count()),
                )
            },
        ));
    }
    rows
}

fn usage_count_line(usage_count: i64) -> String {
    if usage_count <= 0 {
        return "never used".to_string();
    }
    if usage_count == 1 {
        "1 time".to_string()
    } else {
        format!("{usage_count} times")
    }
}

fn frequency_line(item: &LibraryTrigger, now: i64) -> String {
    if item.usage_count() <= 0 {
        return EMPTY_TOKEN.to_string();
    }
    let days = now
        .saturating_sub(item.created_at())
        .saturating_div(86_400)
        .max(1);
    let per_day = item.usage_count() as f64 / days as f64;
    if per_day < 0.05 {
        return format!("{} total", usage_total(item.usage_count()));
    }
    let rounded = (per_day * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{:.0}/day", rounded)
    } else {
        format!("{rounded:.1}/day")
    }
}

fn usage_total(count: i64) -> String {
    if count == 1 {
        "1 use".to_string()
    } else {
        format!("{count} uses")
    }
}

fn format_int(value: i64) -> String {
    let negative = value < 0;
    let digits: Vec<char> = value.abs().to_string().chars().collect();
    let mut out = String::new();
    for (index, ch) in digits.iter().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*ch);
    }
    if negative { format!("-{out}") } else { out }
}

fn format_duration(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    if secs < 60 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    if minutes < 60 {
        let rest = secs % 60;
        return if rest == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m {rest}s")
        };
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if rest == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {rest}m")
    }
}
