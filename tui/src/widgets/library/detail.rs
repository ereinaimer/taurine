use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::theme::Theme;
use crate::widgets::library::icons::{CHEVRON_DOWN, CHEVRON_UP, os_icon};
use crate::widgets::library::state::{LibraryPageState, LibraryTrigger};
use crate::widgets::util;

use super::content_halves;

const PREVIEW_LINES: usize = 8;
/// Empty-state token: three ROUNDED-border horizontals, matching the pane
/// border glyph set.
pub(crate) const EMPTY_TOKEN: &str = "───";
const TOGGLE_ON: &str = "[ON]";
const TOGGLE_OFF: &str = "[OFF]";

/// Flow offsets for the right pane. Header 0, description 1, blank 2,
/// buttons 3 (type, plus interpreter + behavior for scripts), blank 4,
/// properties toggle 5, then expanded rows, blank gap, content label +
/// lines, tags row.
fn props_toggle_offset(_item: &LibraryTrigger) -> u16 {
    5
}

/// Fixed single-row offsets above the properties toggle.
const DESCRIPTION_OFFSET: u16 = 1;
const BUTTONS_OFFSET: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailHit {
    PropertiesToggle,
    EnableToggle,
}

/// Which detail row a click landed on. Header toggle and the properties
/// toggle are interactive; everything else is read-only preview.
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
    let selected = state.selected_index()?;
    let item = state.item_at_filtered(selected)?;
    if row == content.y {
        let width = toggle_width(item);
        let start = content
            .x
            .saturating_add(content.width.saturating_sub(width));
        if column >= start && column < start.saturating_add(width) {
            return Some(DetailHit::EnableToggle);
        }
        return None;
    }
    if row == content.y.saturating_add(props_toggle_offset(item))
        && row < content.y.saturating_add(content.height)
        && column >= content.x
        && column < content.x.saturating_add(content.width)
    {
        return Some(DetailHit::PropertiesToggle);
    }
    None
}

/// True when the cell sits inside the right-pane content area.
pub(crate) fn detail_contains(area: Rect, ratio: f32, column: u16, row: u16) -> bool {
    let content = right_content(area, ratio);
    column >= content.x
        && column < content.x.saturating_add(content.width)
        && row >= content.y
        && row < content.y.saturating_add(content.height)
}

/// Max scroll offset for the content section at this pane height.
pub(crate) fn content_scroll_max(height: u16, item: &LibraryTrigger, expanded: bool) -> usize {
    let Some((_, _, total)) = content_window(height, item, expanded, 0) else {
        return 0;
    };
    total
}

/// Visible content window: (label offset, lines to draw, scrollable total
/// beyond the visible window). None when the label row does not fit.
type ContentWindow = (u16, Vec<(u16, String)>, usize);

fn content_window(
    height: u16,
    item: &LibraryTrigger,
    expanded: bool,
    scroll: usize,
) -> Option<ContentWindow> {
    let lines: Vec<&str> = item.content().lines().collect();
    let total = lines.len();
    let mut label = props_toggle_offset(item).saturating_add(1);
    if expanded {
        label = label.saturating_add(property_rows(item).len() as u16);
    }
    // One blank gap row between properties and content.
    label = label.saturating_add(1);
    if label >= height {
        return None;
    }
    let start = label.saturating_add(1);
    // Reserve a gap row plus the tags row below the content.
    let visible = PREVIEW_LINES
        .min(total.saturating_sub(scroll.min(total)))
        .min(height.saturating_sub(start).saturating_sub(2) as usize);
    let rows = lines
        .into_iter()
        .skip(scroll)
        .take(visible)
        .enumerate()
        .map(|(index, line)| (start.saturating_add(index as u16), line.to_string()))
        .collect();
    let rest = total.saturating_sub(scroll.saturating_add(visible));
    Some((label, rows, rest))
}

