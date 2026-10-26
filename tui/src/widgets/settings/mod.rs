pub mod modals;
pub mod row;
pub mod state;
pub(crate) use state::*;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::Paragraph,
};

use crate::terminal::mouse;
use crate::theme::Theme;
use crate::widgets::settings::row::{
    hotkey_value_rect, inline_value_rect, render_setting_row, select_value_rect,
    setting_toggle_rect, wrap_description_lines,
};
use crate::widgets::settings::state::{EditorKind, SettingKeyMeta, SettingsPageState};
use crate::widgets::toggle::{ToggleAnimations, ToggleId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsHit {
    Row(SettingKey),
    Toggle(SettingKey),
    /// Keycap pills of a hotkey setting; opens the on-screen recorder.
    HotkeyValue(SettingKey),
    /// Filled value field of a short setting; starts editing in place.
    InlineValue(SettingKey),
    /// Value of a picker setting; opens its option overlay.
    SelectValue(SettingKey),
    SearchAt(usize),
}

fn content_sections(
    area: Rect,
    has_status: bool,
    field: &crate::widgets::field::TextField,
    is_active: bool,
) -> (Rect, Rect) {
    let search_height = crate::widgets::util::search_block_height(field, is_active, area.width)
        .min(max_search_height(area.height, has_status));
    let sections = if has_status {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(search_height),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(search_height),
            ])
            .split(area)
    };
    (
        sections[if has_status { 1 } else { 0 }],
        sections[sections.len() - 1],
    )
}

/// Cap the search block so a long query never eats the list above it.
fn max_search_height(area_height: u16, has_status: bool) -> u16 {
    const MIN_LIST_HEIGHT: u16 = 3;
    let reserved = u16::from(has_status) + 1 + MIN_LIST_HEIGHT;
    area_height.saturating_sub(reserved).max(3)
}

struct SettingsRows {
    control_width: u16,
    descriptions: Vec<Vec<String>>,
    heights: Vec<u16>,
    start: usize,
    end: usize,
}

fn compute_rows(list_area: Rect, state: &SettingsPageState) -> SettingsRows {
    let (descriptions, heights, control_width) = row_shapes(list_area.width, state);
    let (start, end) = window_for(
        &heights,
        state.selected_index(),
        list_area.height,
        state.window_anchor,
    );
    SettingsRows {
        control_width,
        descriptions,
        heights,
        start,
        end,
    }
}

/// Row shapes at a list width, shared by rendering and popup sizing so
/// both agree on heights: wrapped descriptions, per-row heights, and
/// the control column width.
fn row_shapes(width: u16, state: &SettingsPageState) -> (Vec<Vec<String>>, Vec<u16>, u16) {
    let all_keys = state.visible_keys();
    let control_width = control_column_width(state.settings(), width);
    // honey: descriptions live in the label column only; wrapping at the
    // full row width would let them run under the value field.
    let description_width = width.saturating_sub(2).saturating_sub(control_width);
    let descriptions: Vec<Vec<String>> = all_keys
        .iter()
        .map(|key| wrap_description_lines(key.description(), description_width))
        .collect();
    // honey: each row is 1 pad + title + descriptions + 1 pad.
    let heights: Vec<u16> = descriptions
        .iter()
        .map(|lines| 3 + lines.len() as u16)
        .collect();
    (descriptions, heights, control_width)
}

/// Startup selection: highlight the last row that fits in the first
/// window, so the list opens at the top with the cursor on the row
/// touching the search bar. Mirrors the library list startup rule.
pub(crate) fn select_first_window_bottom(area: Rect, state: &mut SettingsPageState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (list_area, _) = content_sections(
        area,
        state.status_message().is_some(),
        state.search_field(),
        state.is_search_active(),
    );
    if list_area.height == 0 {
        return;
    }
    let (_, heights, _) = row_shapes(list_area.width, state);
    let mut used = 0u16;
    let mut selected = 0usize;
    for (index, height) in heights.iter().enumerate() {
        if used.saturating_add(*height) > list_area.height {
            break;
        }
        used = used.saturating_add(*height);
        selected = index;
    }
    state.selected = selected;
    state.window_anchor = None;
}

