pub mod modals;
pub mod row;
pub mod state;
pub(crate) use state::*;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    widgets::Paragraph,
};

use crate::theme::Theme;
use crate::widgets::settings::row::{render_setting_row, wrap_description_lines};
use crate::widgets::settings::state::{SettingKeyMeta, SettingsPageState};

pub fn render_settings_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &SettingsPageState,
) {
    let area = area.inner(Margin::new(1, 1));
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

    let sections = if has_status {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0)])
            .split(area)
    };

    if let Some(message) = status_message {
        frame.render_widget(
            Paragraph::new(message).style(
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            sections[0],
        );
    }

    let list_area = sections[sections.len() - 1];
    if list_area.height == 0 {
        return;
    }

    let all_keys = state.visible_keys();
    let control_width = control_column_width(state.settings(), list_area.width);
    let description_lines: Vec<Vec<String>> = all_keys
        .iter()
        .map(|key| wrap_description_lines(key.description(), list_area.width))
        .collect();
    let heights: Vec<u16> = description_lines
        .iter()
        .map(|lines| 1 + lines.len() as u16)
        .collect();
    let (start, end) = visible_variable_range(&heights, state.selected_index(), list_area.height);

    let mut row_y = list_area.y;
    for (index, key) in all_keys[start..end].iter().enumerate() {
        let height = heights[start + index];
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
            control_width,
            &description_lines[start + index],
            theme,
        );
        row_y = row_y.saturating_add(row_area.height);
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
}
