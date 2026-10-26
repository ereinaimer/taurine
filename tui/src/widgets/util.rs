use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::library::state::ButtonSelection;

pub(crate) fn truncate_to_width(value: &str, max_chars: u16) -> String {
    if value.chars().count() <= max_chars as usize {
        value.to_string()
    } else {
        format!(
            "{}…",
            value
                .chars()
                .take(max_chars.saturating_sub(1) as usize)
                .collect::<String>()
        )
    }
}

/// Caret column for a char-index cursor inside a one-line field starting
/// at `x` with `width` cells. Stays inside the field so the terminal
/// caret (which honors the configured cursor style) is what the user sees.
pub(crate) fn caret_position(x: u16, y: u16, cursor: usize, width: u16) -> (u16, u16) {
    let column = (cursor as u16).min(width.saturating_sub(1));
    (x.saturating_add(column), y)
}

pub(crate) fn visible_range(total: usize, selected: usize, visible_count: usize) -> (usize, usize) {
    if total <= visible_count {
        return (0, total);
    }
    let mut start = selected.saturating_sub(visible_count.saturating_sub(1));
    let mut end = (start + visible_count).min(total);
    if end - start < visible_count {
        start = end.saturating_sub(visible_count);
        end = total.min(start + visible_count);
    }
    (start, end)
}

/// Window `[start, end)` of variable-height rows containing `selected`
/// that fits in `available` rows. Anchors `selected` at the bottom first,
/// then fills above and below. Shared by settings and the command palette.
pub(crate) fn visible_variable_range(
    heights: &[u16],
    selected: usize,
    available: u16,
) -> (usize, usize) {
    if heights.is_empty() || available == 0 {
        return (0, 0);
    }
    let selected = selected.min(heights.len().saturating_sub(1));
    if heights[selected] > available {
        return (selected, selected + 1);
    }
    let mut start = selected;
    let mut end = selected + 1;
    let mut used = heights[selected];
    while start > 0 && used.saturating_add(heights[start - 1]) <= available {
        start -= 1;
        used = used.saturating_add(heights[start]);
    }
    while end < heights.len() && used.saturating_add(heights[end]) <= available {
        used = used.saturating_add(heights[end]);
        end += 1;
    }
    (start, end)
}

pub(crate) fn visible_library_item_capacity(available_height: u16) -> usize {
    const LIBRARY_ITEM_HEIGHT: u16 = 2;
    const LIBRARY_ITEM_PADDING: u16 = 1;
    const ROW_HEIGHT: u16 = LIBRARY_ITEM_HEIGHT + 2 * LIBRARY_ITEM_PADDING;
    if available_height < LIBRARY_ITEM_HEIGHT {
        return 0;
    }
    usize::from(available_height / ROW_HEIGHT).max(1)
}

/// Percentage of a terminal dimension, rounded down.
pub(crate) const fn percent_of(value: u16, percent: u16) -> u16 {
    (value as u32 * percent as u32 / 100) as u16
}

/// Centered rect clamped to the terminal. Single home for every
/// overlay popup so renderers never recompute centering.
pub(crate) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
        ])
        .split(vertical[1])[1]
}

/// Shared centered option overlay: flat #141414 fill, no borders, one
/// cell of padding, cursor list. Each option renders as a white value
/// over a dimmed detail line. Every plain option menu renders through
/// here so future overlays reuse it instead of copying it.
pub(crate) struct OverlayEntry {
    pub(crate) label: String,
    pub(crate) detail: String,
    pub(crate) icon: String,
}

/// Label prefix: the icon tucked against the label, or three spaces
/// when there is no icon, so text always starts inside the padding.
fn overlay_label_prefix(icon: &str) -> String {
    if icon.is_empty() {
        "   ".to_string()
    } else {
        format!("   {icon} ")
    }
}

/// Fixed overlay rect shared by rendering and hit-testing so clicks
/// land on the options as drawn.
pub(crate) fn overlay_popup(area: Rect) -> Rect {
    overlay_popup_sized(56, 15, area)
}