/// Window start for the current state, used to anchor the view when a
/// visible row is clicked so the list does not jump.
pub(crate) fn visible_window_start(area: Rect, state: &SettingsPageState) -> usize {
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let (list_area, _) = content_sections(
        area,
        state.status_message().is_some(),
        state.search_field(),
        state.is_search_active(),
    );
    compute_rows(list_area, state).start
}

/// Window `[start, end)` preferring a click-time `anchor` so the clicked
/// row stays where it was; falls back to bottom-anchored scrolling when
/// the anchor is stale or the selection left the window.
fn window_for(
    heights: &[u16],
    selected: usize,
    available: u16,
    anchor: Option<usize>,
) -> (usize, usize) {
    if let Some(start) = anchor.filter(|_| !heights.is_empty()) {
        let start = start.min(heights.len().saturating_sub(1));
        let mut end = start;
        let mut used = 0u16;
        while end < heights.len() && used.saturating_add(heights[end]) <= available {
            used = used.saturating_add(heights[end]);
            end += 1;
        }
        let end = end.max(start.saturating_add(1)).min(heights.len());
        if selected >= start && selected < end {
            return (start, end);
        }
    }
    visible_variable_range(heights, selected, available)
}

/// Hit-test a click at terminal cell `(column, row)` against the layout
/// `render_settings_content` produces for the same `area` and `state`.
pub(crate) fn hit_test(
    area: Rect,
    state: &SettingsPageState,
    column: u16,
    row: u16,
) -> Option<SettingsHit> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let (list_area, search_area) = content_sections(
        area,
        state.status_message().is_some(),
        state.search_field(),
        state.is_search_active(),
    );
    if mouse::contains(search_area, column, row) {
        return Some(SettingsHit::SearchAt(
            crate::widgets::util::search_block_index_at(
                state.search_field(),
                state.is_search_active(),
                search_area,
                column,
                row,
            ),
        ));
    }
    if !mouse::contains(list_area, column, row) {
        return None;
    }
    let all_keys = state.visible_keys();
    if all_keys.is_empty() {
        return None;
    }
    let rows = compute_rows(list_area, state);
    let mut row_y = rows_origin(list_area, &rows.heights, rows.start, rows.end);
    for (index, key) in all_keys[rows.start..rows.end].iter().enumerate() {
        let remaining = (list_area.y + list_area.height).saturating_sub(row_y);
        if remaining == 0 {
            break;
        }
        let height = rows.heights[rows.start + index].min(remaining);
        if row >= row_y && row < row_y.saturating_add(height) {
            let row_area = Rect {
                x: list_area.x,
                y: row_y,
                width: list_area.width,
                height,
            };
            if key.editor_kind() == EditorKind::Toggle
                && mouse::contains(
                    setting_toggle_rect(row_area, rows.control_width),
                    column,
                    row,
                )
            {
                return Some(SettingsHit::Toggle(*key));
            }
            if let Some(keycap) =
                hotkey_value_rect(row_area, rows.control_width, *key, state.settings())
                && mouse::contains(keycap, column, row)
            {
                return Some(SettingsHit::HotkeyValue(*key));
            }
            if let Some(field) = inline_value_rect(
                row_area,
                rows.control_width,
                *key,
                state.settings(),
                state.inline_edit(),
            ) && mouse::contains(field, column, row)
            {
                return Some(SettingsHit::InlineValue(*key));
            }
            if let Some(value) = select_value_rect(row_area, rows.control_width, *key)
                && mouse::contains(value, column, row)
            {
                return Some(SettingsHit::SelectValue(*key));
            }
            return Some(SettingsHit::Row(*key));
        }
        row_y = row_y.saturating_add(height);
    }
    None
}

