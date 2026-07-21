use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, ListState, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::settings::state::{
    ConfirmResetModalState, InputModalState, SelectModalState, SettingsModal,
};
use crate::widgets::util;

pub fn render_settings_modal(frame: &mut Frame, area: Rect, theme: &Theme, modal: &SettingsModal) {
    match modal {
        SettingsModal::Input(state) => render_input_modal(frame, area, theme, state),
        SettingsModal::Select(state) => render_select_modal(frame, area, theme, state),
        SettingsModal::ConfirmReset(state) => render_confirm_reset_modal(frame, area, theme, state),
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
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

fn render_input_modal(frame: &mut Frame, area: Rect, theme: &Theme, state: &InputModalState) {
    let width = if area.width > 32 {
        area.width.saturating_sub(4).min(64)
    } else {
        area.width.max(1)
    };
    let height = if area.height >= 8 {
        8
    } else {
        area.height.max(1)
    };
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, state.key().display_name(), theme);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(state.key().description()).style(
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(util::input_cursor_line(state.value(), state.cursor()))
            .style(Style::default().fg(theme.text).bg(theme.surface)),
        sections[1],
    );

    let feedback = state.error().unwrap_or("Enter Save   Esc Cancel");
    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    };
    frame.render_widget(Paragraph::new(feedback).style(feedback_style), sections[2]);
}

fn render_select_modal(frame: &mut Frame, area: Rect, theme: &Theme, state: &SelectModalState) {
    let width = if area.width > 24 {
        area.width.saturating_sub(4).min(44)
    } else {
        area.width.max(1)
    };
    let body_height = state.options().len().min(8) as u16;
    let desired_height = body_height + 5;
    let height = if area.height >= 6 {
        desired_height.min(area.height.saturating_sub(2).max(6))
    } else {
        area.height.max(1)
    };
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, state.key().display_name(), theme);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    let visible_rows = usize::from(sections[0].height.max(1));
    let (start, end) = visible_range(state.options().len(), state.selected_index(), visible_rows);
    let items: Vec<ListItem> = state.options()[start..end]
        .iter()
        .map(|option| ListItem::new(option.as_str()))
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_index().saturating_sub(start)));

    let list = List::new(items).highlight_symbol("").highlight_style(
        Style::default()
            .bg(theme.surface)
            .fg(theme.text)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(list, sections[0], &mut list_state);

    let feedback = state.error().unwrap_or("Enter Save   Esc Cancel");
    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    };
    frame.render_widget(Paragraph::new(feedback).style(feedback_style), sections[1]);
}

fn render_confirm_reset_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &ConfirmResetModalState,
) {
    let width = if area.width > 44 {
        area.width.saturating_sub(4).min(64)
    } else {
        area.width.max(1)
    };
    let height = if area.height >= 8 {
        8
    } else {
        area.height.max(1)
    };
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, "Reset Setting", theme);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!(
                "{} will be reset to its default value ({}).",
                state.key().display_name(),
                state.default_display_value()
            )),
            Line::from("Do you want to continue?"),
        ])
        .style(Style::default().fg(theme.text_muted)),
        sections[0],
    );

    let yes_style = if state.selected_yes() {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let no_style = if !state.selected_yes() {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };

    let buttons = Line::from(vec![
        Span::styled("  Yes  ", yes_style),
        Span::raw("    "),
        Span::styled("  No  ", no_style),
    ]);

    let feedback = state.error().unwrap_or("");
    if !feedback.is_empty() {
        frame.render_widget(
            Paragraph::new(feedback).style(
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            sections[1],
        );
    }

    frame.render_widget(
        Paragraph::new(buttons).alignment(Alignment::Center),
        sections[2],
    );
}

fn visible_range(total: usize, selected: usize, visible_count: usize) -> (usize, usize) {
    crate::widgets::util::visible_range(total, selected, visible_count)
}
