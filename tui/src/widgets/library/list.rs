use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph},
};

use crate::terminal::mouse;
use crate::theme::Theme;
use crate::widgets::hotkey;
use crate::widgets::library::state::LibraryPageState;
use crate::widgets::util;

use super::{content_sections, left_content};

const LIBRARY_ITEM_HEIGHT: u16 = 2;
const LIBRARY_ITEM_PADDING: u16 = 1;
const LIBRARY_ITEM_PAD_X: u16 = 2;
const LIBRARY_ROW_HEIGHT: u16 = LIBRARY_ITEM_HEIGHT + 2 * LIBRARY_ITEM_PADDING;
/// Row height in lines, used by the free-scroll offset.
pub(crate) const LIBRARY_ROW_LINES: usize = LIBRARY_ROW_HEIGHT as usize;

/// Full rows only: the list is bottom-aligned so the last row sits
/// directly above the search bar and no row is ever cut in half.
fn row_viewport(list_area: Rect) -> Rect {
    let height = (list_area.height / LIBRARY_ROW_HEIGHT) * LIBRARY_ROW_HEIGHT;
    Rect {
        x: list_area.x,
        y: list_area.bottom().saturating_sub(height),
        width: list_area.width,
        height,
    }
}

/// Top line and clamped line offset for a list scrolled by `scroll`.
/// Short lists sit directly above the search bar; overflowing lists
/// scroll freely inside the full-row viewport.
fn list_layout(list_area: Rect, total: usize, scroll: usize) -> (i32, usize) {
    let viewport = row_viewport(list_area);
    let height = viewport.height as usize;
    if height == 0 {
        return (i32::from(viewport.y), 0);
    }
    let used = total.saturating_mul(LIBRARY_ROW_LINES);
    if used <= height {
        ((viewport.y as usize + height - used) as i32, 0)
    } else {
        let scroll = scroll.min(used - height);
        let offset = (scroll % LIBRARY_ROW_LINES) as i32;
        (viewport.y as i32 - offset, scroll)
    }
}

