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
use crate::widgets::settings::row::{render_setting_row, wrap_description_lines};
use crate::widgets::settings::state::{SettingKeyMeta, SettingsPageState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsHit {
    Row(crate::widgets::settings::state::SettingKey),
    Search,
}

fn page_area(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    }
}

fn content_sections(area: Rect, has_status: bool) -> (Rect, Rect) {
    let sections = if has_status {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(area)
    };
    (
        sections[if has_status { 1 } else { 0 }],
        sections[sections.len() - 1],
    )
}

struct SettingsRows {
    control_width: u16,
    descriptions: Vec<Vec<String>>,
    heights: Vec<u16>,
    start: usize,
    end: usize,
}

fn compute_rows(list_area: Rect, state: &SettingsPageState) -> SettingsRows {
    let all_keys = state.visible_keys();
    let control_width = control_column_width(state.settings(), list_area.width);
    let descriptions: Vec<Vec<String>> = all_keys
        .iter()
        .map(|key| wrap_description_lines(key.description(), list_area.width.saturating_sub(2)))
        .collect();
    // honey: each row is 1 pad + title + descriptions + 1 pad.
    let heights: Vec<u16> = descriptions
        .iter()
        .map(|lines| 3 + lines.len() as u16)
        .collect();
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

/// Window start for the current state, used to anchor the view when a
/// visible row is clicked so the list does not jump.
pub(crate) fn visible_window_start(area: Rect, state: &SettingsPageState) -> usize {
    let area = page_area(area);
    if area.width == 0 || area.height == 0 {
        return 0;
    }
    let (list_area, _) = content_sections(area, state.status_message().is_some());
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
    let area = page_area(area);
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let (list_area, search_area) = content_sections(area, state.status_message().is_some());
    if mouse::contains(search_area, column, row) {
        return Some(SettingsHit::Search);
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
            return Some(SettingsHit::Row(*key));
        }
        row_y = row_y.saturating_add(height);
    }
    None
}

pub fn render_settings_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &SettingsPageState,
) {
    let area = page_area(area);
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

    let (list_area, search_area) = content_sections(area, has_status);

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
        state.search_query(),
        state.is_search_active(),
        "Search settings…",
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
        .map(|key| key.display_value(settings).chars().count())
        .max()
        .unwrap_or(10) as u16;

    let desired = (longest_value + 4).min(28);
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
            hit_test(area, &state, 30, 6),
            Some(SettingsHit::Row(SettingKey::PauseHotkey))
        );
        assert_eq!(hit_test(area, &state, 30, 24), Some(SettingsHit::Search));
        assert_eq!(hit_test(area, &state, 0, 0), None);
        assert_eq!(hit_test(area, &state, 30, 22), None);
    }

    #[test]
    fn hit_test_returns_none_for_empty_results() {
        let state = SettingsPageState {
            search_query: "zzz-no-such-setting".to_string(),
            ..SettingsPageState::default()
        };

        let area = ratatui::layout::Rect::new(26, 2, 71, 24);
        assert_eq!(hit_test(area, &state, 30, 3), None);
        assert_eq!(hit_test(area, &state, 30, 24), Some(SettingsHit::Search));
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
