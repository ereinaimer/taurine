use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, ListState, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::library::state::{
    LibraryDeleteModalState, LibraryExportModalField, LibraryExportModalState,
    LibraryExportResultModalState, LibraryImportModalField, LibraryImportModalState,
    LibraryImportResultModalState, LibraryImportRunVariablesModalState, LibraryModal,
    LibrarySelectState,
};
use crate::widgets::util::{self};

const EXPORT_RESULT_MODAL_TITLE: &str = "Export complete";
const IMPORT_RUN_VARIABLES_WARNING_LINES: [&str; 3] = [
    "CAUTION: This import contains [exec] variables that execute",
    "shell commands. Untrusted scripts can damage your system.",
    "Continue? [y/N]",
];

pub fn render_library_modal(frame: &mut Frame, area: Rect, theme: &Theme, modal: &LibraryModal) {
    match modal {
        LibraryModal::Export(state) => render_library_export_modal(frame, area, theme, state),
        LibraryModal::Import(state) => render_library_import_modal(frame, area, theme, state),
        LibraryModal::ExportResult(state) => {
            render_library_export_result_modal(frame, area, theme, state)
        }
        LibraryModal::ImportResult(state) => {
            render_library_import_result_modal(frame, area, theme, state)
        }
        LibraryModal::ConfirmImportRunVariables(state) => {
            render_library_import_run_variables_modal(frame, area, theme, state)
        }
        LibraryModal::ConfirmDelete(state) => {
            render_library_delete_modal(frame, area, theme, state)
        }
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

fn render_library_delete_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryDeleteModalState,
) {
    let width = if area.width > 44 {
        area.width.saturating_sub(4).min(64)
    } else {
        area.width.max(1)
    };
    let height = 8;
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, "Delete Trigger", theme);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new("Do you want to delete this trigger?")
            .style(Style::default().fg(theme.text_muted)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(util::truncate_to_width(state.name(), sections[1].width))
            .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        sections[1],
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
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Yes  ", yes_style),
            Span::raw("    "),
            Span::styled("  No  ", no_style),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        sections[2],
    );

    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        Paragraph::new(state.error().unwrap_or("")).style(feedback_style),
        sections[3],
    );
}

fn render_library_export_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryExportModalState,
) {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(76)
    } else {
        area.width.max(1)
    };
    let height = area.height.clamp(1, 10);
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, "Export Triggers", theme);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    util::render_modal_field_label(
        frame,
        sections[0],
        "Path",
        state.focus() == LibraryExportModalField::Path,
        None,
        theme,
    );
    util::render_modal_input_field(
        frame,
        sections[1],
        state.path(),
        state.path_cursor(),
        state.focus() == LibraryExportModalField::Path,
        theme,
    );

    let focused = |field| state.focus() == field;

    let password_focused = focused(LibraryExportModalField::Password);
    util::render_modal_password_row(
        frame,
        sections[2],
        "Password (optional)",
        &state.password_display_value(),
        state.password_cursor(),
        password_focused,
        false,
        false,
        theme,
    );

    let feedback_area = sections[3];
    let (feedback_text, feedback_style) = if let Some(error) = state.error() {
        (
            error,
            Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            "",
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        )
    };
    frame.render_widget(
        Paragraph::new(feedback_text).style(feedback_style),
        feedback_area,
    );

    let buttons_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(0),
            Constraint::Length(1),
            Constraint::Length(0),
        ])
        .split(sections[4]);
    util::render_action_buttons(
        frame,
        buttons_area[1],
        "Cancel",
        "Export",
        focused(LibraryExportModalField::ActionButton),
        state.button_selection(),
        theme,
    );

    match state.focus() {
        LibraryExportModalField::Path => {
            frame.set_cursor_position((
                sections[1].x + 1 + state.path_cursor() as u16,
                sections[1].y,
            ));
        }
        LibraryExportModalField::Password => {
            let label_width = sections[2].width.min(12);
            frame.set_cursor_position((
                sections[2].x + label_width + state.password_cursor() as u16,
                sections[2].y,
            ));
        }
        _ => {}
    }
}

