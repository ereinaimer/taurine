use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::theme::Theme;
use crate::widgets::library::icons::{MIC_ICON, os_icon};
use crate::widgets::library::state::LibraryPageState;
use crate::widgets::util;

use super::content_halves;

const PREVIEW_LINES: usize = 8;
/// Trigger and Type occupy offsets 0..4, Output label sits at 4 with the
/// preview below, then Alias and the toggle.
const OUTPUT_OFFSET: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailHit {
    Toggle,
}

/// Which detail row a click landed on. Only the Advanced toggle is
/// interactive; everything else is read-only preview.
pub(crate) fn hit_test(
    area: Rect,
    ratio: f32,
    state: &LibraryPageState,
    column: u16,
    row: u16,
) -> Option<DetailHit> {
    let content = right_content(area, ratio);
    if content.width == 0 || content.height == 0 {
        return None;
    }
    state.selected_index()?;
    let toggle_y = toggle_row(content.height)?;
    let toggle_y = content.y.saturating_add(toggle_y);
    if row == toggle_y && column >= content.x && column < content.x.saturating_add(content.width) {
        Some(DetailHit::Toggle)
    } else {
        None
    }
}

/// Minimum content height for the pinned bottom block (Alias, toggle).
/// Shorter panes show the header and preview only.
const MIN_PINNED_HEIGHT: u16 = 12;

/// Visible output line count for the pane height.
fn output_count(height: u16, preview: &str) -> usize {
    preview
        .lines()
        .count()
        .min(PREVIEW_LINES)
        .min(height.saturating_sub(OUTPUT_OFFSET + 4) as usize)
}

/// Alias row offset: pinned near the bottom with one blank row and the
/// toggle beneath it. None when the pane is too short to pin.
fn alias_offset(height: u16) -> Option<u16> {
    (height >= MIN_PINNED_HEIGHT).then(|| height.saturating_sub(3))
}

/// Toggle row offset: pinned to the last content row. None when the pane
/// is too short to pin.
fn toggle_row(height: u16) -> Option<u16> {
    (height >= MIN_PINNED_HEIGHT).then(|| height.saturating_sub(1))
}

/// Right-pane content: two cells of padding on the left, one on the
/// right. Shared by rendering and mouse hit-testing.
pub(crate) fn right_content(area: Rect, ratio: f32) -> Rect {
    let (_, right) = content_halves(area, ratio);
    Rect {
        x: right.x.saturating_add(2),
        y: right.y.saturating_add(1),
        width: right.width.saturating_sub(3),
        height: right.height.saturating_sub(1),
    }
}

pub(crate) fn render_detail(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
) {
    let content = right_content(area, state.split_ratio());
    if content.width == 0 || content.height == 0 {
        return;
    }
    if state.load_error().is_some() {
        return;
    }

    let Some(index) = state.selected_index() else {
        render_empty(
            frame,
            content,
            theme,
            state
                .empty_state_message()
                .unwrap_or("No trigger selected."),
        );
        return;
    };
    let Some(item) = state.item_at_filtered(index) else {
        render_empty(frame, content, theme, "No trigger selected.");
        return;
    };

    render_trigger_row(frame, content, theme, item);
    render_type_row(frame, content, theme, item);
    render_content_rows(frame, content, theme, item);
    render_alias_rows(frame, content, theme, item);
    render_toggle_row(frame, content, theme, state);
    if state.advanced_expanded() {
        render_advanced_rows(frame, content, theme, item);
    }
}

fn render_empty(frame: &mut Frame, area: Rect, theme: &Theme, message: &str) {
    frame.render_widget(
        Paragraph::new(message).style(Style::default().fg(theme.text)),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1.min(area.height),
        },
    );
}

/// Edge-aligned row: label flush left, value flush right, everything in
/// plain text color. No dimming, no columns.
fn edge_line(
    label: &str,
    value: Vec<Span<'static>>,
    value_width: usize,
    row_width: u16,
    theme: &Theme,
) -> Line<'static> {
    let gap = row_width
        .saturating_sub(label.chars().count() as u16)
        .saturating_sub(value_width as u16);
    let mut spans = vec![Span::styled(
        label.to_string(),
        Style::default().fg(theme.text),
    )];
    spans.push(Span::raw(" ".repeat(gap as usize)));
    spans.extend(value);
    Line::from(spans)
}

