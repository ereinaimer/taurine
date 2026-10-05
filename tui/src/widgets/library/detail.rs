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
use crate::widgets::library::state::{
    EditTarget, HeaderMenuKind, LibraryModal, LibraryPageState, LibraryTrigger,
};
use crate::widgets::util;

use super::split_panes;

pub(crate) const CONTENT_INNER_HEIGHT: usize = 16;
/// Empty-state token: three ROUNDED-border horizontals, matching the pane
/// border glyph set.
pub(crate) const EMPTY_TOKEN: &str = "───";
// honey: padded to the same 5-cell width as TOGGLE_OFF so flipping
// the switch never shifts the header layout.
const TOGGLE_ON: &str = "[ON ]";
const TOGGLE_OFF: &str = "[OFF]";

/// Fixed single-row offsets above the content section.
const DESCRIPTION_OFFSET: u16 = 2;
const BUTTONS_OFFSET: u16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailHit {
    EnableToggle,
    NameEditAt(usize),
    DescriptionEdit,
    ContentEditAt { row: usize, col: usize },
    Button(usize),
}

/// One rendered header button: horizontal offset in the buttons row,
/// width, and index into the button labels.
pub(crate) struct ButtonCell {
    pub(crate) x: u16,
    pub(crate) width: u16,
    pub(crate) index: usize,
}

fn button_labels(item: &LibraryTrigger) -> Vec<String> {
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
    labels
}

/// Button cells shared by rendering and hit-testing: two-space gaps,
/// buttons that no longer fit are dropped, type first.
pub(crate) fn button_layout(row_width: u16, labels: &[String]) -> Vec<ButtonCell> {
    let mut cells = Vec::new();
    let mut used = 0usize;
    for (index, label) in labels.iter().enumerate() {
        let width = format!(" {label} {CHEVRON_DOWN} ").chars().count();
        let gap = if index > 0 { 2 } else { 0 };
        if used.saturating_add(gap).saturating_add(width) > row_width as usize {
            break;
        }
        used = used.saturating_add(gap);
        cells.push(ButtonCell {
            x: used as u16,
            width: width as u16,
            index,
        });
        used = used.saturating_add(width);
    }
    cells
}

/// Which detail cell a click landed on. Toggle flips enable, name and
/// description open inline edits, content-box text opens the body
/// editor; everything else is read-only preview.
pub(crate) fn hit_test(
    area: Rect,
    list_ratio: f32,
    props_ratio: f32,
    state: &LibraryPageState,
    column: u16,
    row: u16,
) -> Option<DetailHit> {
    let content = center_content(area, list_ratio, props_ratio);
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
        if column >= content.x && column < start.saturating_sub(1) {
            return Some(DetailHit::NameEditAt(
                column.saturating_sub(content.x) as usize
            ));
        }
        return None;
    }
    if row == content.y.saturating_add(DESCRIPTION_OFFSET)
        && column >= content.x
        && column < content.x.saturating_add(content.width)
    {
        return Some(DetailHit::DescriptionEdit);
    }
    if row == content.y.saturating_add(BUTTONS_OFFSET) {
        let labels = button_labels(item);
        for cell in button_layout(content.width, &labels) {
            let start = content.x.saturating_add(cell.x);
            if column >= start && column < start.saturating_add(cell.width) {
                return Some(DetailHit::Button(cell.index));
            }
        }
    }
    // honey: text rows inside the box map straight onto wrapped visual
    // rows; the border frame itself is not editable. Text starts two
    // cells in (border plus padding), mirroring the render path.
    let box_top = content.y.saturating_add(CONTENT_BOX_TOP);
    let text_top = box_top.saturating_add(1);
    let text_bottom = text_top.saturating_add(CONTENT_INNER_HEIGHT as u16);
    let text_x = content.x.saturating_add(2);
    if row >= text_top
        && row < text_bottom
        && row < content.y.saturating_add(content.height)
        && column >= text_x
        && column < content.x.saturating_add(content.width).saturating_sub(1)
    {
        return Some(DetailHit::ContentEditAt {
            row: row.saturating_sub(text_top) as usize,
            col: column.saturating_sub(text_x) as usize,
        });
    }
    None
}

/// True when the cell sits inside the center-pane content area.
pub(crate) fn detail_contains(
    area: Rect,
    list_ratio: f32,
    props_ratio: f32,
    column: u16,
    row: u16,
) -> bool {
    let content = center_content(area, list_ratio, props_ratio);
    column >= content.x
        && column < content.x.saturating_add(content.width)
        && row >= content.y
        && row < content.y.saturating_add(content.height)
}