/// Sized overlay rect for dialogs with less content than a full option
/// menu; same centering, so small popups feel like the same component.
pub(crate) fn overlay_popup_sized(width: u16, height: u16, area: Rect) -> Rect {
    centered_rect(width, height, area)
}

/// Content rect inside an overlay popup, shared by overlay renders
/// and hit-testing so both agree on padding.
pub(crate) fn overlay_body(popup: Rect) -> Rect {
    popup.inner(Margin::new(3, 1))
}

/// Top popup rows covered by the shared header band.
pub(crate) const OVERLAY_HEADER_ROWS: u16 = 3;

/// Flat popup surface; the header band paints over the top rows.
pub(crate) const OVERLAY_BG: Color = Color::Rgb(0x14, 0x14, 0x14);

/// Shared popup chrome: flat fill, header band, title row, and the
/// content rect below the band and its one-row gap.
pub(crate) struct OverlayChrome {
    pub(crate) popup: Rect,
    pub(crate) body: Rect,
    pub(crate) header: Rect,
    pub(crate) content: Rect,
}

impl OverlayChrome {
    pub(crate) fn new(popup: Rect) -> Self {
        Self::with_body(popup, overlay_body(popup))
    }

    pub(crate) fn with_body(popup: Rect, body: Rect) -> Self {
        let content_y = body.y.saturating_add(OVERLAY_HEADER_ROWS);
        Self {
            popup,
            body,
            header: Rect {
                x: popup.x,
                y: popup.y,
                width: popup.width,
                height: OVERLAY_HEADER_ROWS.min(popup.height),
            },
            content: Rect {
                x: body.x,
                y: content_y,
                width: body.width,
                height: body.y.saturating_add(body.height).saturating_sub(content_y),
            },
        }
    }

    /// Paint the fill and header band without the title; app-filter
    /// overlays carry their validation error in the title row.
    pub(crate) fn paint(&self, frame: &mut Frame, theme: &Theme) {
        frame.render_widget(Clear, self.popup);
        frame.render_widget(
            Block::default().style(Style::default().bg(OVERLAY_BG)),
            self.popup,
        );
        if self.header.height > 0 {
            frame.render_widget(
                Block::default().style(Style::default().bg(theme.header.bg)),
                self.header,
            );
        }
    }

    pub(crate) fn render(&self, frame: &mut Frame, theme: &Theme, title: &str) {
        self.paint(frame, theme);
        if self.body.width > 0 && self.body.height > 0 {
            render_overlay_title(frame, self.body, theme, title);
        }
    }
}

/// Option index under the cell inside an overlay popup. `line_counts`
/// carries each option's rendered row height (1 or 2), mirroring the
/// render path; header, gap, and padding cells never select.
pub(crate) fn overlay_option_hit(
    area: Rect,
    line_counts: &[u16],
    column: u16,
    row: u16,
) -> Option<usize> {
    let chrome = OverlayChrome::new(overlay_popup(area));
    // honey: rows run full-bleed across the popup; only the header
    // band, gap, and vertical padding constrain the hit.
    if column < chrome.popup.x
        || column >= chrome.popup.x.saturating_add(chrome.popup.width)
        || row < chrome.content.y
    {
        return None;
    }
    // honey: the header band and its gap sit above the options.
    let mut y = chrome.content.y;
    for (index, lines) in line_counts.iter().enumerate() {
        if row >= y && row < y.saturating_add(*lines) {
            return Some(index);
        }
        y = y.saturating_add(*lines);
    }
    None
}

pub(crate) const OVERLAY_TITLE_HINT: &str = "Esc";

/// Overlay title row shared by every overlay: bold title left,
/// dimmed Esc hint flush right.
pub(crate) fn render_overlay_title(frame: &mut Frame, body: Rect, theme: &Theme, title: &str) {
    if body.height == 0 {
        return;
    }
    let hint = OVERLAY_TITLE_HINT;
    let gap = body
        .width
        .saturating_sub(title.chars().count() as u16)
        .saturating_sub(hint.chars().count() as u16);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title.to_string(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" ".repeat(gap as usize)),
            Span::styled(
                hint.to_string(),
                Style::default()
                    .fg(theme.text_muted)
                    .add_modifier(Modifier::DIM),
            ),
        ])),
        Rect {
            x: body.x,
            y: body.y,
            width: body.width,
            height: 1,
        },
    );
}

