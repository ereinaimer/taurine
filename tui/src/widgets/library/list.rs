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
const LIBRARY_ROW_LINES: usize = LIBRARY_ROW_HEIGHT as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryHit {
    Item(usize),
    SearchAt(usize),
}

/// Top line and clamped line offset for a list scrolled by `scroll`.
/// Short lists sit directly above the search bar; overflowing lists
/// scroll freely from the top edge.
fn list_layout(list_area: Rect, total: usize, scroll: usize) -> (i32, usize) {
    let used = total.saturating_mul(LIBRARY_ROW_LINES);
    let height = list_area.height as usize;
    if used <= height {
        ((list_area.y as usize + height - used) as i32, 0)
    } else {
        let scroll = scroll.min(used - height);
        let offset = (scroll % LIBRARY_ROW_LINES) as i32;
        (list_area.y as i32 - offset, scroll)
    }
}

/// Line offset that puts the selected row fully in view.
pub(crate) fn selection_scroll(
    list_area: Rect,
    total: usize,
    scroll: usize,
    selected: usize,
) -> usize {
    let max = total
        .saturating_mul(LIBRARY_ROW_LINES)
        .saturating_sub(list_area.height as usize);
    let top = selected.saturating_mul(LIBRARY_ROW_LINES);
    let bottom = top.saturating_add(LIBRARY_ROW_LINES);
    let viewport = list_area.height as usize;
    if top < scroll {
        top
    } else if bottom > scroll.saturating_add(viewport) {
        bottom.saturating_sub(viewport)
    } else {
        scroll
    }
    .min(max)
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
    let total = state.filtered_len();
    let (top, _) = list_layout(list_area, total, state.list_scroll());
    if (row as i32) < top {
        return None;
    }
    let index = (row as i32 - top) as usize / LIBRARY_ROW_LINES;
    (index < total).then_some(LibraryHit::Item(index))
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

    let total = state.filtered_len();
    let max_scroll = total
        .saturating_mul(LIBRARY_ROW_LINES)
        .saturating_sub(area.height as usize);
    state.set_list_scroll_max(max_scroll);
    // Keyboard selection pulls its row into view; wheel and hover never
    // move the cursor.
    if state.list_selection_moved()
        && let Some(selected) = state.selected_index()
    {
        let scroll = selection_scroll(area, total, state.list_scroll(), selected);
        state.jump_list_scroll(scroll);
    }
    state.clear_list_selection_moved();
    let scroll = state.advance_list_scroll().min(max_scroll);
    let (top, scroll) = list_layout(area, total, scroll);
    let first = scroll / LIBRARY_ROW_LINES;

    let mut y = top;
    for index in first..total {
        if y >= i32::from(area.bottom()) {
            break;
        }
        if let Some(item) = state.item_at_filtered(index) {
            render_library_item(
                frame,
                area,
                y,
                item,
                theme,
                state.selected_index() == Some(index),
            );
        }
        y += LIBRARY_ROW_LINES as i32;
    }
}

/// One list row; `top` may sit above or below the list so partially
/// scrolled rows clip instead of jumping.
fn render_library_item(
    frame: &mut Frame,
    list_area: Rect,
    top: i32,
    item: &crate::widgets::library::state::LibraryTrigger,
    theme: &Theme,
    selected: bool,
) {
    let row_style = if selected {
        Style::default().bg(theme.surface)
    } else {
        Style::default()
    };
    let clipped_top = top.max(i32::from(list_area.y));
    let clipped_bottom = (top + LIBRARY_ROW_LINES as i32).min(i32::from(list_area.bottom()));
    if clipped_bottom > clipped_top {
        frame.render_widget(
            Block::default().style(row_style),
            Rect {
                x: list_area.x,
                y: clipped_top as u16,
                width: list_area.width,
                height: (clipped_bottom - clipped_top) as u16,
            },
        );
    }

    let content_x = list_area.x.saturating_add(LIBRARY_ITEM_PAD_X);
    let content_width = list_area.width.saturating_sub(2 * LIBRARY_ITEM_PAD_X);
    if content_width == 0 {
        return;
    }
    let trigger_y = top + i32::from(LIBRARY_ITEM_PADDING);
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
    if trigger_y >= i32::from(list_area.y) && trigger_y < i32::from(list_area.bottom()) {
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
        frame.render_widget(
            Paragraph::new(top_line).style(trigger_style),
            Rect {
                x: content_x,
                y: trigger_y as u16,
                width: content_width,
                height: 1,
            },
        );
    }
    if preview_y >= i32::from(list_area.y) && preview_y < i32::from(list_area.bottom()) {
        frame.render_widget(
            Paragraph::new(util::truncate_to_width(item.preview(), content_width))
                .style(preview_style),
            Rect {
                x: content_x,
                y: preview_y as u16,
                width: content_width,
                height: 1,
            },
        );
    }
}
