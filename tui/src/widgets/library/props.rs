use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::Paragraph,
};

use crate::theme::Theme;
use crate::widgets::library::detail::{EMPTY_TOKEN, edge_line, edge_value_width, relative_time};
use crate::widgets::library::state::LibraryTrigger;
use crate::widgets::util;

/// Right-pane content: two cells of padding on each side, mirroring
/// the list pane. Shared by rendering and mouse hit-testing.
pub(crate) fn props_content(area: Rect, list_ratio: f32, props_ratio: f32) -> Rect {
    let split = super::split_panes(area, list_ratio, props_ratio);
    Rect {
        x: split.props.x.saturating_add(2),
        y: split.props.y.saturating_add(1),
        width: split.props.width.saturating_sub(5),
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
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Rect {
            x: content.x,
            y: content.y,
            width: content.width,
            height: 1.min(content.height),
        },
    );
    let rows = info_rows(item);
    let split_at = usage_start(item);
    for (position, (label, value)) in rows.iter().take(split_at).enumerate() {
        // Single blank line between items.
        let offset = (position as u16).saturating_mul(2).saturating_add(2);
        if offset >= content.height {
            break;
        }
        render_row(frame, content, theme, offset, label, value);
    }
    let tags_at = usage_toggle_offset(item).saturating_sub(2);
    if tags_at < content.height {
        render_tags_row(frame, content, theme, item, tags_at);
    }
    let toggle_at = usage_toggle_offset(item);
    if toggle_at < content.height {
        render_toggle(frame, content, theme, toggle_at, state.usage_expanded());
    }
    if state.usage_expanded() {
        for (position, (label, value)) in rows.iter().skip(split_at).enumerate() {
            let offset = toggle_at
                .saturating_add(2)
                .saturating_add((position as u16).saturating_mul(2));
            if offset >= content.height {
                break;
            }
            render_row(frame, content, theme, offset, label, value);
        }
    }
}

fn tag_color(theme: &Theme, index: usize) -> ratatui::style::Color {
    match index % 5 {
        0 => theme.accent,
        1 => theme.success,
        2 => theme.warning,
        3 => theme.error,
        _ => theme.primary,
    }
}

/// Chip strings that fit `available` cells, space-separated downstream.
/// Conservative by one cell; matches the old strip behavior.
pub(crate) fn pack_tag_chips(tags: &[String], available: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut used = 0usize;
    for tag in tags {
        let chip = format!("#{tag}");
        let width = chip.chars().count().saturating_add(1);
        if used.saturating_add(width) > available {
            break;
        }
        used = used.saturating_add(width);
        out.push(chip);
    }
    out
}

/// Tags property row: dim label left, packed color chips (or the add
/// button when empty) right-aligned like every other row.
fn render_tags_row(
    frame: &mut Frame,
    content: Rect,
    theme: &Theme,
    item: &LibraryTrigger,
    offset: u16,
) {
    use crate::widgets::library::icons::ADD_ICON;

    let row = Rect {
        x: content.x,
        y: content.y.saturating_add(offset),
        width: content.width,
        height: 1,
    };
    let label = "Tags";
    let available = edge_value_width(label, row.width);
    let (spans, width) = if item.tags().is_empty() {
        (
            vec![Span::styled(
                format!(" {ADD_ICON} "),
                Style::default()
                    .fg(theme.button.text)
                    .bg(theme.button.inactive_bg),
            )],
            3usize,
        )
    } else {
        let packed = pack_tag_chips(item.tags(), available as usize);
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut used = 0usize;
        for (index, chip) in packed.into_iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw(" ".to_string()));
                used = used.saturating_add(1);
            }
            let width = chip.chars().count();
            spans.push(Span::styled(
                chip,
                Style::default().fg(tag_color(theme, index)),
            ));
            used = used.saturating_add(width);
        }
        let width = used;
        (spans, width)
    };
    if spans.is_empty() {
        return;
    }
    frame.render_widget(
        Paragraph::new(edge_line(label, spans, width, row.width, theme)),
        row,
    );
}

fn render_toggle(frame: &mut Frame, content: Rect, theme: &Theme, offset: u16, expanded: bool) {
    use crate::widgets::library::icons::{CHEVRON_DOWN, CHEVRON_UP};
    let chevron = if expanded { CHEVRON_UP } else { CHEVRON_DOWN };
    let label = format!("Usage {chevron}");
    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(theme.text)),
        Rect {
            x: content.x,
            y: content.y.saturating_add(offset),
            width: content.width,
            height: 1,
        },
    );
}

fn render_row(
    frame: &mut Frame,
    content: Rect,
    theme: &Theme,
    offset: u16,
    label: &str,
    value: &str,
) {
    let row = Rect {
        x: content.x,
        y: content.y.saturating_add(offset),
        width: content.width,
        height: 1,
    };
    let value = util::truncate_to_width(value, edge_value_width(label, row.width));
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

/// Toggle offset for the usage section, shared by rendering and
/// hit-testing. Caller checks it against the pane height.
pub(crate) fn usage_toggle_offset(item: &LibraryTrigger) -> u16 {
    2 + (usage_start(item) as u16 + 1).saturating_mul(2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropsHit {
    UsageToggle,
}

/// Click on the usage toggle row. Nothing else in the pane is interactive.
pub(crate) fn hit_test(
    area: Rect,
    list_ratio: f32,
    props_ratio: f32,
    state: &crate::widgets::library::state::LibraryPageState,
    column: u16,
    row: u16,
) -> Option<PropsHit> {
    let content = props_content(area, list_ratio, props_ratio);
    if content.width == 0 || content.height == 0 {
        return None;
    }
    let selected = state.selected_index()?;
    let item = state.item_at_filtered(selected)?;
    let toggle = content.y.saturating_add(usage_toggle_offset(item));
    if row == toggle
        && row < content.y.saturating_add(content.height)
        && column >= content.x
        && column < content.x.saturating_add(content.width)
    {
        return Some(PropsHit::UsageToggle);
    }
    None
}

/// Pane rows: base properties plus the raw usage history (totals only,
/// no computed savings). The usage section folds behind a toggle.
pub(crate) fn info_rows(item: &LibraryTrigger) -> Vec<(&'static str, String)> {
    use crate::widgets::library::detail::property_rows;

    let mut rows = property_rows(item);
    rows.extend(usage_rows(item));
    rows
}

/// Index in `info_rows` where the usage section starts.
fn usage_start(item: &LibraryTrigger) -> usize {
    use crate::widgets::library::detail::property_rows;

    property_rows(item).len()
}

/// Raw usage history: stored totals, nothing derived.
pub(crate) fn usage_rows(item: &LibraryTrigger) -> Vec<(&'static str, String)> {
    vec![
        ("Usage", usage_count_line(item.usage_count())),
        (
            "Last used",
            item.last_used_at()
                .and_then(relative_time)
                .unwrap_or_else(|| EMPTY_TOKEN.to_string()),
        ),
        (
            "Created",
            relative_time(item.created_at()).unwrap_or_else(|| EMPTY_TOKEN.to_string()),
        ),
    ]
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