/// Whether a cell lands on the rendered Esc hint in the title row.
pub(crate) fn overlay_title_esc_hit(body: Rect, column: u16, row: u16) -> bool {
    if body.height == 0 || row != body.y {
        return false;
    }
    let hint_start = body.x.saturating_add(
        body.width
            .saturating_sub(OVERLAY_TITLE_HINT.chars().count() as u16),
    );
    column >= hint_start && column < body.x.saturating_add(body.width)
}

/// Shared dismissal rule for standard overlays: a click on the scrim
/// outside the popup, or on the title's Esc hint, dismisses it.
pub(crate) fn overlay_dismiss_hit(popup: Rect, column: u16, row: u16) -> bool {
    !crate::terminal::mouse::contains(popup, column, row)
        || overlay_title_esc_hit(overlay_body(popup), column, row)
}

pub(crate) fn render_overlay_select(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    entries: &[OverlayEntry],
    selected: usize,
) {
    // honey: fixed size for every option overlay so all menus feel
    // like the same component; terminal cells run taller than wide,
    // so the width floor keeps the landscape read. Centered_rect
    // clamps to the terminal.
    let chrome = OverlayChrome::new(overlay_popup(area));
    chrome.render(frame, theme, title);
    if chrome.body.width == 0 || chrome.body.height == 0 {
        return;
    }
    let list_area = Rect {
        x: chrome.popup.x,
        y: chrome.content.y,
        width: chrome.popup.width,
        height: chrome.content.height,
    };
    if list_area.height == 0 {
        return;
    }

    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| {
            let prefix = overlay_label_prefix(&entry.icon);
            if entry.detail.is_empty() {
                ListItem::new(Line::from(format!("{prefix}{}", entry.label)))
            } else {
                // honey: unselected values stay regular so the bold
                // highlight patch marks the cursor on the dark fill.
                // Text aligns with the title padding; only the
                // highlight band runs full-bleed.
                ListItem::new(vec![
                    Line::from(Span::styled(
                        format!("{prefix}{}", entry.label),
                        Style::default().fg(theme.text),
                    )),
                    Line::from(Span::styled(
                        format!("   {}", entry.detail),
                        Style::default().fg(theme.description),
                    )),
                ])
            }
        })
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(selected));

    // honey: patch adds the cursor band and bold only, so the dimmed
    // detail keeps its color on the highlighted row.
    let list = List::new(items).highlight_symbol("").highlight_style(
        Style::default()
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(list, list_area, &mut list_state);
}