/// Line offset that puts the selected row fully in view.
pub(crate) fn selection_scroll(
    list_area: Rect,
    total: usize,
    scroll: usize,
    selected: usize,
) -> usize {
    let viewport = row_viewport(list_area).height as usize;
    if viewport == 0 {
        return 0;
    }
    let max = total
        .saturating_mul(LIBRARY_ROW_LINES)
        .saturating_sub(viewport);
    let top = selected.saturating_mul(LIBRARY_ROW_LINES);
    let bottom = top.saturating_add(LIBRARY_ROW_LINES);
    if top < scroll {
        top
    } else if bottom > scroll.saturating_add(viewport) {
        bottom.saturating_sub(viewport)
    } else {
        scroll
    }
    .min(max)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryHit {
    Item(usize),
    SearchAt(usize),
}

/// Hit-test a click at terminal cell `(column, row)` against the layout
/// `render_library_content` produces for the same `area` and `state`.
/// Returns the filtered-list position for item hits.
pub(crate) fn hit_test(
    area: Rect,
    state: &LibraryPageState,
    column: u16,
    row: u16,
) -> Option<LibraryHit> {
    let content = left_content(area, state.split_ratio(), state.detail_ratio());
    if content.width == 0 || content.height == 0 {
        return None;
    }
    let (list_area, search_area) = content_sections(
        content,
        state.status_message().is_some(),
        state.search_field(),
        state.is_search_active(),
    );
    if mouse::contains(search_area, column, row) {
        return Some(LibraryHit::SearchAt(util::search_block_index_at(
            state.search_field(),
            state.is_search_active(),
            search_area,
            column,
            row,
        )));
    }
    if !mouse::contains(list_area, column, row) {
        return None;
    }
    let viewport = row_viewport(list_area);
    if !mouse::contains(viewport, column, row) {
        return None;
    }
    let total = state.filtered_len();
    let (top, scroll) = list_layout(list_area, total, state.list_scroll());
    if (row as i32) < top {
        return None;
    }
    // The row under the pointer is offset by the rows scrolled past the
    // top edge, so clicks map to the item actually drawn there.
    let first = scroll / LIBRARY_ROW_LINES;
    let offset = (row as i32 - top) as usize / LIBRARY_ROW_LINES;
    let index = first.saturating_add(offset);
    if index >= total {
        return None;
    }
    let row_top = top + (offset * LIBRARY_ROW_LINES) as i32;
    let row_bottom = row_top + LIBRARY_ROW_LINES as i32;
    if row_top < i32::from(viewport.y) || row_bottom > i32::from(viewport.bottom()) {
        return None;
    }
    Some(LibraryHit::Item(index))
}

/// Whether the pointer is over the searchable list, wheel scrolling
/// target for the library page.
pub(crate) fn list_contains(area: Rect, state: &LibraryPageState, column: u16, row: u16) -> bool {
    let content = left_content(area, state.split_ratio(), state.detail_ratio());
    if content.width == 0 || content.height == 0 {
        return false;
    }
    let (list_area, _) = content_sections(
        content,
        state.status_message().is_some(),
        state.search_field(),
        state.is_search_active(),
    );
    mouse::contains(list_area, column, row)
}

pub fn render_library_list(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &mut LibraryPageState,
) {
    if let Some(message) = state.load_error() {
        frame.render_widget(
            Paragraph::new(message).style(
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            area,
        );
        return;
    }

    if area.height == 0 {
        return;
    }

    if let Some(message) = state.empty_state_message() {
        frame.render_widget(
            Paragraph::new(message).style(
                Style::default()
                    .fg(theme.text_muted)
                    .add_modifier(Modifier::DIM),
            ),
            Rect {
                x: area.x,
                y: area.y.saturating_add(area.height.saturating_sub(1)),
                width: area.width,
                height: 1.min(area.height),
            },
        );
        return;
    }

    let viewport = row_viewport(area);
    let rows = viewport.height as usize / LIBRARY_ROW_LINES;
    if rows == 0 {
        state.set_list_viewport(0, 0);
        return;
    }

    let total = state.filtered_len();
    let max_scroll = total
        .saturating_mul(LIBRARY_ROW_LINES)
        .saturating_sub(viewport.height as usize);
    state.set_list_viewport(rows, max_scroll);
    // Keyboard selection pulls its row into view; wheel and hover never
    // move the cursor.
    if state.list_selection_moved() {
        state.settle_list_scroll();
        if let Some(selected) = state.selected_index() {
            let scroll = selection_scroll(area, total, state.list_scroll(), selected);
            state.jump_list_scroll(scroll);
        }
    }
    state.clear_list_selection_moved();
    let scroll = state.advance_list_scroll().min(max_scroll);
    // While the wheel owns the list, the cursor is pinned to the row
    // touching the search bar. At the top there is nothing left to
    // scroll, so the cursor walks up the visible rows instead.
    let follow = state.list_follows_bottom() && total > 0;
    if follow {
        state.follow_list_scroll(state.follow_row().min(total - 1));
    }
    let (top, scroll) = list_layout(area, total, scroll);
    if follow && max_scroll > 0 && scroll > 0 {
        render_following_list(frame, state, viewport, top, scroll, theme);
    } else {
        render_static_list(frame, state, viewport, top, scroll, theme);
    }
}

/// Rows gliding under the fixed highlight window above the search bar.
fn render_following_list(
    frame: &mut Frame,
    state: &LibraryPageState,
    viewport: Rect,
    top: i32,
    scroll: usize,
    theme: &Theme,
) {
    let total = state.filtered_len();
    let bar_top = i32::from(viewport.bottom()) - i32::from(LIBRARY_ROW_HEIGHT);
    let bar_bottom = bar_top + i32::from(LIBRARY_ROW_HEIGHT);
    let viewport_top = i32::from(viewport.y);
    let viewport_bottom = i32::from(viewport.bottom());

    let mut y = top;
    for index in scroll / LIBRARY_ROW_LINES..total {
        if y >= viewport_bottom {
            break;
        }
        if y + LIBRARY_ROW_LINES as i32 > viewport_top
            && let Some(item) = state.item_at_filtered(index)
        {
            // Above the window the row keeps its normal styling.
            if y < bar_top {
                render_library_item(
                    frame,
                    viewport,
                    y as u16,
                    item,
                    theme,
                    false,
                    viewport_top..bar_top,
                );
            }
            // Inside the window it is the highlighted entry.
            if y + LIBRARY_ROW_LINES as i32 > bar_top {
                render_library_item(
                    frame,
                    viewport,
                    y as u16,
                    item,
                    theme,
                    true,
                    bar_top..bar_bottom,
                );
            }
        }
        y += LIBRARY_ROW_LINES as i32;
    }
}

/// Rows laid out whole; partial rows wait for the glide to bring them in.
fn render_static_list(
    frame: &mut Frame,
    state: &LibraryPageState,
    viewport: Rect,
    top: i32,
    scroll: usize,
    theme: &Theme,
) {
    let total = state.filtered_len();
    let viewport_top = i32::from(viewport.y);
    let viewport_bottom = i32::from(viewport.bottom());

    let mut y = top;
    for index in scroll / LIBRARY_ROW_LINES..total {
        if y >= viewport_bottom {
            break;
        }
        // Only whole rows are drawn: a row that would be cut by either
        // edge waits until the glide brings it fully into view.
        if y >= viewport_top
            && y + LIBRARY_ROW_LINES as i32 <= viewport_bottom
            && let Some(item) = state.item_at_filtered(index)
        {
            render_library_item(
                frame,
                viewport,
                y as u16,
                item,
                theme,
                state.selected_index() == Some(index),
                viewport_top..viewport_bottom,
            );
        }
        y += LIBRARY_ROW_LINES as i32;
    }
}

/// One list row, clipped to `clip` so it never paints outside its slot.
fn render_library_item(
    frame: &mut Frame,
    list_area: Rect,
    top: u16,
    item: &crate::widgets::library::state::LibraryTrigger,
    theme: &Theme,
    selected: bool,
    clip: std::ops::Range<i32>,
) {
    let row_top = i32::from(top);
    let row_bottom = row_top + i32::from(LIBRARY_ROW_HEIGHT);
    let visible_top = row_top.max(clip.start);
    let visible_bottom = row_bottom.min(clip.end);
    if visible_top >= visible_bottom {
        return;
    }
    let row_style = if selected {
        Style::default().bg(theme.surface)
    } else {
        Style::default()
    };
    frame.render_widget(
        Block::default().style(row_style),
        Rect {
            x: list_area.x,
            y: visible_top as u16,
            width: list_area.width,
            height: (visible_bottom - visible_top) as u16,
        },
    );

    let content_x = list_area.x.saturating_add(LIBRARY_ITEM_PAD_X);
    let content_width = list_area.width.saturating_sub(2 * LIBRARY_ITEM_PAD_X);
    if content_width == 0 {
        return;
    }
    let trigger_y = top + LIBRARY_ITEM_PADDING;
    let preview_y = trigger_y + 1;

    let trigger_style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    };
    let preview_style = if selected {
        Style::default().fg(theme.description).bg(theme.surface)
    } else {
        Style::default().fg(theme.description)
    };
    let (trigger_style, preview_style) = if item.is_enabled() {
        (trigger_style, preview_style)
    } else {
        (
            trigger_style.add_modifier(Modifier::DIM),
            preview_style.add_modifier(Modifier::DIM),
        )
    };

    // honey: hotkey rows split the chord into keycap pills; the pill
    // fill lightens on the selected card so it stays visible against
    // the card surface, and the gap keeps the card background.
    let top_line = if item.is_hotkey() {
        let (keycap_bg, base_bg) = if selected {
            (theme.keycap.on_surface, theme.surface)
        } else {
            (theme.keycap.bg, theme.background)
        };
        Line::from(hotkey::render_hotkey_spans(
            item.trigger(),
            content_width as usize,
            keycap_bg,
            theme.text,
            base_bg,
        ))
    } else {
        Line::from(util::truncate_to_width(item.trigger(), content_width))
    };
    if clip.contains(&i32::from(trigger_y)) {
        frame.render_widget(
            Paragraph::new(top_line).style(trigger_style),
            Rect {
                x: content_x,
                y: trigger_y,
                width: content_width,
                height: 1,
            },
        );
    }
    if clip.contains(&i32::from(preview_y)) {
        frame.render_widget(
            Paragraph::new(util::truncate_to_width(item.preview(), content_width))
                .style(preview_style),
            Rect {
                x: content_x,
                y: preview_y,
                width: content_width,
                height: 1,
            },
        );
    }
}
