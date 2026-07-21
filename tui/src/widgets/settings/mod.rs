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

use crate::theme::Theme;
use crate::widgets::settings::row::render_setting_row;
use crate::widgets::settings::state::SettingsPageState;
use crate::widgets::util;

pub fn render_settings_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &SettingsPageState,
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
    let spacious = use_spacious_settings_layout(list_area.height, all_keys.len(), 0);
    let row_height = if spacious { 2 } else { 1 };
    let visible_count = usize::from((list_area.height / row_height).max(1));
    let (start, end) = visible_setting_range(all_keys.len(), state.selected_index(), visible_count);
    let control_width = control_column_width(state.settings(), list_area.width);

    for (visible_index, key) in all_keys[start..end].iter().enumerate() {
        let row_area = Rect {
            x: list_area.x,
            y: list_area.y + (visible_index as u16 * row_height),
            width: list_area.width,
            height: row_height,
        };

        render_setting_row(
            frame,
            row_area,
            key,
            state.settings(),
            Some(*key) == Some(state.selected_key()),
            spacious,
            control_width,
            theme,
        );
    }
}

fn use_spacious_settings_layout(
    available_height: u16,
    settings_count: usize,
    reserved_rows: u16,
) -> bool {
    let required_height = settings_count as u16 * 2 + reserved_rows;
    available_height >= required_height
}

fn visible_setting_range(total: usize, selected: usize, visible_count: usize) -> (usize, usize) {
    util::visible_range(total, selected, visible_count)
}

fn control_column_width(settings: &taurine_core::settings::Settings, area_width: u16) -> u16 {
    use crate::widgets::settings::state::SettingKey;
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