/// Value text truncated to share its row with `label`, leaving one cell gap.
fn edge_value_width(label: &str, row_width: u16) -> u16 {
    row_width.saturating_sub(label.chars().count() as u16 + 1)
}

fn render_trigger_row(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &crate::widgets::library::state::LibraryTrigger,
) {
    let row = row_area(area, 0);
    if row.width == 0 {
        return;
    }
    let style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);
    let available = edge_value_width("Trigger", row.width);
    let line = if item.is_hotkey() && hotkey_width(item.trigger()) <= available as usize {
        let spans = hotkey_spans(item.trigger(), theme);
        edge_line(
            "Trigger",
            spans,
            hotkey_width(item.trigger()),
            row.width,
            theme,
        )
    } else {
        let value = util::truncate_to_width(item.trigger(), available);
        let width = value.chars().count();
        edge_line(
            "Trigger",
            vec![Span::styled(value, style)],
            width,
            row.width,
            theme,
        )
    };
    frame.render_widget(Paragraph::new(line), row);
}

fn render_type_row(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &crate::widgets::library::state::LibraryTrigger,
) {
    if area.height < 3 {
        return;
    }
    let row = row_area(area, 2);
    let style = Style::default().fg(theme.text);
    let kind = util::truncate_to_width(item.kind_label(), edge_value_width("Type", row.width));
    let mut text = String::new();
    if item.is_voice() {
        text.push_str(&format!("{MIC_ICON} "));
    }
    text.push_str(&kind);
    let text = util::truncate_to_width(&text, edge_value_width("Type", row.width));
    let width = text.chars().count();
    frame.render_widget(
        Paragraph::new(edge_line(
            "Type",
            vec![Span::styled(text, style)],
            width,
            row.width,
            theme,
        )),
        row,
    );
}

/// Sibling invocations, excluding the currently displayed trigger.
/// Nothing renders when the trigger has no siblings.
pub(crate) fn sibling_aliases<'a>(aliases: &'a [String], current: &str) -> Vec<&'a str> {
    aliases
        .iter()
        .map(String::as_str)
        .filter(|alias| alias.strip_suffix(" (confirm)").unwrap_or(alias) != current)
        .collect()
}

fn render_alias_rows(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &crate::widgets::library::state::LibraryTrigger,
) {
    let Some(offset) = alias_offset(area.height) else {
        return;
    };
    let siblings = sibling_aliases(item.aliases(), item.trigger());
    if siblings.is_empty() {
        return;
    }
    let row = row_area(area, offset);
    let aliases = siblings.join(", ");
    let value = util::truncate_to_width(&aliases, edge_value_width("Alias", row.width));
    let width = value.chars().count();
    frame.render_widget(
        Paragraph::new(edge_line(
            "Alias",
            vec![Span::styled(value, Style::default().fg(theme.text))],
            width,
            row.width,
            theme,
        )),
        row,
    );
}

fn render_content_rows(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &crate::widgets::library::state::LibraryTrigger,
) {
    if area.height < 5 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Output".to_string(),
            Style::default().fg(theme.text),
        )),
        row_area(area, 4),
    );

    let start_y = area.y.saturating_add(OUTPUT_OFFSET);
    for (index, line) in item
        .preview()
        .lines()
        .take(output_count(area.height, item.preview()))
        .enumerate()
    {
        let row = Rect {
            x: area.x,
            y: start_y.saturating_add(index as u16),
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(util::truncate_to_width(line.trim(), area.width))
                .style(Style::default().fg(theme.text)),
            row,
        );
    }
}

fn hotkey_width(trigger: &str) -> usize {
    let parts: Vec<&str> = trigger
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return trigger.chars().count();
    }
    parts
        .iter()
        .map(|part| part.chars().count() + 2)
        .sum::<usize>()
        + 3 * parts.len().saturating_sub(1)
}