/// Tags row offset: gap row + tags row below the visible content.
/// None when it does not fit.
fn tags_offset(height: u16, item: &LibraryTrigger, expanded: bool, scroll: usize) -> Option<u16> {
    let (label, rows, _) = content_window(height, item, expanded, scroll)?;
    let offset = label
        .saturating_add(1)
        .saturating_add(rows.len() as u16)
        .saturating_add(1);
    (offset < height).then_some(offset)
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

    let expanded = state.advanced_expanded();
    render_header_row(frame, content, theme, item);
    render_description_row(frame, content, theme, item);
    render_buttons_row(frame, content, theme, item);
    render_properties_toggle(frame, content, theme, item, expanded);
    if expanded {
        render_property_rows(frame, content, theme, item);
    }
    render_content_section(frame, content, theme, item, expanded, state.detail_scroll());
    render_tags_row(frame, content, theme, item, expanded, state.detail_scroll());
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

/// Edge-aligned row: dimmed label flush left, value flush right.
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
        Style::default().fg(theme.text).add_modifier(Modifier::DIM),
    )];
    spans.push(Span::raw(" ".repeat(gap as usize)));
    spans.extend(value);
    Line::from(spans)
}

/// Value text truncated to share its row with `label`, leaving one cell gap.
fn edge_value_width(label: &str, row_width: u16) -> u16 {
    row_width.saturating_sub(label.chars().count() as u16 + 1)
}

fn toggle_width(item: &LibraryTrigger) -> u16 {
    if item.is_enabled() {
        TOGGLE_ON.chars().count() as u16
    } else {
        TOGGLE_OFF.chars().count() as u16
    }
}

fn render_header_row(frame: &mut Frame, area: Rect, theme: &Theme, item: &LibraryTrigger) {
    let row = row_area(area, 0);
    if row.width == 0 {
        return;
    }
    let toggle = if item.is_enabled() {
        Span::styled(
            TOGGLE_ON.to_string(),
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            TOGGLE_OFF.to_string(),
            Style::default().fg(theme.text_muted),
        )
    };
    let width = toggle_width(item);
    let available = row.width.saturating_sub(width).saturating_sub(1);
    let name = util::truncate_to_width(item.display_name(), available);
    let name_width = name.chars().count();
    let gap = available.saturating_sub(name_width as u16);
    let line = Line::from(vec![
        Span::styled(
            name,
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap as usize + 1)),
        toggle,
    ]);
    frame.render_widget(Paragraph::new(line), row);
}

fn render_description_row(frame: &mut Frame, area: Rect, theme: &Theme, item: &LibraryTrigger) {
    if DESCRIPTION_OFFSET >= area.height {
        return;
    }
    let (text, dimmed) = match item.description() {
        Some(description) if !description.trim().is_empty() => (description.to_string(), false),
        _ => ("No description.".to_string(), true),
    };
    let mut style = Style::default().fg(theme.description);
    if dimmed {
        style = style.add_modifier(Modifier::DIM);
    }
    // Single row only: first line, cut to width, never wrapped.
    let first = text.lines().next().unwrap_or("").trim();
    let value = util::truncate_to_width(first, area.width);
    frame.render_widget(
        Paragraph::new(Line::from(value)).style(style),
        row_area(area, DESCRIPTION_OFFSET),
    );
}

/// Dropdown-style button (visual only until the dropdown component lands):
/// 1-cell horizontal padding inside a background fill.
fn button_spans(label: &str, theme: &Theme) -> (Vec<Span<'static>>, usize) {
    let text = format!(" {label} {CHEVRON_DOWN} ");
    let width = text.chars().count();
    (
        vec![Span::styled(
            text,
            Style::default()
                .fg(theme.button.text)
                .bg(theme.button.inactive_bg),
        )],
        width,
    )
}

fn render_buttons_row(frame: &mut Frame, area: Rect, theme: &Theme, item: &LibraryTrigger) {
    if BUTTONS_OFFSET >= area.height {
        return;
    }
    let row = row_area(area, BUTTONS_OFFSET);
    let mut labels = vec![item.invocation_type_label().to_string()];
    if item.is_script() {
        labels.push(
            item.interpreter()
                .map(|interpreter| interpreter.as_str())
                .unwrap_or(EMPTY_TOKEN)
                .to_string(),
        );
        labels.push(
            item.behavior()
                .map(|behavior| behavior.as_str())
                .unwrap_or(EMPTY_TOKEN)
                .to_string(),
        );
    }
    // Two-space gaps; buttons that no longer fit are dropped, type first.
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for (index, label) in labels.iter().enumerate() {
        let (button, width) = button_spans(label, theme);
        let gap = if index > 0 { 2 } else { 0 };
        if used.saturating_add(gap).saturating_add(width) > row.width as usize {
            break;
        }
        if gap > 0 {
            spans.push(Span::raw("  ".to_string()));
            used = used.saturating_add(gap);
        }
        spans.extend(button);
        used = used.saturating_add(width);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
}

fn render_properties_toggle(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &LibraryTrigger,
    expanded: bool,
) {
    let offset = props_toggle_offset(item);
    if offset >= area.height {
        return;
    }
    let chevron = if expanded { CHEVRON_UP } else { CHEVRON_DOWN };
    let label = format!("Properties {chevron}");
    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(theme.text)),
        row_area(area, offset),
    );
}