pub(crate) fn render_action_buttons(
    frame: &mut Frame,
    area: Rect,
    cancel_label: &str,
    confirm_label: &str,
    is_focused: bool,
    selection: ButtonSelection,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let cancel_text = format!("  {cancel_label}  ");
    let confirm_text = format!("  {confirm_label}  ");
    let cancel_width = cancel_text.len() as u16;
    let confirm_width = confirm_text.len() as u16;
    let gap: u16 = 3;

    let btn_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(cancel_width),
            Constraint::Length(gap),
            Constraint::Length(confirm_width),
            Constraint::Min(1),
        ])
        .split(area);

    let cancel_style = if is_focused && selection == ButtonSelection::Cancel {
        Style::default()
            .fg(theme.text)
            .bg(theme.button.active_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(theme.surface)
    };
    frame.render_widget(
        Paragraph::new(cancel_text).style(cancel_style),
        btn_layout[1],
    );

    let confirm_style = if is_focused && selection == ButtonSelection::Confirm {
        Style::default()
            .fg(theme.text)
            .bg(theme.button.active_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(theme.surface)
    };
    frame.render_widget(
        Paragraph::new(confirm_text).style(confirm_style),
        btn_layout[3],
    );
}

/// Horizontal confirm-button pair centered in `row`: two padded cells
/// (`  label  `) with a three-cell gap. Shared geometry for every
/// confirm overlay so rendering and hit-testing agree; future deletes
/// reuse this instead of copying it.
pub(crate) fn confirm_button_cells(row: Rect, left_label: &str, right_label: &str) -> (Rect, Rect) {
    const PAD: u16 = 2;
    const GAP: u16 = 3;
    let left_width = left_label.chars().count() as u16 + PAD * 2;
    let right_width = right_label.chars().count() as u16 + PAD * 2;
    let total = left_width.saturating_add(GAP).saturating_add(right_width);
    let start = row.x.saturating_add(row.width.saturating_sub(total) / 2);
    let left = Rect {
        x: start,
        y: row.y,
        width: left_width,
        height: 1,
    };
    let right = Rect {
        x: start.saturating_add(left_width).saturating_add(GAP),
        y: row.y,
        width: right_width,
        height: 1,
    };
    (left, right)
}

/// Paints a `confirm_button_cells` pair: the selected cell takes the
/// cursor band, the other stays muted text.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_confirm_buttons(
    frame: &mut Frame,
    left: Rect,
    right: Rect,
    left_label: &str,
    right_label: &str,
    select_left: bool,
    theme: &Theme,
) {
    for (cell, label, selected) in [
        (left, left_label, select_left),
        (right, right_label, !select_left),
    ] {
        if cell.width == 0 || cell.height == 0 {
            continue;
        }
        let style = if selected {
            Style::default()
                .fg(theme.text)
                .bg(theme.surface)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_muted)
        };
        frame.render_widget(Paragraph::new(format!("  {label}  ")).style(style), cell);
    }
}

pub(crate) fn render_modal_field_label(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    focused: bool,
    indicator: Option<String>,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let indicator_width = indicator
        .as_ref()
        .map(|value| value.chars().count() as u16)
        .unwrap_or_default();
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(indicator_width)])
        .split(area);

    let label_style = if focused {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };
    frame.render_widget(Paragraph::new(label).style(label_style), sections[0]);

    if let Some(indicator) = indicator {
        frame.render_widget(
            Paragraph::new(indicator)
                .alignment(ratatui::layout::Alignment::Right)
                .style(
                    Style::default()
                        .fg(theme.text_muted)
                        .add_modifier(Modifier::DIM),
                ),
            sections[1],
        );
    }
}

pub(crate) fn render_modal_input_field(
    frame: &mut Frame,
    area: Rect,
    value: &str,
    cursor: usize,
    focused: bool,
    theme: &Theme,
) {
    let bg = if focused {
        theme.surface
    } else {
        theme.background
    };
    let text_style = if focused {
        Style::default()
            .fg(theme.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(bg)
    };

    let block = Block::default().style(Style::default().bg(bg));
    frame.render_widget(block, area);
    let (visible, caret) = super::field::window_of(value, cursor, area.width);
    frame.render_widget(Paragraph::new(visible.to_string()).style(text_style), area);
    if focused && area.width > 0 && area.height > 0 && !crate::overlay_fx::is_cursor_suppressed() {
        let (cx, cy) = caret_position(area.x, area.y, caret, area.width);
        frame.set_cursor_position((cx, cy));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_modal_password_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    cursor: usize,
    focused: bool,
    disabled: bool,
    red_asterisk: bool,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let label_width = area.width.min(12);
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(label_width), Constraint::Min(0)])
        .split(area);

    let (label_style, value_style) = if disabled {
        let dimmed = Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM);
        (dimmed, dimmed)
    } else if focused {
        (
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(theme.text),
            Style::default().fg(theme.text),
        )
    };

    let label_line = if red_asterisk {
        Line::from(vec![
            Span::styled(label, label_style),
            Span::styled("*", Style::default().fg(theme.error)),
        ])
    } else {
        Line::from(vec![Span::styled(label, label_style)])
    };
    frame.render_widget(Paragraph::new(label_line), sections[0]);
    // honey: same "*" mask as before, windowed so long secrets slide.
    let (visible, caret) = super::field::window_of(value, cursor, sections[1].width);
    frame.render_widget(
        Paragraph::new(visible.to_string()).style(value_style),
        sections[1],
    );
    if focused
        && !disabled
        && sections[1].width > 0
        && sections[1].height > 0
        && !crate::overlay_fx::is_cursor_suppressed()
    {
        let (cx, cy) = caret_position(sections[1].x, sections[1].y, caret, sections[1].width);
        frame.set_cursor_position((cx, cy));
    }
}