fn render_library_import_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryImportModalState,
) {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(76)
    } else {
        area.width.max(1)
    };
    let popup = centered_rect(width, area.height.clamp(1, 11), area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, "Import Triggers", theme);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    util::render_modal_field_label(
        frame,
        sections[0],
        "Path",
        state.focus() == LibraryImportModalField::Path,
        None,
        theme,
    );
    util::render_modal_input_field(
        frame,
        sections[1],
        state.path(),
        state.path_cursor(),
        state.focus() == LibraryImportModalField::Path,
        theme,
    );

    let password_focused = state.focus() == LibraryImportModalField::Password;
    let password_disabled = state.is_encrypted() == Some(false);
    let show_red_asterisk = state.is_encrypted() == Some(true)
        && state.password().is_empty()
        && state.error().is_some();
    util::render_modal_password_row(
        frame,
        sections[3],
        "Password",
        &state.password_display_value(),
        state.password_cursor(),
        password_focused && !password_disabled,
        password_disabled,
        show_red_asterisk,
        theme,
    );

    util::render_modal_key_value_row(
        frame,
        sections[4],
        "Conflicts",
        state.conflict_mode_label(),
        state.focus() == LibraryImportModalField::ConflictMode,
        false,
        theme,
    );

    let feedback_area = sections[5];
    let (feedback_text, feedback_style) = if let Some(error) = state.error() {
        (
            error,
            Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            "",
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        )
    };
    frame.render_widget(
        Paragraph::new(feedback_text).style(feedback_style),
        feedback_area,
    );

    let buttons_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(0),
            Constraint::Length(1),
            Constraint::Length(0),
        ])
        .split(sections[6]);
    util::render_action_buttons(
        frame,
        buttons_area[1],
        "Cancel",
        "Import",
        state.focus() == LibraryImportModalField::ActionButton,
        state.button_selection(),
        theme,
    );

    match state.focus() {
        LibraryImportModalField::Path => {
            frame.set_cursor_position((
                sections[1].x + 1 + state.path_cursor() as u16,
                sections[1].y,
            ));
        }
        LibraryImportModalField::Password if state.is_encrypted() != Some(false) => {
            let label_width = sections[3].width.min(12);
            frame.set_cursor_position((
                sections[3].x + label_width + state.password_cursor() as u16,
                sections[3].y,
            ));
        }
        _ => {}
    }

    if let Some(selector) = state.selector() {
        render_library_select_modal(frame, area, theme, selector);
    }
}

fn render_library_import_run_variables_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryImportRunVariablesModalState,
) {
    let width = if area.width > 52 {
        area.width.saturating_sub(6).min(72)
    } else {
        area.width.max(1)
    };
    let popup = centered_rect(width, 9.min(area.height.max(1)), area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, "Run Variables Warning", theme);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(IMPORT_RUN_VARIABLES_WARNING_LINES[0]),
            Line::from(IMPORT_RUN_VARIABLES_WARNING_LINES[1]),
            Line::from(IMPORT_RUN_VARIABLES_WARNING_LINES[2]),
        ])
        .style(Style::default().fg(theme.text_muted)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(util::truncate_to_width(state.path(), sections[1].width))
            .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        sections[1],
    );

    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        Paragraph::new(state.error().unwrap_or("")).style(feedback_style),
        sections[2],
    );
}

fn render_library_import_result_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryImportResultModalState,
) {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(64)
    } else {
        area.width.max(1)
    };
    let popup = centered_rect(
        width,
        (state.lines().len() as u16 + 4).min(area.height.max(1)),
        area,
    );
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, "Import complete", theme);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(state.lines().len() as u16),
            Constraint::Min(0),
        ])
        .split(inner);

    let lines = state
        .lines()
        .iter()
        .map(|line| Line::from(line.as_str()))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(theme.text_muted)),
        sections[0],
    );
}

fn render_library_export_result_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryExportResultModalState,
) {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(76)
    } else {
        area.width.max(1)
    };
    let popup = centered_rect(width, 5.min(area.height.max(1)), area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, EXPORT_RESULT_MODAL_TITLE, theme);

    frame.render_widget(
        Paragraph::new(vec![Line::from(state.body())]).style(Style::default().fg(theme.text_muted)),
        inner,
    );
}

fn render_library_select_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibrarySelectState,
) {
    let width = if area.width > 24 {
        area.width.saturating_sub(4).min(44)
    } else {
        area.width.max(1)
    };
    let body_height = state.options.len().min(8) as u16;
    let desired_height = body_height + 4;
    let height = if area.height >= 6 {
        desired_height.min(area.height.saturating_sub(2).max(6))
    } else {
        area.height.max(1)
    };
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, state.title(), theme);

    let items: Vec<ListItem> = state
        .options
        .iter()
        .map(|option| ListItem::new(option.as_str()))
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected));

    let list = List::new(items).highlight_symbol("").highlight_style(
        Style::default()
            .bg(theme.surface)
            .fg(theme.text)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(list, inner, &mut list_state);
}