/// Settings overlay geometry: 90% of the terminal in both axes at all
/// times. Shared by rendering and hit-testing so clicks land as drawn.
pub(crate) fn overlay_popup(area: Rect) -> Rect {
    crate::widgets::util::centered_rect(
        crate::widgets::util::percent_of(area.width, 90),
        crate::widgets::util::percent_of(area.height, 90),
        area,
    )
}

/// Content rect inside the popup, shared by overlay renders and
/// hit-testing so both agree on padding.
pub(crate) fn overlay_body(popup: Rect) -> Rect {
    crate::widgets::util::overlay_body(popup)
}

/// List rect below the header band and gap, shared by rendering and
/// hit-testing so both hand the same area to the page layout code.
pub(crate) fn overlay_content(body: Rect) -> Rect {
    Rect {
        x: body.x,
        y: body
            .y
            .saturating_add(crate::widgets::util::OVERLAY_HEADER_ROWS),
        width: body.width,
        height: body
            .height
            .saturating_sub(crate::widgets::util::OVERLAY_HEADER_ROWS),
    }
}

/// Settings as a near-fullscreen overlay: title row plus the exact page
/// list, search, and rows inside the popup body.
pub fn render_settings_overlay(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &SettingsPageState,
    animations: &ToggleAnimations,
    hovered: Option<ToggleId>,
) {
    let popup = overlay_popup(area);
    let chrome = crate::widgets::util::OverlayChrome::new(popup);
    chrome.render(frame, theme, "Settings");
    if chrome.body.width == 0 || chrome.body.height == 0 {
        return;
    }
    if chrome.content.width == 0 || chrome.content.height == 0 {
        return;
    }
    render_settings_content(frame, chrome.content, theme, state, animations, hovered);
}

pub fn render_settings_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &SettingsPageState,
    animations: &ToggleAnimations,
    hovered: Option<ToggleId>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
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

    let status_message = state.status_message();
    let has_status = status_message.is_some();

    let (list_area, search_area) = content_sections(
        area,
        has_status,
        state.search_field(),
        state.is_search_active(),
    );

    let status_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    if let Some(message) = status_message {
        frame.render_widget(
            Paragraph::new(message).style(
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            status_area,
        );
    }

    crate::widgets::util::render_search_block(
        frame,
        search_area,
        theme,
        state.search_field(),
        state.is_search_active(),
        "Search",
    );
    if list_area.height == 0 {
        return;
    }

    let all_keys = state.visible_keys();
    if all_keys.is_empty() {
        frame.render_widget(
            Paragraph::new("No settings match your search.")
                .style(Style::default().fg(theme.description)),
            Rect {
                x: list_area.x,
                y: list_area
                    .y
                    .saturating_add(list_area.height.saturating_sub(1)),
                width: list_area.width,
                height: 1.min(list_area.height),
            },
        );
        return;
    }
    let rows = compute_rows(list_area, state);
    let (start, end) = (rows.start, rows.end);

    let mut row_y = rows_origin(list_area, &rows.heights, start, end);
    for (index, key) in all_keys[start..end].iter().enumerate() {
        let height = rows.heights[start + index];
        let remaining = (list_area.y + list_area.height).saturating_sub(row_y);
        if remaining == 0 {
            break;
        }
        let row_area = Rect {
            x: list_area.x,
            y: row_y,
            width: list_area.width,
            height: height.min(remaining),
        };

        render_setting_row(
            frame,
            row_area,
            key,
            state.settings(),
            Some(*key) == Some(state.selected_key()),
            rows.control_width,
            &rows.descriptions[start + index],
            theme,
            animations,
            hovered,
            state.inline_edit(),
        );
        row_y = row_y.saturating_add(row_area.height);
    }
}

/// First-row y so the visible stack sits directly above the search bar
/// when it is shorter than the list area; overflowing stacks stay
/// top-aligned. Shared by rendering and hit-testing.
fn rows_origin(list_area: Rect, heights: &[u16], start: usize, end: usize) -> u16 {
    let used: u16 = heights[start..end].iter().sum();
    if used >= list_area.height {
        list_area.y
    } else {
        list_area
            .y
            .saturating_add(list_area.height.saturating_sub(used))
    }
}