pub(crate) fn render_modal_key_value_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    quiet: bool,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let bg = if focused {
        theme.surface
    } else {
        ratatui::style::Color::Reset
    };
    frame.render_widget(Block::default().style(Style::default().bg(bg)), area);

    let label_width = area.width.min(12);
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(label_width), Constraint::Min(0)])
        .split(area);

    let label_style = if focused {
        Style::default()
            .fg(theme.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else if quiet {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let value_style = if focused {
        Style::default().fg(theme.text).bg(bg)
    } else if quiet {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(theme.text)
    };

    frame.render_widget(Paragraph::new(label).style(label_style), sections[0]);
    frame.render_widget(
        Paragraph::new(truncate_to_width(value, sections[1].width)).style(value_style),
        sections[1],
    );
}

/// Cells the search block spends on chrome: one border and one padding
/// cell on each side, plus the magnifier slot and its gap.
const SEARCH_BLOCK_CHROME_WIDTH: u16 = 9;
/// Icon slot: a two-cell raster centered in five cells so the ASCII
/// fallback `[S]` still leaves one cell before the text.
const SEARCH_ICON_WIDTH: u16 = 5;
/// Content lines the block grows to before it scrolls.
pub(crate) const SEARCH_BLOCK_MAX_LINES: usize = 3;
const SEARCH_BLOCK_MIN_HEIGHT: u16 = 3;

/// Block height for a wrapped query: content lines plus the two border
/// rows, capped so the list above keeps its space.
pub(crate) fn search_block_height(
    field: &super::field::TextField,
    is_active: bool,
    width: u16,
) -> u16 {
    let text_width = width.saturating_sub(SEARCH_BLOCK_CHROME_WIDTH);
    if text_width == 0 {
        return SEARCH_BLOCK_MIN_HEIGHT;
    }
    let lines = field.wrapped(text_width, is_active).lines().len();
    (lines.clamp(1, SEARCH_BLOCK_MAX_LINES) as u16).saturating_add(2)
}

/// Char index under a click inside the search block, accounting for the
/// wrapped lines and the scroll that keeps the caret visible.
pub(crate) fn search_block_index_at(
    field: &super::field::TextField,
    is_active: bool,
    area: Rect,
    column: u16,
    row: u16,
) -> usize {
    let text_width = area.width.saturating_sub(SEARCH_BLOCK_CHROME_WIDTH);
    if text_width == 0 {
        return 0;
    }
    let wrapped = field.wrapped(text_width, is_active);
    let visible_rows = usize::from(area.height.saturating_sub(2)).max(1);
    let scroll = wrapped.scroll(visible_rows);
    let line = scroll + usize::from(row.saturating_sub(area.y.saturating_add(1)));
    let col = usize::from(
        column.saturating_sub(area.x.saturating_add(2).saturating_add(SEARCH_ICON_WIDTH)),
    );
    wrapped.index_at(line, col)
}

/// Shared bottom search box: rounded border, an optional leading icon,
/// one to three wrapped text lines, and a caret-anchored viewport that
/// scrolls once the query is taller than the block. No background fill.
/// `placeholder` shows when the query is empty and inactive.
pub(crate) fn render_search_block(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    field: &super::field::TextField,
    is_active: bool,
    placeholder: &str,
    icon: Option<&ratatui_image::protocol::Protocol>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let border_style = Style::default().fg(theme.border);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let title_style = if is_active {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else if field.is_empty() {
        Style::default().fg(theme.description)
    } else {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    };
    // honey: the real terminal caret shows here so the configured cursor
    // style applies; no painted fake block.
    // honey: one cell of horizontal padding inside the border, then the
    // magnifier slot before the text.
    let line_area = Rect {
        x: inner.x.saturating_add(1),
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };
    if line_area.width == 0 {
        return;
    }
    let icon_width = SEARCH_ICON_WIDTH.min(line_area.width);
    crate::widgets::icons::render_icon(
        frame,
        Rect {
            x: line_area.x,
            y: line_area.y,
            width: icon_width,
            height: 1.min(line_area.height),
        },
        icon,
        crate::widgets::icons::SEARCH_FALLBACK,
        Style::default().fg(theme.text_muted),
    );
    let line_area = Rect {
        x: line_area.x.saturating_add(icon_width),
        y: line_area.y,
        width: line_area.width.saturating_sub(icon_width),
        height: line_area.height,
    };
    if line_area.width == 0 {
        return;
    }
    let wrapped = field.wrapped(line_area.width, is_active);
    let scroll = wrapped.scroll(usize::from(line_area.height));
    let title: Vec<Line> = if !is_active && field.is_empty() {
        vec![Line::from(placeholder.to_string())]
    } else {
        wrapped
            .lines()
            .iter()
            .skip(scroll)
            .take(usize::from(line_area.height))
            .map(|line| Line::from(line.clone()))
            .collect()
    };
    frame.render_widget(Paragraph::new(title).style(title_style), line_area);
    if is_active && !crate::overlay_fx::is_cursor_suppressed() {
        let (row, col) = wrapped.caret();
        let row = row.saturating_sub(scroll) as u16;
        let (cx, cy) = caret_position(
            line_area.x,
            line_area.y.saturating_add(row),
            col,
            line_area.width,
        );
        frame.set_cursor_position((cx, cy));
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use crate::widgets::field::TextField;

    use super::{
        OVERLAY_HEADER_ROWS, OverlayChrome, confirm_button_cells, overlay_body,
        overlay_dismiss_hit, overlay_popup,
    };

    #[test]
    fn overlay_chrome_puts_content_below_band_and_gap() {
        let chrome = OverlayChrome::new(overlay_popup(Rect::new(0, 0, 100, 30)));

        assert_eq!(chrome.popup, Rect::new(22, 7, 56, 15));
        assert_eq!(chrome.body, Rect::new(25, 8, 50, 13));
        assert_eq!(chrome.header, Rect::new(22, 7, 56, OVERLAY_HEADER_ROWS));
        assert_eq!(chrome.content, Rect::new(25, 11, 50, 10));
    }

    #[test]
    fn overlay_dismiss_hit_covers_scrim_and_title_hint() {
        let popup = Rect::new(10, 5, 40, 12);
        let body = overlay_body(popup);

        assert!(overlay_dismiss_hit(popup, 0, 0));
        assert!(overlay_dismiss_hit(
            popup,
            body.x.saturating_add(body.width).saturating_sub(1),
            body.y
        ));
        assert!(!overlay_dismiss_hit(popup, body.x, body.y));
        assert!(!overlay_dismiss_hit(
            popup,
            body.x,
            body.y.saturating_add(1)
        ));
    }

    #[test]
    fn confirm_button_cells_center_yes_no_pair() {
        let row = Rect::new(25, 14, 50, 1);

        let (yes, no) = confirm_button_cells(row, "Yes", "No");

        assert_eq!(yes, Rect::new(42, 14, 7, 1));
        assert_eq!(no, Rect::new(52, 14, 6, 1));
    }

    #[test]
    fn viewport_shows_full_query_when_it_fits() {
        let field = TextField::new("gm");
        assert_eq!(field.window(37), ("gm", 2));
        assert_eq!(TextField::new("").window(37), ("", 0));
    }

    #[test]
    fn viewport_slides_to_tail_when_overlong() {
        let field = TextField::new("a".repeat(50));
        let (visible, caret) = field.window(37);
        assert_eq!(visible, "a".repeat(37));
        assert_eq!(caret, 37);
    }

    #[test]
    fn viewport_never_splits_a_char() {
        let field = TextField::new("héllo wörld");
        let (visible, _) = field.window(5);
        assert!(visible.is_char_boundary(0));
        assert!(visible.is_char_boundary(visible.len()));
    }

    #[test]
    fn search_block_shows_plain_placeholder() {
        use ratatui::{Terminal, backend::TestBackend};

        use crate::theme::default::DARK_THEME;

        let area = Rect::new(0, 0, 24, 3);
        let field = TextField::new("");
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| {
                super::render_search_block(frame, area, &DARK_THEME, &field, false, "Search", None);
                let buffer = frame.buffer_mut();
                let row: String = (0..area.width)
                    .map(|x| {
                        buffer.content[(area.width + x) as usize]
                            .symbol()
                            .to_string()
                    })
                    .collect();
                assert!(row.contains("[S]"), "search icon fallback missing: {row:?}");
                assert_eq!(row.chars().skip(7).take(6).collect::<String>(), "Search");
            })
            .expect("test draw");
    }

    #[test]
    fn search_block_places_query_at_text_start() {
        use ratatui::{Terminal, backend::TestBackend};

        use crate::theme::default::DARK_THEME;

        let area = Rect::new(0, 0, 24, 3);
        let field = TextField::new("gm");
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| {
                super::render_search_block(frame, area, &DARK_THEME, &field, true, "Search", None);
                let buffer = frame.buffer_mut();
                let row: String = (0..area.width)
                    .map(|x| {
                        buffer.content[(area.width + x) as usize]
                            .symbol()
                            .to_string()
                    })
                    .collect();
                assert_eq!(row.chars().nth(7), Some('g'));
                assert_eq!(row.chars().nth(8), Some('m'));
            })
            .expect("test draw");
    }

    #[test]
    fn search_block_places_caret_at_text_start() {
        use ratatui::{Terminal, backend::TestBackend};

        use crate::theme::default::DARK_THEME;

        let area = Rect::new(0, 0, 24, 3);
        let field = TextField::new("");
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| {
                super::render_search_block(frame, area, &DARK_THEME, &field, true, "Search", None);
            })
            .expect("test draw");

        let cursor = terminal.backend().cursor_position();
        assert_eq!((cursor.x, cursor.y), (7, 1));
    }

    #[test]
    fn search_block_height_grows_then_caps() {
        let short = TextField::new("gm");
        assert_eq!(super::search_block_height(&short, true, 20), 3);

        let two_lines = TextField::new("hello worlds");
        assert_eq!(super::search_block_height(&two_lines, true, 20), 4);

        let many_lines = TextField::new("a".repeat(100));
        assert_eq!(super::search_block_height(&many_lines, true, 20), 5);
    }

    #[test]
    fn search_block_wraps_long_query_onto_following_rows() {
        use ratatui::{Terminal, backend::TestBackend};

        use crate::theme::default::DARK_THEME;

        // 20 wide leaves 11 text cells after the icon slot, so the query
        // wraps after "hello".
        let area = Rect::new(0, 0, 20, 5);
        let field = TextField::new("hello world again");
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| {
                super::render_search_block(frame, area, &DARK_THEME, &field, true, "Search", None);
                let buffer = frame.buffer_mut();
                let row_text = |y: u16| -> String {
                    (0..area.width)
                        .map(|x| {
                            buffer.content[(y * area.width + x) as usize]
                                .symbol()
                                .to_string()
                        })
                        .collect()
                };
                assert!(row_text(1).contains("hello"));
                assert!(row_text(2).contains("world again"));
            })
            .expect("test draw");
    }

    #[test]
    fn search_block_scrolls_to_keep_the_caret_visible() {
        use ratatui::{Terminal, backend::TestBackend};

        use crate::theme::default::DARK_THEME;

        // 12 wide leaves 4 text cells after the icon slot: the query wraps
        // one word per row and the viewport follows the caret.
        let area = Rect::new(0, 0, 12, 5);
        let field = TextField::new("aa bb cc dd ee ff gg hh");
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| {
                super::render_search_block(frame, area, &DARK_THEME, &field, true, "Search", None);
                let buffer = frame.buffer_mut();
                let row_text = |y: u16| -> String {
                    (0..area.width)
                        .map(|x| {
                            buffer.content[(y * area.width + x) as usize]
                                .symbol()
                                .to_string()
                        })
                        .collect()
                };
                assert!(!row_text(1).contains("aa"));
                assert!(row_text(3).contains("hh"));
            })
            .expect("test draw");
    }
}
