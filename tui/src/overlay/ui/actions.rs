use crate::theme::builtin::DARK_THEME;
use crate::widgets::library::ButtonSelection;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Paragraph},
};
pub(crate) fn render_action_buttons_overlay(
    frame: &mut Frame,
    area: Rect,
    cancel_label: &str,
    confirm_label: &str,
    is_focused: bool,
    selection: ButtonSelection,
) {
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
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.button.active_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text).bg(DARK_THEME.surface)
    };
    frame.render_widget(
        Paragraph::new(cancel_text).style(cancel_style),
        btn_layout[1],
    );

    let confirm_style = if is_focused && selection == ButtonSelection::Confirm {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.button.active_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text).bg(DARK_THEME.surface)
    };
    frame.render_widget(
        Paragraph::new(confirm_text).style(confirm_style),
        btn_layout[3],
    );
}

pub(crate) fn fill_bg(frame: &mut Frame) {
    frame.render_widget(
        Block::default().style(Style::default().bg(DARK_THEME.background)),
        frame.area(),
    );
}