fn hotkey_spans(trigger: &str, theme: &Theme) -> Vec<Span<'static>> {
    let chip = Style::default()
        .fg(theme.text)
        .bg(theme.surface)
        .add_modifier(Modifier::BOLD);
    let plus = Style::default().fg(theme.text);
    let mut spans = Vec::new();
    let parts: Vec<&str> = trigger
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return vec![Span::raw(trigger.to_string())];
    }
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" + ", plus));
        }
        spans.push(Span::styled(format!("[{part}]"), chip));
    }
    spans
}

fn row_area(area: Rect, offset: u16) -> Rect {
    Rect {
        x: area.x,
        y: area.y.saturating_add(offset),
        width: area.width,
        height: 1,
    }
}

fn render_toggle_row(frame: &mut Frame, area: Rect, theme: &Theme, state: &LibraryPageState) {
    if state.selected_index().is_none() {
        return;
    }
    let Some(toggle) = toggle_row(area.height) else {
        return;
    };
    let label = if state.advanced_expanded() {
        "- Advanced Options"
    } else {
        "+ Advanced Options"
    };
    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(theme.text)),
        row_area(area, toggle),
    );
}

fn render_advanced_rows(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &crate::widgets::library::state::LibraryTrigger,
) {
    let Some(alias) = alias_offset(area.height) else {
        return;
    };
    // honey: expanded rows fill the gap between the preview and the
    // pinned Alias row, stopping one row short to keep the blank gap.
    let count = output_count(area.height, item.preview());
    for (position, (label, value)) in advanced_rows(item).into_iter().enumerate() {
        let y = area
            .y
            .saturating_add(OUTPUT_OFFSET)
            .saturating_add(count as u16 + 1 + position as u16 * 2);
        if y.saturating_add(1) >= area.y.saturating_add(alias) {
            return;
        }
        let row = Rect {
            x: area.x,
            y,
            width: area.width,
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

fn advanced_rows(
    item: &crate::widgets::library::state::LibraryTrigger,
) -> Vec<(&'static str, String)> {
    let mut rows = vec![
        (
            "Platform",
            format!("{} {}", os_icon(&item.target_os), item.target_os),
        ),
        (
            "Tags",
            if item.tags().is_empty() {
                "—".to_string()
            } else {
                item.tags().join(", ")
            },
        ),
        ("Description", item.description().unwrap_or("—").to_string()),
        ("Usage", usage_line(item.usage_count(), item.last_used_at())),
        (
            "Auto-case",
            if item.auto_case() {
                "on".to_string()
            } else {
                "off".to_string()
            },
        ),
    ];
    if let Some(interpreter) = item.interpreter() {
        rows.push(("Language", interpreter.as_str().to_string()));
    }
    if let Some(behavior) = item.behavior() {
        rows.push(("Mode", behavior.as_str().to_string()));
    }
    if let Some(only) = item.only_apps() {
        rows.push(("Only in", only.to_string()));
    }
    if let Some(except) = item.except_apps() {
        rows.push(("Except", except.to_string()));
    }
    rows
}

fn usage_line(usage_count: i64, last_used_at: Option<i64>) -> String {
    if usage_count <= 0 {
        return "never used".to_string();
    }
    let times = if usage_count == 1 {
        "1 time".to_string()
    } else {
        format!("{usage_count} times")
    };
    match last_used_at.and_then(relative_time) {
        Some(ago) => format!("{times} · {ago}"),
        None => times,
    }
}

fn relative_time(timestamp: i64) -> Option<String> {
    if timestamp <= 0 {
        return None;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)?;
    let diff = now.saturating_sub(timestamp).max(0);

    if diff < 60 {
        Some("just now".to_string())
    } else if diff < 3600 {
        Some(format!("{}m ago", diff / 60))
    } else if diff < 86400 {
        Some(format!("{}h ago", diff / 3600))
    } else if diff < 2_592_000 {
        Some(format!("{}d ago", diff / 86400))
    } else if diff < 31_104_000 {
        Some(format!("{}mo ago", diff / 2_592_000))
    } else {
        Some(format!("{}y ago", diff / 31_104_000))
    }
}