/// Wrap one source line to the pane width: greedy whole words, one
/// visual row per entry, each with its source char range. Words move
/// whole to the next row; only a lone word longer than the width
/// hard-breaks (paths, URLs). The consumed break space is dropped so
/// continuation rows start clean; interior spacing is preserved.
/// Blank lines stay blank; tabs flatten.
pub(crate) fn wrap_content_spans(line: &str, width: u16) -> Vec<(String, std::ops::Range<usize>)> {
    let width = (width.max(1)) as usize;
    let flat = line.replace('\t', "  ");
    let trimmed = flat.trim_end();
    if trimmed.is_empty() {
        return vec![(String::new(), 0..0)];
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let mut rows = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        if chars.len() - start <= width {
            rows.push((chars[start..].iter().collect(), start..chars.len()));
            break;
        }
        let end = start + width;
        match chars[start..end].iter().rposition(|ch| *ch == ' ') {
            Some(gap) => {
                rows.push((
                    chars[start..start + gap].iter().collect(),
                    start..start + gap,
                ));
                start += gap + 1;
            }
            None => {
                // honey: overlong word hard-breaks; the only cut this
                // wrapper ever makes.
                rows.push((chars[start..end].iter().collect(), start..end));
                start = end;
                while start < chars.len() && chars[start] == ' ' {
                    start += 1;
                }
            }
        }
    }
    rows
}

/// Wrap source lines to the pane width, one visual row per chunk.
/// Blank lines stay blank; tabs flatten; trailing space trimmed so it
/// never forces an extra row. No ellipsis, nothing clipped.
pub(crate) fn wrap_content_lines(content: &str, width: u16) -> Vec<String> {
    let mut rows = Vec::new();
    for line in content.lines() {
        for (text, _) in wrap_content_spans(line, width) {
            rows.push(text);
        }
    }
    rows
}

/// Full flow layout, top to bottom: header 0, blank 1, description 2,
/// blank 3, buttons 4, blank 5, static content box (18 rows: border +
/// 16 text + border). Nothing follows the box.
struct DetailLayout {
    content_rows: Vec<(u16, String)>,
    rest: usize,
}

const CONTENT_BOX_TOP: u16 = 6;
const CONTENT_BOX_HEIGHT: u16 = CONTENT_INNER_HEIGHT as u16 + 2;

/// Text width inside the content box: border plus one cell of padding
/// each side, mirroring the search box.
pub(crate) fn content_text_width(box_width: u16) -> u16 {
    box_width.saturating_sub(4)
}

/// Map a source caret (row, column) onto wrapped visual (row, column)
/// at `width`, using the same word wrap as the render path. A caret
/// exactly on a break lands on the next row, column 0.
pub(crate) fn content_visual_cursor(
    lines: &[String],
    row: usize,
    col: usize,
    width: u16,
) -> (usize, usize) {
    let mut visual = 0usize;
    for (index, line) in lines.iter().enumerate() {
        let spans = wrap_content_spans(line, width);
        let count = spans.len().max(1);
        if index == row.min(lines.len().saturating_sub(1)) && !lines.is_empty() {
            let clamped = col.min(line.chars().count());
            for (offset, (text, range)) in spans.iter().enumerate() {
                if clamped < range.end || offset + 1 == spans.len() {
                    // honey: the break space itself belongs to no row;
                    // it renders as the next row's start.
                    let cell = clamped
                        .saturating_sub(range.start)
                        .min(text.chars().count());
                    return (visual + offset, cell);
                }
            }
        }
        visual += count;
    }
    (visual.saturating_sub(1), 0)
}

/// Map a wrapped visual row + column onto the source (row, column).
/// Edit mode shows lines unwrapped, so clicks land on the source row
/// with an approximate column; placement clamps into the visual row.
pub(crate) fn content_source_cell(
    item: &LibraryTrigger,
    width: u16,
    visual_row: usize,
    visual_col: usize,
) -> (usize, usize) {
    let mut acc = 0usize;
    let mut last = (0usize, 0usize);
    for (srow, line) in item.content().lines().enumerate() {
        let spans = wrap_content_spans(line, width);
        let n = spans.len().max(1);
        if visual_row < acc + n {
            let (text, range) = &spans[(visual_row - acc).min(spans.len().saturating_sub(1))];
            return (srow, range.start + visual_col.min(text.chars().count()));
        }
        acc += n;
        last = (srow, line.chars().count());
    }
    last
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
    Some(DetailLayout { content_rows, rest })
}

/// Max scroll offset for the content section at this pane size.
pub(crate) fn content_scroll_max(height: u16, width: u16, item: &LibraryTrigger) -> usize {
    let Some(layout) = detail_layout(height, width, item, 0) else {
        return 0;
    };
    layout.rest
}

