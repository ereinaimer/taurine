use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::library::state::{
    LibraryDeleteModalState, LibraryExportModalField, LibraryExportModalState,
    LibraryExportResultModalState, LibraryHeaderMenuState, LibraryImportModalField,
    LibraryImportModalState, LibraryImportResultModalState, LibraryImportRunVariablesModalState,
    LibraryModal, LibrarySelectState,
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
        LibraryModal::HeaderMenu(state) => {
            render_library_header_menu_modal(frame, area, theme, state)
        }
    }
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
    let popup = util::centered_rect(width, height, area);
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
    let popup = util::centered_rect(width, height, area);
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

    // honey: text fields position the real caret themselves via util.
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
    let popup = util::centered_rect(width, area.height.clamp(1, 11), area);
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

    // honey: text fields position the real caret themselves via util.

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
    let popup = util::centered_rect(width, 9.min(area.height.max(1)), area);
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
    let popup = util::centered_rect(
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
    let popup = util::centered_rect(width, 5.min(area.height.max(1)), area);
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
    let entries: Vec<util::OverlayEntry> = state
        .options
        .iter()
        .map(|option| util::OverlayEntry {
            label: option.clone(),
            detail: String::new(),
        })
        .collect();
    util::render_overlay_select(frame, area, theme, state.title(), &entries, state.selected);
}

/// Centered option menu for the detail header buttons (trigger type,
/// script language, run behavior), rendered through the shared
/// overlay component.
fn render_library_header_menu_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryHeaderMenuState,
) {
    let entries: Vec<util::OverlayEntry> = state
        .options()
        .iter()
        .zip(state.details())
        .map(|(option, detail)| util::OverlayEntry {
            label: option.clone(),
            detail: detail.clone(),
        })
        .collect();
    util::render_overlay_select(
        frame,
        area,
        theme,
        state.kind().title(),
        &entries,
        state.selected(),
    );
}
