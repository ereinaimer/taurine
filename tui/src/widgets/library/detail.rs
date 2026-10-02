use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::theme::Theme;
use crate::widgets::library::icons::{ADD_ICON, CHEVRON_DOWN, INFO_ICON, os_icon};
use crate::widgets::library::state::{LibraryPageState, LibraryTrigger};
use crate::widgets::util;

use super::content_halves;

const CONTENT_INNER_HEIGHT: usize = 12;
/// Empty-state token: three ROUNDED-border horizontals, matching the pane
/// border glyph set.
pub(crate) const EMPTY_TOKEN: &str = "───";
const TOGGLE_ON: &str = "[ON]";
const TOGGLE_OFF: &str = "[OFF]";
const INFO_BUTTON_WIDTH: u16 = 3;
const INFO_GAP: u16 = 2;

/// Fixed single-row offsets above the content section.
const DESCRIPTION_OFFSET: u16 = 2;
const BUTTONS_OFFSET: u16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailHit {
    EnableToggle,
    InfoOpen,
}

/// Which detail row a click landed on. The header toggle and info button
/// are interactive; everything else is read-only preview.
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
        let info_start = content
            .x
            .saturating_add(content.width.saturating_sub(INFO_BUTTON_WIDTH));
        if column >= info_start && column < info_start.saturating_add(INFO_BUTTON_WIDTH) {
            return Some(DetailHit::InfoOpen);
        }
        let toggle_width = toggle_width(item);
        let toggle_start = info_start.saturating_sub(INFO_GAP + toggle_width);
        if column >= toggle_start && column < toggle_start.saturating_add(toggle_width) {
            return Some(DetailHit::EnableToggle);
        }
        return None;
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

/// Wrap source lines to the pane width, one visual row per chunk.
/// Blank lines stay blank; tabs flatten; trailing space trimmed so it
/// never forces an extra row. No ellipsis, nothing clipped.
pub(crate) fn wrap_content_lines(content: &str, width: u16) -> Vec<String> {
    let width = (width.max(1)) as usize;
    let mut rows = Vec::new();
    for line in content.lines() {
        let flat = line.replace('\t', "  ");
        let trimmed = flat.trim_end();
        if trimmed.is_empty() {
            rows.push(String::new());
            continue;
        }
        let chars: Vec<char> = trimmed.chars().collect();
        for chunk in chars.chunks(width) {
            rows.push(chunk.iter().collect());
        }
    }
    rows
}

/// Full flow layout, top to bottom: header 0, blank 1, description 2,
/// blank 3, buttons 4, blank 5, static content box (14 rows: border +
/// 12 text + border), gap, tags. Sections that do not fit are None
/// and skipped.
struct DetailLayout {
    content_rows: Vec<(u16, String)>,
    rest: usize,
    tags: Option<u16>,
}

const CONTENT_BOX_TOP: u16 = 6;
const CONTENT_BOX_HEIGHT: u16 = CONTENT_INNER_HEIGHT as u16 + 2;

/// Text width inside the content box: border plus one cell of padding
/// each side, mirroring the search box.
fn content_text_width(box_width: u16) -> u16 {
    box_width.saturating_sub(4)
}

fn detail_layout(
    height: u16,
    width: u16,
    item: &LibraryTrigger,
    scroll: usize,
) -> Option<DetailLayout> {
    let box_bottom = CONTENT_BOX_TOP.saturating_add(CONTENT_BOX_HEIGHT);
    if box_bottom > height || width < 5 {
        return None;
    }
    let text_width = content_text_width(width);
    let wrapped = wrap_content_lines(item.content(), text_width);
    let total = wrapped.len();
    let visible = CONTENT_INNER_HEIGHT.min(total.saturating_sub(scroll.min(total)));
    let start = CONTENT_BOX_TOP.saturating_add(1);
    let content_rows = wrapped
        .into_iter()
        .skip(scroll)
        .take(visible)
        .enumerate()
        .map(|(index, line)| (start.saturating_add(index as u16), line))
        .collect::<Vec<_>>();
    let rest = total.saturating_sub(scroll.saturating_add(visible));
    let tags = box_bottom.saturating_add(1);
    let tags = (tags < height).then_some(tags);
    Some(DetailLayout {
        content_rows,
        rest,
        tags,
    })
}