pub(crate) fn property_rows(item: &LibraryTrigger) -> Vec<(&'static str, String)> {
    let siblings = sibling_aliases(item.aliases(), item.trigger());
    let mut rows = vec![
        (
            "Auto case",
            if item.auto_case() {
                "on".to_string()
            } else {
                "off".to_string()
            },
        ),
        (
            "Allow on",
            item.only_apps().unwrap_or(EMPTY_TOKEN).to_string(),
        ),
        (
            "Block on",
            item.except_apps().unwrap_or(EMPTY_TOKEN).to_string(),
        ),
        (
            "Alias",
            if siblings.is_empty() {
                EMPTY_TOKEN.to_string()
            } else {
                siblings.join(", ")
            },
        ),
        (
            "Platform",
            format!("{} {}", os_icon(&item.target_os), item.target_os),
        ),
        ("Usage", usage_count_line(item.usage_count())),
        (
            "Last used",
            item.last_used_at()
                .and_then(relative_time)
                .unwrap_or_else(|| EMPTY_TOKEN.to_string()),
        ),
    ];
    if item.is_voice() {
        rows.push((
            "Confirm",
            if item.require_confirmation() {
                "on".to_string()
            } else {
                "off".to_string()
            },
        ));
    }
    rows
}

fn render_property_rows(frame: &mut Frame, area: Rect, theme: &Theme, item: &LibraryTrigger) {
    let base = props_toggle_offset(item).saturating_add(1);
    for (position, (label, value)) in property_rows(item).into_iter().enumerate() {
        let offset = base.saturating_add(position as u16);
        if offset >= area.height {
            return;
        }
        let row = row_area(area, offset);
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

fn render_content_section(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &LibraryTrigger,
    expanded: bool,
    scroll: usize,
) {
    let Some((label, rows, _)) = content_window(area.height, item, expanded, scroll) else {
        return;
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Content".to_string(),
            Style::default().fg(theme.text).add_modifier(Modifier::DIM),
        )),
        row_area(area, label),
    );
    for (offset, line) in rows {
        let row = Rect {
            x: area.x,
            y: area.y.saturating_add(offset),
            width: area.width,
            height: 1,
        };
        // One source line per row: tabs flattened, cut to width, no wrap.
        let flat = line.replace('\t', "  ");
        let text = util::truncate_to_width(flat.trim_end(), area.width);
        frame.render_widget(
            Paragraph::new(Line::from(text)).style(Style::default().fg(theme.text)),
            row,
        );
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

fn render_tags_row(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &LibraryTrigger,
    expanded: bool,
    scroll: usize,
) {
    let Some(offset) = tags_offset(area.height, item, expanded, scroll) else {
        return;
    };
    let row = row_area(area, offset);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    let plus = " + ";
    let plus_width = plus.chars().count();
    for (index, tag) in item.tags().iter().enumerate() {
        let chip = format!("#{tag}");
        let width = chip.chars().count().saturating_add(1);
        if used.saturating_add(width).saturating_add(plus_width) > row.width as usize {
            break;
        }
        if index > 0 {
            spans.push(Span::raw(" ".to_string()));
            used = used.saturating_add(1);
        }
        spans.push(Span::styled(
            chip,
            Style::default().fg(tag_color(theme, index)),
        ));
        used = used.saturating_add(width.saturating_sub(1));
    }
    if used + plus_width <= row.width as usize {
        if !spans.is_empty() {
            spans.push(Span::raw(" ".to_string()));
        }
        spans.push(Span::styled(
            plus.to_string(),
            Style::default()
                .fg(theme.button.text)
                .bg(theme.button.inactive_bg),
        ));
    }
    if spans.is_empty() {
        spans.push(Span::styled(
            plus.to_string(),
            Style::default()
                .fg(theme.button.text)
                .bg(theme.button.inactive_bg),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
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

fn row_area(area: Rect, offset: u16) -> Rect {
    Rect {
        x: area.x,
        y: area.y.saturating_add(offset),
        width: area.width,
        height: 1,
    }
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