/// Center-pane content: two cells of padding on the left, one on the
/// right. Shared by rendering and mouse hit-testing.
pub(crate) fn center_content(area: Rect, list_ratio: f32, props_ratio: f32) -> Rect {
    let split = split_panes(area, list_ratio, props_ratio);
    Rect {
        x: split.center.x.saturating_add(2),
        y: split.center.y.saturating_add(1),
        width: split.center.width.saturating_sub(3),
        height: split.center.height.saturating_sub(1),
    }
}

pub(crate) fn render_detail(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
) {
    let content = center_content(area, state.split_ratio(), state.detail_ratio());
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
    render_header_row(frame, content, theme, state, item);
    render_description_row(frame, content, theme, state, item);
    render_buttons_row(frame, content, theme, state, item);
    if let Some(layout) = detail_layout(content.height, content.width, item, scroll) {
        render_content_rows(frame, content, theme, state, &layout);
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

fn render_header_row(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
    item: &LibraryTrigger,
) {
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
    let width = toggle_width(item);
    let available = row.width.saturating_sub(width).saturating_sub(1);
    // honey: editing shows the caret-anchored viewport with a real
    // caret, mirroring the search box; read mode shows the truncated
    // display name.
    if let Some(edit) = state.name_edit() {
        let (visible, caret) = edit.field().window(available);
        let gap = available.saturating_sub(visible.chars().count() as u16);
        let line = Line::from(vec![
            Span::styled(
                visible.to_string(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" ".repeat(gap as usize + 1)),
            toggle,
        ]);
        frame.render_widget(Paragraph::new(line), row);
        let (cx, cy) = util::caret_position(row.x, row.y, caret, available);
        frame.set_cursor_position((cx, cy));
        return;
    }
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

fn render_description_row(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
    item: &LibraryTrigger,
) {
    if DESCRIPTION_OFFSET >= area.height {
        return;
    }
    let row = row_area(area, DESCRIPTION_OFFSET);
    if let Some(edit) = state
        .edit()
        .filter(|edit| edit.target() == EditTarget::Description)
    {
        let (visible, caret) = edit.line().window(row.width);
        let gap = row.width.saturating_sub(visible.chars().count() as u16);
        let line = Line::from(vec![
            Span::styled(visible.to_string(), Style::default().fg(theme.text)),
            Span::raw(" ".repeat(gap as usize)),
        ]);
        frame.render_widget(Paragraph::new(line), row);
        let (cx, cy) = util::caret_position(row.x, row.y, caret, row.width);
        frame.set_cursor_position((cx, cy));
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

/// Header button: 1-cell horizontal padding inside a background
/// fill, with a chevron pointing up while its menu is open.
fn button_spans(label: &str, open: bool, theme: &Theme) -> (Vec<Span<'static>>, usize) {
    let chevron = if open { CHEVRON_UP } else { CHEVRON_DOWN };
    let text = format!(" {label} {chevron} ");
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

fn render_buttons_row(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
    item: &LibraryTrigger,
) {
    if BUTTONS_OFFSET >= area.height {
        return;
    }
    let row = row_area(area, BUTTONS_OFFSET);
    let labels = button_labels(item);
    // honey: same flip convention as the usage toggle: the spawning
    // button points up while its menu overlay is open.
    let open_kind = match state.modal() {
        Some(LibraryModal::HeaderMenu(menu)) => Some(menu.kind()),
        _ => None,
    };
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut cursor = 0u16;
    for cell in button_layout(row.width, &labels) {
        if cell.x > cursor {
            spans.push(Span::raw(
                " ".repeat(cell.x.saturating_sub(cursor) as usize),
            ));
        }
        let (button, _) = button_spans(
            &labels[cell.index],
            open_kind == Some(dropdown_kind_for_button(cell.index)),
            theme,
        );
        spans.extend(button);
        cursor = cell.x.saturating_add(cell.width);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
}

/// Header button index onto its menu: type first, then script
/// language and run behavior. Hit-testing only fires on rendered
/// buttons, so trailing indices are always script menus.
pub(crate) const fn dropdown_kind_for_button(index: usize) -> HeaderMenuKind {
    match index {
        0 => HeaderMenuKind::InvocationType,
        1 => HeaderMenuKind::Interpreter,
        _ => HeaderMenuKind::Behavior,
    }
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

/// Static content box: fixed 18 rows with the rounded search-box border
/// and 16 scrollable text rows inside, no scrollbar.
/// The box never grows or shrinks with the content length.
fn render_content_rows(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
    layout: &DetailLayout,
) {
    use ratatui::symbols::border;
    use ratatui::widgets::{Block, Borders};

    let popup = Rect {
        x: area.x,
        y: area.y.saturating_add(CONTENT_BOX_TOP),
        width: area.width,
        height: CONTENT_BOX_HEIGHT,
    };
    // honey: open bottom — corners continue as walls, bottom edge is
    // blank, so the sides read as running off the pane.
    let open_set = border::Set {
        bottom_left: border::ROUNDED.vertical_left,
        bottom_right: border::ROUNDED.vertical_right,
        horizontal_bottom: " ",
        ..border::ROUNDED
    };
    let wall = Style::default().fg(theme.border);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(open_set)
        .border_style(wall);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    // honey: decorative walls from the box bottom to the terminal bottom
    // edge, straight through the pane padding; clicks fall through
    // exactly as on empty space.
    let walls_end = frame.area().y.saturating_add(frame.area().height);
    let mut y = popup.y.saturating_add(popup.height);
    while y < walls_end {
        for x in [
            popup.x,
            popup.x.saturating_add(popup.width.saturating_sub(1)),
        ] {
            frame.render_widget(
                Paragraph::new(Line::from("│")).style(wall),
                Rect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                },
            );
        }
        y = y.saturating_add(1);
    }
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
    if let Some(edit) = state
        .edit()
        .filter(|edit| edit.target() == EditTarget::Content)
    {
        render_content_editor(frame, theme, text, edit, state.detail_scroll());
        return;
    }
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

/// Body editor: session lines with per-line horizontal viewports and a
/// real caret. Non-caret lines show their head, truncated like read mode.
fn render_content_editor(
    frame: &mut Frame,
    theme: &Theme,
    text: Rect,
    edit: &crate::widgets::library::state::ActiveEdit,
    scroll: usize,
) {
    // honey: identical wrapping to read mode; the caret paints on the
    // wrapped visual row, so long paragraphs stay editable in place.
    // Selected ranges render reversed so the selection is visible.
    let body = edit.body();
    let (caret_row, caret_col) = body.cursor();
    let (caret_visual, caret_cell) =
        content_visual_cursor(body.lines(), caret_row, caret_col, text.width);
    let selection = body.selection_range();
    let mut visual = 0usize;
    'rows: for (source_row, line) in body.lines().iter().enumerate() {
        for (chunk, range) in wrap_content_spans(line, text.width) {
            if visual < scroll {
                visual = visual.saturating_add(1);
                continue;
            }
            let shown = visual.saturating_sub(scroll);
            if shown >= text.height as usize {
                break 'rows;
            }
            let row = Rect {
                x: text.x,
                y: text.y.saturating_add(shown as u16),
                width: text.width,
                height: 1,
            };
            let paragraph =
                if let Some(((sel_start_row, sel_start_col), (sel_end_row, sel_end_col))) =
                    selection
                    && source_row >= sel_start_row
                    && source_row <= sel_end_row
                {
                    let lo = if source_row == sel_start_row {
                        sel_start_col
                    } else {
                        range.start
                    }
                    .max(range.start);
                    let hi = if source_row == sel_end_row {
                        sel_end_col
                    } else {
                        range.end
                    }
                    .min(range.end);
                    let chars: Vec<char> = chunk.chars().collect();
                    let base = Style::default().fg(theme.text);
                    if lo < hi {
                        let from = lo.saturating_sub(range.start).min(chars.len());
                        let to = hi.saturating_sub(range.start).min(chars.len());
                        let selected: String = chars[from..to].iter().collect();
                        let head: String = chars[..from].iter().collect();
                        let tail: String = chars[to..].iter().collect();
                        let lit = Style::default()
                            .fg(theme.text)
                            .add_modifier(Modifier::REVERSED);
                        Paragraph::new(Line::from(vec![
                            Span::styled(head, base),
                            Span::styled(selected, lit),
                            Span::styled(tail, base),
                        ]))
                    } else {
                        Paragraph::new(Line::from(Span::styled(chunk.clone(), base)))
                    }
                } else {
                    Paragraph::new(
                        Line::from(chunk.as_str()).style(Style::default().fg(theme.text)),
                    )
                };
            frame.render_widget(paragraph, row);
            if visual == caret_visual {
                // honey: no backgrounds while editing; the real caret alone
                // marks the position.
                let (cx, cy) = util::caret_position(row.x, row.y, caret_cell, text.width);
                frame.set_cursor_position((cx, cy));
            }
            visual = visual.saturating_add(1);
        }
    }
}

/// Sibling invocations, excluding the currently displayed trigger.
/// Nothing renders when the trigger has no siblings.
pub(crate) fn sibling_aliases<'a>(aliases: &'a [String], current: &str) -> Vec<&'a str> {
    aliases
        .iter()
        .map(String::as_str)
        .filter(|alias| *alias != current)
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