/// Max scroll offset for the content section at this pane size.
pub(crate) fn content_scroll_max(height: u16, width: u16, item: &LibraryTrigger) -> usize {
    let Some(layout) = detail_layout(height, width, item, 0) else {
        return 0;
    };
    layout.rest
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

    let scroll = state.detail_scroll();
    render_header_row(frame, content, theme, item);
    render_description_row(frame, content, theme, item);
    render_buttons_row(frame, content, theme, item);
    let Some(layout) = detail_layout(content.height, content.width, item, scroll) else {
        return;
    };
    render_content_rows(frame, content, theme, &layout);
    render_tags_row(frame, content, theme, item, &layout);
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
pub(crate) fn edge_line(
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
pub(crate) fn edge_value_width(label: &str, row_width: u16) -> u16 {
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
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            TOGGLE_OFF.to_string(),
            Style::default().fg(theme.text).add_modifier(Modifier::DIM),
        )
    };
    let info = Span::styled(
        format!(" {INFO_ICON} "),
        Style::default()
            .fg(theme.button.text)
            .bg(theme.button.inactive_bg),
    );
    let width = toggle_width(item);
    let reserved = width + INFO_GAP + INFO_BUTTON_WIDTH;
    let available = row.width.saturating_sub(reserved).saturating_sub(1);
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
        Span::raw(" ".repeat(INFO_GAP as usize)),
        info,
    ]);
    frame.render_widget(Paragraph::new(line), row);
}

fn render_description_row(frame: &mut Frame, area: Rect, theme: &Theme, item: &LibraryTrigger) {
    if DESCRIPTION_OFFSET >= area.height {
        return;
    }
    let text = match item.description() {
        Some(description) if !description.trim().is_empty() => description.to_string(),
        _ => "No description.".to_string(),
    };
    // Same dimmed treatment as the other labels (Content, properties).
    let style = Style::default().fg(theme.text).add_modifier(Modifier::DIM);
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

/// Static content box: fixed 14 rows with the rounded search-box border
/// and 12 scrollable text rows inside, no scrollbar.
/// The box never grows or shrinks with the content length.
fn render_content_rows(frame: &mut Frame, area: Rect, theme: &Theme, layout: &DetailLayout) {
    use ratatui::widgets::{Block, Borders};

    let popup = Rect {
        x: area.x,
        y: area.y.saturating_add(CONTENT_BOX_TOP),
        width: area.width,
        height: CONTENT_BOX_HEIGHT,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(Style::default().fg(theme.border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    // honey: one cell of horizontal padding inside the border, like search.
    let text = Rect {
        x: inner.x.saturating_add(1),
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: CONTENT_INNER_HEIGHT as u16,
    };
    for (index, (_, line)) in layout.content_rows.iter().enumerate() {
        if index >= text.height as usize {
            break;
        }
        let row = Rect {
            x: text.x,
            y: text.y.saturating_add(index as u16),
            width: text.width,
            height: 1,
        };
        // Pre-wrapped to the text width: one visual row, nothing clipped.
        frame.render_widget(
            Paragraph::new(Line::from(line.clone())).style(Style::default().fg(theme.text)),
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
    layout: &DetailLayout,
) {
    let Some(offset) = layout.tags else {
        return;
    };
    let row = row_area(area, offset);
    let plus = || {
        Span::styled(
            format!(" {ADD_ICON} "),
            Style::default()
                .fg(theme.button.text)
                .bg(theme.button.inactive_bg),
        )
    };
    if item.tags().is_empty() {
        let label = "No tags available. ";
        let width = label.chars().count().saturating_add(3);
        if width > row.width as usize {
            frame.render_widget(Paragraph::new(Line::from(vec![plus()])), row);
            return;
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    label.to_string(),
                    Style::default().fg(theme.text).add_modifier(Modifier::DIM),
                ),
                plus(),
            ])),
            row,
        );
        return;
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    let plus_width = 3usize;
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
        spans.push(plus());
    }
    if spans.is_empty() {
        spans.push(plus());
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

pub(crate) fn relative_time(timestamp: i64) -> Option<String> {
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
