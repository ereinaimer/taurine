use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
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
    let alias_at = alias_position(&rows);
    let alias_text = alias_text_lines(
        item,
        edge_value_width("Alias", content.width) as usize,
        content.width as usize,
    );
    // honey: wrapped alias lines push every row below them down.
    let alias_extra = alias_text.len().saturating_sub(1) as u16;
    for (position, (label, value)) in rows.iter().take(split_at).enumerate() {
        let position = position as u16;
        if Some(position) == alias_at {
            render_alias_block(frame, content, theme, &alias_text, position);
            continue;
        }
        // Single blank line between items.
        let mut offset = position.saturating_mul(2).saturating_add(2);
        if alias_at.is_some_and(|at| position > at) {
            offset = offset.saturating_add(alias_extra);
        }
        if offset >= content.height {
            break;
        }
        render_row(frame, content, theme, offset, label, value);
    }
    let tags_at = usage_toggle_offset(item)
        .saturating_add(alias_extra)
        .saturating_sub(2);
    if tags_at < content.height {
        render_tags_row(frame, content, theme, item, tags_at);
    }
    let toggle_at = usage_toggle_offset(item).saturating_add(alias_extra);
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
    AutoCase,
    UsageToggle,
}

/// Click on the auto-case row or the usage toggle row. The auto-case
/// row is located by label so a reorder never misfires.
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
    if column < content.x || column >= content.x.saturating_add(content.width) {
        return None;
    }
    let selected = state.selected_index()?;
    let item = state.item_at_filtered(selected)?;
    let extra = alias_extra_lines(item, content.width);
    let auto_case = content.y.saturating_add(auto_case_offset(item));
    if row == auto_case && row < content.y.saturating_add(content.height) {
        return Some(PropsHit::AutoCase);
    }
    let toggle = content
        .y
        .saturating_add(usage_toggle_offset(item))
        .saturating_add(extra);
    if row == toggle && row < content.y.saturating_add(content.height) {
        return Some(PropsHit::UsageToggle);
    }
    None
}

/// Row offset of the auto-case property, shared by rendering and
/// hit-testing so clicks land on the value they see.
fn auto_case_offset(item: &LibraryTrigger) -> u16 {
    info_rows(item)
        .iter()
        .position(|(label, _)| *label == "Auto case")
        .map(|position| (position as u16).saturating_mul(2).saturating_add(2))
        .unwrap_or(2)
}

/// Position of the Alias row within `info_rows`, if present.
fn alias_position(rows: &[(&str, String)]) -> Option<u16> {
    rows.iter()
        .position(|(label, _)| *label == "Alias")
        .map(|position| position as u16)
}

/// Alias text: up to three sibling aliases, one per line. Overlong
/// aliases truncate in place; a marker trails when siblings remain
/// past three; empty stays the border token.
pub(crate) fn alias_text_lines(
    item: &LibraryTrigger,
    first_budget: usize,
    cont_budget: usize,
) -> Vec<String> {
    use crate::widgets::library::detail::sibling_aliases;

    const MAX_SHOWN_ALIASES: usize = 3;
    if first_budget == 0 && cont_budget == 0 {
        return vec![String::new()];
    }
    let siblings = sibling_aliases(item.aliases(), item.trigger());
    if siblings.is_empty() {
        return vec![EMPTY_TOKEN.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    for (index, alias) in siblings.iter().take(MAX_SHOWN_ALIASES).enumerate() {
        let budget = if index == 0 {
            first_budget
        } else {
            cont_budget
        };
        lines.push(util::truncate_to_width(alias, budget as u16));
    }
    if siblings.len() > MAX_SHOWN_ALIASES {
        let budget = if lines.len() == 1 {
            first_budget
        } else {
            cont_budget
        };
        let last = lines.last().map(String::as_str).unwrap_or("");
        if last.chars().count().saturating_add(2) <= budget {
            lines.last_mut().expect("alias line").push_str(" …");
        } else {
            lines.push("…".to_string());
        }
    }
    lines
}

/// Extra rows the wrapped alias block adds below its first line,
/// shared by rendering and hit-testing.
pub(crate) fn alias_extra_lines(item: &LibraryTrigger, width: u16) -> u16 {
    alias_text_lines(
        item,
        edge_value_width("Alias", width) as usize,
        width as usize,
    )
    .len()
    .saturating_sub(1) as u16
}

/// Alias block: first line shares the row with the dim label like
/// every other row; continuations run full width in text color.
fn render_alias_block(
    frame: &mut Frame,
    content: Rect,
    theme: &Theme,
    lines: &[String],
    position: u16,
) {
    let base = position.saturating_mul(2).saturating_add(2);
    for (index, line) in lines.iter().enumerate() {
        let offset = base.saturating_add(index as u16);
        if offset >= content.height {
            break;
        }
        let row = Rect {
            x: content.x,
            y: content.y.saturating_add(offset),
            width: content.width,
            height: 1,
        };
        if index == 0 {
            let width = line.chars().count();
            frame.render_widget(
                Paragraph::new(edge_line(
                    "Alias",
                    vec![Span::styled(line.clone(), Style::default().fg(theme.text))],
                    width,
                    row.width,
                    theme,
                )),
                row,
            );
        } else {
            // honey: continuations right-align under the first line,
            // mirroring the label row above them.
            let pad = content.width.saturating_sub(line.chars().count() as u16) as usize;
            frame.render_widget(
                Paragraph::new(Line::from(format!("{}{}", " ".repeat(pad), line)))
                    .style(Style::default().fg(theme.text)),
                row,
            );
        }
    }
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
