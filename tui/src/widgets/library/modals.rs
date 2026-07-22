use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::library::state::{
    LibraryDeleteModalState, LibraryEditorModalState, LibraryExportModalField,
    LibraryExportModalState, LibraryExportResultModalState, LibraryImportModalField,
    LibraryImportModalState, LibraryImportResultModalState, LibraryImportRunVariablesModalState,
    LibraryModal, LibraryModalField, LibrarySelectState,
};
use crate::widgets::util::{self, yes_no_label};

const EXPORT_RESULT_MODAL_TITLE: &str = "Export complete";
const IMPORT_RUN_VARIABLES_WARNING_LINES: [&str; 3] = [
    "CAUTION: This import contains [exec] variables that execute",
    "shell commands. Untrusted scripts can damage your system.",
    "Continue? [y/N]",
];

pub fn render_library_modal(frame: &mut Frame, area: Rect, theme: &Theme, modal: &LibraryModal) {
    match modal {
        LibraryModal::Editor(state) => render_library_editor_modal(frame, area, theme, state),
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

fn render_library_editor_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryEditorModalState,
) {
    let width = ((area.width as u32 * 4) / 5) as u16;
    let height = ((area.height as u32 * 4) / 5) as u16;
    let popup = centered_rect(width.max(48), height.max(12), area);
    frame.render_widget(Clear, popup);
    let inner = util::render_modal_block(frame, popup, "Trigger", theme);

    let header_rows = 4;
    let available_after_headers = inner.height.saturating_sub(header_rows);
    let editable_metadata_rows = if state.is_script_kind() { 4 } else { 2 };
    let metadata_len = state.metadata_rows().len() as u16 + editable_metadata_rows;
    let min_content_height = if available_after_headers >= 6 {
        4
    } else {
        available_after_headers.max(1)
    };
    let metadata_height = metadata_len.min(available_after_headers.saturating_sub(1));
    let mut content_height = available_after_headers.saturating_sub(metadata_height);
    if content_height < min_content_height {
        content_height = min_content_height.min(available_after_headers.max(1));
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(content_height.max(1)),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    util::render_modal_field_label(
        frame,
        sections[0],
        "Trigger",
        state.focus() == LibraryModalField::Trigger,
        None,
        theme,
    );
    util::render_modal_input_field(
        frame,
        sections[1],
        state.trigger(),
        state.trigger_cursor(),
        state.focus() == LibraryModalField::Trigger,
        theme,
    );

    util::render_modal_field_label(
        frame,
        sections[2],
        state.content_label(),
        state.focus() == LibraryModalField::Content,
        state.content_line_indicator(sections[3].height),
        theme,
    );
    render_modal_content_field(frame, sections[3], theme, state);

    render_modal_metadata(frame, sections[4], theme, state, state.metadata_rows());
    render_library_editor_feedback(frame, sections[5], theme, state);

    if state.focus() == LibraryModalField::Trigger {
        frame.set_cursor_position((
            sections[1].x + 1 + state.trigger_cursor() as u16,
            sections[1].y,
        ));
    } else if state.focus() == LibraryModalField::Content {
        let (cursor_line, cursor_col) = crate::widgets::library::actions::line_col_for_char_index(
            state.content(),
            state.content_cursor(),
        );
        let scroll = state.effective_content_scroll(sections[3].height);
        frame.set_cursor_position((
            sections[3].x + 1 + cursor_col as u16,
            sections[3].y + cursor_line.saturating_sub(scroll) as u16,
        ));
    }

    if let Some(selector) = state.selector() {
        render_library_select_modal(frame, area, theme, selector);
    }
}

fn render_modal_content_field(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryEditorModalState,
) {
    let focused = state.focus() == LibraryModalField::Content;
    let bg = if focused {
        theme.surface
    } else {
        theme.background
    };
    let block = Block::default().style(Style::default().bg(bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let scroll = state.effective_content_scroll(inner.height);
    let (cursor_line, cursor_col) = crate::widgets::library::actions::line_col_for_char_index(
        state.content(),
        state.content_cursor(),
    );
    let visible_lines = state.visible_content_lines(inner.height);
    let rendered = visible_lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let absolute_line = scroll + index;
            if focused && absolute_line == cursor_line {
                util::input_cursor_line(&line, cursor_col)
            } else {
                Line::from(line)
            }
        })
        .collect::<Vec<_>>();

    let content_style = if focused {
        Style::default()
            .fg(theme.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(bg)
    };
    frame.render_widget(Paragraph::new(rendered).style(content_style), inner);
}

fn render_modal_metadata(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryEditorModalState,
    metadata_rows: &[crate::widgets::library::state::LibraryMetadataRow],
) {
    let mut rows: Vec<(&str, String, bool, bool)> = Vec::with_capacity(metadata_rows.len() + 2);
    rows.push((
        "Kind",
        state.kind_label().to_string(),
        state.focus() == LibraryModalField::Kind,
        false,
    ));
    rows.push((
        "Target OS",
        state.target_os().to_string(),
        state.focus() == LibraryModalField::TargetOs,
        false,
    ));
    if state.is_script_kind() {
        rows.push((
            "Language",
            state.language_label().to_string(),
            state.focus() == LibraryModalField::Language,
            false,
        ));
        rows.push((
            "Mode",
            state.mode_label().to_string(),
            state.focus() == LibraryModalField::Mode,
            false,
        ));
    }
    rows.extend(
        metadata_rows
            .iter()
            .map(|row| (row.label(), row.value().to_string(), false, true)),
    );

    let render_count = rows.len().min(area.height as usize);
    for (index, (label, value, focused, quiet)) in rows.into_iter().take(render_count).enumerate() {
        let row_area = Rect {
            x: area.x,
            y: area.y + index as u16,
            width: area.width,
            height: 1,
        };
        util::render_modal_key_value_row(frame, row_area, label, &value, focused, quiet, theme);
    }
}

fn render_library_editor_feedback(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryEditorModalState,
) {
    let (text, style) = if let Some(error) = state.error() {
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
    frame.render_widget(Paragraph::new(text).style(style), area);
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
    let height = area.height.clamp(1, 14);
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

    let encrypt = state.encrypt();
    let focused = |field| state.focus() == field;

    util::render_modal_key_value_row(
        frame,
        sections[2],
        "Encrypt",
        yes_no_label(encrypt),
        focused(LibraryExportModalField::Encrypt),
        false,
        theme,
    );

    let password_focused = focused(LibraryExportModalField::Password);
    util::render_modal_password_row(
        frame,
        sections[3],
        "Password",
        &state.password_display_value(),
        state.password_cursor(),
        password_focused && encrypt,
        !encrypt,
        false,
        theme,
    );

    util::render_modal_key_value_row(
        frame,
        sections[4],
        "Settings",
        yes_no_label(state.include_settings()),
        focused(LibraryExportModalField::IncludeSettings),
        false,
        theme,
    );
    let sensitive_focused = focused(LibraryExportModalField::IncludeSensitiveSettings);
    util::render_modal_key_value_row(
        frame,
        sections[5],
        "Sensitive",
        yes_no_label(state.include_sensitive_settings()),
        sensitive_focused && encrypt,
        !encrypt,
        theme,
    );

    util::render_modal_key_value_row(
        frame,
        sections[6],
        "Stats",
        yes_no_label(state.include_stats()),
        focused(LibraryExportModalField::IncludeStats),
        false,
        theme,
    );

    let feedback_area = sections[7];
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
        .split(sections[8]);
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
        LibraryExportModalField::Password if encrypt => {
            let label_width = sections[3].width.min(12);
            frame.set_cursor_position((
                sections[3].x + label_width + state.password_cursor() as u16,
                sections[3].y,
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
    let popup = centered_rect(width, area.height.clamp(1, 14), area);
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
        "Settings",
        yes_no_label(state.include_settings()),
        state.focus() == LibraryImportModalField::IncludeSettings,
        false,
        theme,
    );
    util::render_modal_key_value_row(
        frame,
        sections[5],
        "Sensitive",
        yes_no_label(state.include_sensitive_settings()),
        state.focus() == LibraryImportModalField::IncludeSensitiveSettings,
        false,
        theme,
    );
    util::render_modal_key_value_row(
        frame,
        sections[6],
        "Stats",
        state.stats_mode_label(),
        state.focus() == LibraryImportModalField::StatsMode,
        false,
        theme,
    );
    util::render_modal_key_value_row(
        frame,
        sections[7],
        "Conflicts",
        state.conflict_mode_label(),
        state.focus() == LibraryImportModalField::ConflictMode,
        false,
        theme,
    );

    frame.render_widget(
        Paragraph::new("").style(
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        sections[8],
    );

    let buttons_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(0),
            Constraint::Length(1),
            Constraint::Length(0),
        ])
        .split(sections[9]);
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