/// Window `[start, end)` of variable-height rows containing `selected`
/// that fits in `available` rows. Anchors `selected` at the bottom first
/// (matching the previous fixed-height behavior), then fills below.
fn visible_variable_range(heights: &[u16], selected: usize, available: u16) -> (usize, usize) {
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

fn control_column_width(settings: &taurine_core::settings::Settings, area_width: u16) -> u16 {
    use crate::widgets::settings::state::{SettingKey, SettingKeyMeta};
    let longest_value = SettingKey::ALL
        .iter()
        .map(|key| {
            let value = key.display_value(settings);
            if key.editor_kind() == EditorKind::HotkeyCapture {
                crate::widgets::hotkey::display_width(&value) as u16 + 1
            } else {
                value.chars().count() as u16 + 4
            }
        })
        .max()
        .unwrap_or(10);

    let desired = longest_value.min(30);
    let max_width = area_width.saturating_sub(8);
    if max_width >= 10 {
        desired.min(max_width).max(10)
    } else {
        area_width.saturating_sub(2).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_wrap_within_the_label_column() {
        let state = SettingsPageState::default();
        let width = 60;
        let (descriptions, heights, control_width) = row_shapes(width, &state);
        let label_width = width.saturating_sub(2).saturating_sub(control_width) as usize;
        assert!(label_width > 0);
        for (key, lines) in state.visible_keys().iter().zip(&descriptions) {
            for line in lines {
                assert!(
                    line.chars().count() <= label_width,
                    "{key:?} overflowed: {line:?}"
                );
            }
        }
        assert!(
            descriptions.iter().any(|lines| lines.len() > 1),
            "expected at least one wrapped description"
        );
        for (lines, height) in descriptions.iter().zip(&heights) {
            assert_eq!(*height, 3 + lines.len() as u16);
        }
    }

    #[test]
    fn startup_selection_fills_the_first_window_from_the_top() {
        let area = ratatui::layout::Rect::new(26, 2, 71, 24);
        let mut state = SettingsPageState::default();

        select_first_window_bottom(area, &mut state);

        let (list_area, _) = content_sections(
            area,
            state.status_message().is_some(),
            state.search_field(),
            state.is_search_active(),
        );
        let (_, heights, _) = row_shapes(list_area.width, &state);
        let rows = compute_rows(list_area, &state);
        assert_eq!(rows.start, 0, "the first window should start at the top");
        let selected = state.selected_index();
        assert_eq!(
            selected,
            rows.end - 1,
            "the cursor should hug the search bar"
        );
        let used: u16 = heights[..=selected].iter().sum();
        assert!(used <= list_area.height);
        assert!(
            used.saturating_add(heights.get(selected + 1).copied().unwrap_or(u16::MAX))
                > list_area.height,
            "one more row should not fit"
        );
    }

    #[test]
    fn variable_range_fits_all_when_tall_enough() {
        assert_eq!(visible_variable_range(&[2, 2, 3], 0, 10), (0, 3));
    }

    #[test]
    fn variable_range_keeps_selected_visible() {
        let (start, end) = visible_variable_range(&[2, 2, 2, 2], 3, 4);
        assert!(start <= 3 && 3 < end);
        assert_eq!((start, end), (2, 4));
    }

    #[test]
    fn oversized_selected_row_returns_selected_only() {
        assert_eq!(visible_variable_range(&[2, 5, 2], 1, 3), (1, 2));
    }

    #[test]
    fn hit_test_finds_first_row_and_search() {
        use crate::widgets::settings::state::SettingKey;

        let area = ratatui::layout::Rect::new(26, 2, 71, 24);
        let state = SettingsPageState::default();

        assert_eq!(
            hit_test(area, &state, 30, 3),
            Some(SettingsHit::Row(SettingKey::PauseHotkey))
        );
        assert_eq!(
            hit_test(area, &state, 30, 24),
            Some(SettingsHit::SearchAt(0))
        );
        assert_eq!(hit_test(area, &state, 0, 0), None);
        assert_eq!(hit_test(area, &state, 30, 22), None);
    }

    #[test]
    fn hit_test_returns_none_for_empty_results() {
        let state = SettingsPageState {
            search: crate::widgets::field::TextField::new("zzz-no-such-setting"),
            ..SettingsPageState::default()
        };

        let area = ratatui::layout::Rect::new(26, 2, 71, 24);
        assert_eq!(hit_test(area, &state, 30, 3), None);
        // Line text starts two cells in: column 30 is the third character.
        assert_eq!(
            hit_test(area, &state, 30, 24),
            Some(SettingsHit::SearchAt(2))
        );
    }

    #[test]
    fn anchored_window_holds_clicked_row_in_place() {
        let heights = [4u16, 4, 4, 4, 4, 4];
        assert_eq!(window_for(&heights, 1, 20, Some(1)), (1, 6));
    }

    #[test]
    fn stale_anchor_falls_back_to_default_policy() {
        let heights = [4u16, 4, 4, 4, 4, 4];
        assert_eq!(window_for(&heights, 5, 12, Some(0)), (3, 6));
        assert_eq!(window_for(&heights, 5, 12, None), (3, 6));
    }

    #[test]
    fn switch_hit_returns_toggle_on_boolean_row() {
        let area = ratatui::layout::Rect::new(26, 2, 71, 24);
        let state = SettingsPageState::default();
        let (list_area, _) =
            content_sections(area, false, state.search_field(), state.is_search_active());
        let rows = compute_rows(list_area, &state);
        let all_keys = state.visible_keys();
        let mut row_y = rows_origin(list_area, &rows.heights, rows.start, rows.end);

        for (index, key) in all_keys[rows.start..rows.end].iter().enumerate() {
            let remaining = (list_area.y + list_area.height).saturating_sub(row_y);
            let height = rows.heights[rows.start + index].min(remaining);
            let row_area = Rect {
                x: list_area.x,
                y: row_y,
                width: list_area.width,
                height,
            };
            if key.editor_kind() == EditorKind::Toggle {
                let toggle = setting_toggle_rect(row_area, rows.control_width);
                assert_eq!(
                    hit_test(area, &state, toggle.x + 2, toggle.y),
                    Some(SettingsHit::Toggle(*key))
                );
                assert_eq!(
                    hit_test(area, &state, toggle.x.saturating_sub(1), toggle.y),
                    Some(SettingsHit::Row(*key))
                );
                return;
            }
            row_y = row_y.saturating_add(height);
        }
        panic!("no boolean setting row");
    }

    #[test]
    fn keycap_hit_returns_hotkey_value() {
        let area = ratatui::layout::Rect::new(26, 2, 71, 24);
        let state = SettingsPageState::default();
        let (list_area, _) =
            content_sections(area, false, state.search_field(), state.is_search_active());
        let rows = compute_rows(list_area, &state);
        let all_keys = state.visible_keys();
        let mut row_y = rows_origin(list_area, &rows.heights, rows.start, rows.end);

        for (index, key) in all_keys[rows.start..rows.end].iter().enumerate() {
            let remaining = (list_area.y + list_area.height).saturating_sub(row_y);
            let height = rows.heights[rows.start + index].min(remaining);
            let row_area = Rect {
                x: list_area.x,
                y: row_y,
                width: list_area.width,
                height,
            };
            if let Some(keycap) =
                hotkey_value_rect(row_area, rows.control_width, *key, state.settings())
            {
                assert_eq!(
                    hit_test(area, &state, keycap.x + keycap.width / 2, keycap.y),
                    Some(SettingsHit::HotkeyValue(*key))
                );
                assert_eq!(
                    hit_test(area, &state, keycap.x.saturating_sub(1), keycap.y),
                    Some(SettingsHit::Row(*key))
                );
                return;
            }
            row_y = row_y.saturating_add(height);
        }
        panic!("no hotkey setting row");
    }

    #[test]
    fn inline_value_hit_returns_inline_value() {
        let area = ratatui::layout::Rect::new(26, 2, 71, 24);
        let mut state = SettingsPageState::default();
        // honey: anchor the window on a known short field so the scan
        // is not limited to whichever rows happen to fit at default
        // selection.
        let key = SettingKey::ClipboardHistoryRetentionSecs;
        state.selected = state
            .visible_keys()
            .iter()
            .position(|candidate| *candidate == key)
            .expect("visible inline key");
        let (list_area, _) =
            content_sections(area, false, state.search_field(), state.is_search_active());
        let rows = compute_rows(list_area, &state);
        let all_keys = state.visible_keys();
        let mut row_y = rows_origin(list_area, &rows.heights, rows.start, rows.end);

        for (index, key) in all_keys[rows.start..rows.end].iter().enumerate() {
            let remaining = (list_area.y + list_area.height).saturating_sub(row_y);
            let height = rows.heights[rows.start + index].min(remaining);
            let row_area = Rect {
                x: list_area.x,
                y: row_y,
                width: list_area.width,
                height,
            };
            if let Some(field) = inline_value_rect(
                row_area,
                rows.control_width,
                *key,
                state.settings(),
                state.inline_edit(),
            ) {
                assert_eq!(
                    hit_test(area, &state, field.x + field.width / 2, field.y),
                    Some(SettingsHit::InlineValue(*key))
                );
                assert_eq!(
                    hit_test(area, &state, field.x.saturating_sub(1), field.y),
                    Some(SettingsHit::Row(*key))
                );
                return;
            }
            row_y = row_y.saturating_add(height);
        }
        panic!("no inline setting row");
    }

    #[test]
    fn select_value_hit_returns_select_value() {
        let area = ratatui::layout::Rect::new(26, 2, 71, 24);
        let mut state = SettingsPageState::default();
        let key = SettingKey::VoiceModel;
        state.selected = state
            .visible_keys()
            .iter()
            .position(|candidate| *candidate == key)
            .expect("visible select key");
        let (list_area, _) =
            content_sections(area, false, state.search_field(), state.is_search_active());
        let rows = compute_rows(list_area, &state);
        let all_keys = state.visible_keys();
        let mut row_y = rows_origin(list_area, &rows.heights, rows.start, rows.end);

        for (index, key) in all_keys[rows.start..rows.end].iter().enumerate() {
            let remaining = (list_area.y + list_area.height).saturating_sub(row_y);
            let height = rows.heights[rows.start + index].min(remaining);
            let row_area = Rect {
                x: list_area.x,
                y: row_y,
                width: list_area.width,
                height,
            };
            if let Some(value) = select_value_rect(row_area, rows.control_width, *key) {
                assert_eq!(
                    hit_test(area, &state, value.x + value.width / 2, value.y),
                    Some(SettingsHit::SelectValue(*key))
                );
                return;
            }
            row_y = row_y.saturating_add(height);
        }
        panic!("no select setting row");
    }

    #[test]
    fn clicked_row_hit_tests_stable_across_selection() {
        use crate::widgets::settings::state::SettingKey;

        let area = ratatui::layout::Rect::new(26, 2, 71, 24);
        let mut state = SettingsPageState::default();

        let first = hit_test(area, &state, 30, 14);
        assert!(matches!(first, Some(SettingsHit::Row(_))));
        let key = match first {
            Some(SettingsHit::Row(key)) => key,
            _ => SettingKey::PauseHotkey,
        };

        let anchor = visible_window_start(area, &state);
        state.click_setting(key, anchor);
        assert_eq!(hit_test(area, &state, 30, 14), Some(SettingsHit::Row(key)));
    }
}
