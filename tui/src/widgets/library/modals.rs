use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::library::icons::CHEVRON_DOWN;
use crate::widgets::library::state::{
    ButtonSelection, LibraryAppFilterState, LibraryCreateModalField, LibraryCreateModalState,
    LibraryDeleteModalState, LibraryExportModalField, LibraryExportModalState,
    LibraryExportResultModalState, LibraryHeaderMenuState, LibraryImportModalField,
    LibraryImportModalState, LibraryImportResultModalState, LibraryImportRunVariablesModalState,
    LibraryModal, LibrarySelectState, LibraryTagsModalState,
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
        LibraryModal::Create(state) => render_library_create_modal(frame, area, theme, state),
        LibraryModal::HeaderMenu(state) => {
            render_library_header_menu_modal(frame, area, theme, state)
        }
        LibraryModal::Tags(state) => render_library_tags_modal(frame, area, theme, state),
        LibraryModal::AppFilter(state) => {
            render_library_app_filter_modal(frame, area, theme, state)
        }
    }
}

/// Popup geometry for library modal dialogs.
pub(crate) fn delete_popup(area: Rect) -> Rect {
    util::overlay_popup_sized(56, DELETE_POPUP_HEIGHT, area)
}

/// Delete popup height: body rows are title, blank, question, name,
/// blank, buttons, error, so nothing sits empty below the buttons.
pub(crate) const DELETE_POPUP_HEIGHT: u16 = 9;

/// Body-row offsets inside the delete confirm, shared by rendering and
/// hit-testing: title, blank, question, trigger name, blank, then the
/// horizontal Yes/No button pair.
pub(crate) const DELETE_QUESTION_OFFSET: u16 = 2;
pub(crate) const DELETE_NAME_OFFSET: u16 = 3;
pub(crate) const DELETE_BUTTONS_OFFSET: u16 = 5;

/// Delete confirm button labels, shared by rendering and hit-testing
/// so the pair never drifts between the two paths.
pub(crate) const DELETE_YES_LABEL: &str = "Yes";
pub(crate) const DELETE_NO_LABEL: &str = "No";

/// Buttons row inside the delete confirm, shared by rendering and
/// hit-testing.
fn delete_buttons_row(area: Rect) -> Rect {
    let popup = delete_popup(area);
    let body = util::overlay_body(popup);
    Rect {
        x: body.x,
        y: body.y.saturating_add(DELETE_BUTTONS_OFFSET),
        width: body.width,
        height: 1,
    }
}

/// Click on the delete confirm at `(column, row)`: `Some(true)` is Yes,
/// `Some(false)` is No. Only the button cells select; title, blanks,
/// question, name, and error cells never do.
pub(crate) fn delete_option_hit(area: Rect, column: u16, row: u16) -> Option<bool> {
    let row_area = delete_buttons_row(area);
    let (yes, no) = util::confirm_button_cells(row_area, DELETE_YES_LABEL, DELETE_NO_LABEL);
    let inside = |cell: Rect| {
        column >= cell.x
            && column < cell.x.saturating_add(cell.width)
            && row == cell.y
            && cell.width > 0
    };
    if inside(yes) {
        Some(true)
    } else if inside(no) {
        Some(false)
    } else {
        None
    }
}

pub(crate) fn export_popup(area: Rect) -> Rect {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(76)
    } else {
        area.width.max(1)
    };
    util::centered_rect(width, area.height.clamp(1, 10), area)
}

pub(crate) fn import_popup(area: Rect) -> Rect {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(76)
    } else {
        area.width.max(1)
    };
    util::centered_rect(width, area.height.clamp(1, 11), area)
}

pub(crate) fn import_run_variables_popup(area: Rect) -> Rect {
    let width = if area.width > 52 {
        area.width.saturating_sub(6).min(72)
    } else {
        area.width.max(1)
    };
    util::centered_rect(width, 9.min(area.height.max(1)), area)
}

pub(crate) fn import_result_popup(area: Rect, state: &LibraryImportResultModalState) -> Rect {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(64)
    } else {
        area.width.max(1)
    };
    util::centered_rect(
        width,
        (state.lines().len() as u16 + 4).min(area.height.max(1)),
        area,
    )
}

pub(crate) fn export_result_popup(area: Rect) -> Rect {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(76)
    } else {
        area.width.max(1)
    };
    util::centered_rect(width, 5.min(area.height.max(1)), area)
}

fn render_library_delete_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryDeleteModalState,
) {
    use ratatui::style::Color::Rgb;

    let popup = delete_popup(area);
    frame.render_widget(Clear, popup);
    // honey: flat borderless fill, the shared overlay language; the
    // cursor band marks the selected option.
    frame.render_widget(
        Block::default().style(Style::default().bg(Rgb(0x14, 0x14, 0x14))),
        popup,
    );
    let body = util::overlay_body(popup);
    if body.width == 0 || body.height == 0 {
        return;
    }
    util::render_overlay_title(frame, body, theme, "Confirm delete");

    let question_y = body.y.saturating_add(DELETE_QUESTION_OFFSET);
    let name_y = body.y.saturating_add(DELETE_NAME_OFFSET);
    let buttons = delete_buttons_row(area);
    // honey: error rides directly under the buttons so a failed
    // confirm stays visible without reserving dead space for it.
    let error_y = buttons.y.saturating_add(1);
    let bottom = body.y.saturating_add(body.height);
    if question_y < bottom {
        frame.render_widget(
            Paragraph::new("Do you want to delete this trigger?")
                .style(Style::default().fg(theme.text_muted)),
            Rect {
                x: body.x,
                y: question_y,
                width: body.width,
                height: 1,
            },
        );
    }
    if name_y < bottom {
        frame.render_widget(
            Paragraph::new(util::truncate_to_width(state.name(), body.width))
                .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
            Rect {
                x: body.x,
                y: name_y,
                width: body.width,
                height: 1,
            },
        );
    }

    // honey: horizontal pair on the shared confirm geometry so a
    // future delete overlay reuses the same cells in rendering and
    // hit-testing.
    if buttons.y < bottom && buttons.width > 0 {
        let (yes, no) = util::confirm_button_cells(buttons, DELETE_YES_LABEL, DELETE_NO_LABEL);
        util::render_confirm_buttons(
            frame,
            yes,
            no,
            DELETE_YES_LABEL,
            DELETE_NO_LABEL,
            state.selected_yes(),
            theme,
        );
    }

    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    };
    if error_y < bottom && error_y != buttons.y {
        frame.render_widget(
            Paragraph::new(state.error().unwrap_or("")).style(feedback_style),
            Rect {
                x: body.x,
                y: error_y,
                width: body.width,
                height: 1,
            },
        );
    }
}

/// Create popup width; the body sheds the overlay margins.
pub(crate) const CREATE_POPUP_WIDTH: u16 = 76;
pub(crate) const CREATE_BODY_WIDTH: u16 = CREATE_POPUP_WIDTH - 6;
/// Label column width for the create form's inline rows.
const CREATE_LABEL_WIDTH: u16 = 12;

/// Create popup height by shape: title, type, trigger, content label
/// plus box, name, action, script rows, os, advanced toggle, advanced
/// rows, buttons, error.
pub(crate) fn create_popup_height(is_script: bool, expanded: bool) -> u16 {
    let mut height = 16u16;
    if is_script {
        height = height.saturating_add(2);
    }
    if expanded {
        height = height.saturating_add(4);
    }
    height
}

pub(crate) fn create_popup(area: Rect, is_script: bool, expanded: bool) -> Rect {
    util::overlay_popup_sized(
        CREATE_POPUP_WIDTH,
        create_popup_height(is_script, expanded),
        area,
    )
}

/// Body-relative rows of the create form, shared by rendering and
/// hit-testing. The content box always spans four rows; the error row
/// rides directly under the buttons.
struct CreateRows {
    field_rows: Vec<(LibraryCreateModalField, u16)>,
    content_first: u16,
    buttons: u16,
}

fn create_rows(is_script: bool, expanded: bool) -> CreateRows {
    use LibraryCreateModalField as Field;
    let content_first = 4u16;
    let mut ordered = vec![
        (Field::TriggerType, 1),
        (Field::Trigger, 2),
        (Field::Name, 8),
        (Field::Action, 9),
    ];
    let mut next = 10u16;
    if is_script {
        ordered.push((Field::Interpreter, next));
        next += 1;
        ordered.push((Field::Behavior, next));
        next += 1;
    }
    ordered.push((Field::Os, next));
    next += 1;
    ordered.push((Field::AdvancedToggle, next));
    next += 1;
    if expanded {
        for field in [Field::AutoCase, Field::Tags, Field::Allow, Field::Block] {
            ordered.push((field, next));
            next += 1;
        }
    }
    let buttons = next;
    CreateRows {
        field_rows: ordered,
        content_first,
        buttons,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateHit {
    Field(LibraryCreateModalField),
    Cancel,
    Create,
}

fn create_buttons_row(area: Rect, is_script: bool, expanded: bool) -> (Rect, Rect, Rect) {
    let popup = create_popup(area, is_script, expanded);
    let body = util::overlay_body(popup);
    let rows = create_rows(is_script, expanded);
    let row = Rect {
        x: body.x,
        y: body.y.saturating_add(rows.buttons),
        width: body.width,
        height: 1,
    };
    let (cancel, create) = util::confirm_button_cells(row, "Cancel", "Create");
    (row, cancel, create)
}

/// Click on the create modal: field rows focus (picker rows also
/// open), the button pair cancels or submits, everything else is
/// dead. Takes the draft since rows depend on its shape.
pub(crate) fn create_option_hit(
    draft: &LibraryCreateModalState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<CreateHit> {
    use super::state::CREATE_CONTENT_ROWS;

    let is_script = !draft.is_text_action();
    let popup = create_popup(area, is_script, draft.advanced_expanded());
    if !crate::terminal::mouse::contains(popup, column, row) {
        return None;
    }
    let body = util::overlay_body(popup);
    if body.width == 0 || body.height == 0 {
        return None;
    }
    if column < body.x || column >= body.x.saturating_add(body.width) {
        return None;
    }
    let rows = create_rows(is_script, draft.advanced_expanded());
    let rel = row.saturating_sub(body.y);
    let (_, cancel, create) = create_buttons_row(area, is_script, draft.advanced_expanded());
    if row == cancel.y && column >= cancel.x && column < cancel.x.saturating_add(cancel.width) {
        return Some(CreateHit::Cancel);
    }
    if row == create.y && column >= create.x && column < create.x.saturating_add(create.width) {
        return Some(CreateHit::Create);
    }
    if rel >= rows.content_first
        && rel
            < rows
                .content_first
                .saturating_add(CREATE_CONTENT_ROWS as u16)
    {
        return Some(CreateHit::Field(LibraryCreateModalField::Content));
    }
    rows.field_rows
        .iter()
        .find(|(_, y)| *y == rel)
        .map(|(field, _)| CreateHit::Field(*field))
}

fn render_create_text_row(
    frame: &mut Frame,
    body: Rect,
    y: u16,
    label: &str,
    field: &crate::widgets::field::TextField,
    focused: bool,
    theme: &Theme,
) {
    let bottom = body.y.saturating_add(body.height);
    if y >= bottom {
        return;
    }
    let area = Rect {
        x: body.x,
        y,
        width: body.width,
        height: 1,
    };
    let label_style = if focused {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };
    frame.render_widget(
        Paragraph::new(label).style(label_style),
        Rect {
            x: area.x,
            y,
            width: CREATE_LABEL_WIDTH.min(area.width),
            height: 1,
        },
    );
    let input_x = area.x.saturating_add(CREATE_LABEL_WIDTH).saturating_add(1);
    let input_width = area
        .width
        .saturating_sub(CREATE_LABEL_WIDTH)
        .saturating_sub(1);
    if input_width == 0 {
        return;
    }
    let bg = if focused {
        theme.surface
    } else {
        theme.background
    };
    frame.render_widget(
        Block::default().style(Style::default().bg(bg)),
        Rect {
            x: input_x,
            y,
            width: input_width,
            height: 1,
        },
    );
    let (visible, caret) = field.window(input_width);
    frame.render_widget(
        Paragraph::new(visible.to_string()).style(Style::default().fg(theme.text).bg(bg)),
        Rect {
            x: input_x,
            y,
            width: input_width,
            height: 1,
        },
    );
    if focused {
        let (cx, cy) = util::caret_position(input_x, y, caret, input_width);
        frame.set_cursor_position((cx, cy));
    }
}

fn render_library_create_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryCreateModalState,
) {
    use ratatui::style::Color::Rgb;

    use super::state::CREATE_CONTENT_ROWS;

    let is_script = !state.is_text_action();
    let expanded = state.advanced_expanded();
    let popup = create_popup(area, is_script, expanded);
    frame.render_widget(Clear, popup);
    // honey: flat borderless fill, the shared overlay language.
    frame.render_widget(
        Block::default().style(Style::default().bg(Rgb(0x14, 0x14, 0x14))),
        popup,
    );
    let body = util::overlay_body(popup);
    if body.width == 0 || body.height == 0 {
        return;
    }
    util::render_overlay_title(frame, body, theme, "New Trigger");
    let rows = create_rows(is_script, expanded);
    let bottom = body.y.saturating_add(body.height);
    let focused = state.focus();

    for (field, y) in &rows.field_rows {
        let y = body.y.saturating_add(*y);
        if y >= bottom {
            continue;
        }
        let is_focused = focused == *field;
        match field {
            LibraryCreateModalField::TriggerType => {
                util::render_modal_key_value_row(
                    frame,
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                    "Type",
                    &format!("{} {CHEVRON_DOWN}", state.type_label()),
                    is_focused,
                    false,
                    theme,
                );
            }
            LibraryCreateModalField::Trigger => {
                render_create_text_row(
                    frame,
                    body,
                    y,
                    "Trigger",
                    state.trigger_field(),
                    is_focused,
                    theme,
                );
            }
            LibraryCreateModalField::Name => {
                render_create_text_row(
                    frame,
                    body,
                    y,
                    "Name",
                    state.name_field(),
                    is_focused,
                    theme,
                );
            }
            LibraryCreateModalField::Action => {
                util::render_modal_key_value_row(
                    frame,
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                    "Action",
                    state.action_label(),
                    is_focused,
                    false,
                    theme,
                );
            }
            LibraryCreateModalField::Interpreter => {
                util::render_modal_key_value_row(
                    frame,
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                    "Language",
                    &format!("{} {CHEVRON_DOWN}", state.interpreter_label()),
                    is_focused,
                    false,
                    theme,
                );
            }
            LibraryCreateModalField::Behavior => {
                util::render_modal_key_value_row(
                    frame,
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                    "Behavior",
                    &format!("{} {CHEVRON_DOWN}", state.behavior_label()),
                    is_focused,
                    false,
                    theme,
                );
            }
            LibraryCreateModalField::Os => {
                util::render_modal_key_value_row(
                    frame,
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                    "OS",
                    &format!("{} {CHEVRON_DOWN}", state.target_os()),
                    is_focused,
                    false,
                    theme,
                );
            }
            LibraryCreateModalField::AdvancedToggle => {
                let chevron = if expanded {
                    super::icons::CHEVRON_UP
                } else {
                    CHEVRON_DOWN
                };
                let style = if is_focused {
                    Style::default()
                        .fg(theme.text)
                        .bg(theme.surface)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text_muted)
                };
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(format!("{chevron} "), style),
                        Span::styled("Advanced Options", style),
                    ])),
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                );
            }
            LibraryCreateModalField::AutoCase => {
                util::render_modal_key_value_row(
                    frame,
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                    "Auto case",
                    if state.auto_case() { "on" } else { "off" },
                    is_focused,
                    false,
                    theme,
                );
            }
            LibraryCreateModalField::Tags => {
                let summary = if state.tags().is_empty() {
                    super::detail::EMPTY_TOKEN.to_string()
                } else {
                    state.tags().join(", ")
                };
                util::render_modal_key_value_row(
                    frame,
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                    "Tags",
                    &format!("{summary} {CHEVRON_DOWN}"),
                    is_focused,
                    false,
                    theme,
                );
            }
            LibraryCreateModalField::Allow => {
                let summary = if state.allow_apps().is_empty() {
                    super::detail::EMPTY_TOKEN.to_string()
                } else {
                    state.allow_apps().join(", ")
                };
                util::render_modal_key_value_row(
                    frame,
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                    "Allow on",
                    &format!("{summary} {CHEVRON_DOWN}"),
                    is_focused,
                    false,
                    theme,
                );
            }
            LibraryCreateModalField::Block => {
                let summary = if state.block_apps().is_empty() {
                    super::detail::EMPTY_TOKEN.to_string()
                } else {
                    state.block_apps().join(", ")
                };
                util::render_modal_key_value_row(
                    frame,
                    Rect {
                        x: body.x,
                        y,
                        width: body.width,
                        height: 1,
                    },
                    "Block on",
                    &format!("{summary} {CHEVRON_DOWN}"),
                    is_focused,
                    false,
                    theme,
                );
            }
            LibraryCreateModalField::Content | LibraryCreateModalField::ActionButton => {}
        }
    }

    // honey: content label plus a four-row viewport that follows the
    // caret; long lines truncate in place like the detail preview.
    let label_y = body.y.saturating_add(3);
    if label_y < bottom {
        let style = if focused == LibraryCreateModalField::Content {
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_muted)
        };
        frame.render_widget(
            Paragraph::new("Content").style(style),
            Rect {
                x: body.x,
                y: label_y,
                width: body.width,
                height: 1,
            },
        );
    }
    let (caret_row, caret_col) = state.content().cursor();
    let lines = state.content().lines();
    for index in 0..CREATE_CONTENT_ROWS {
        let y = body
            .y
            .saturating_add(rows.content_first)
            .saturating_add(index as u16);
        if y >= bottom {
            break;
        }
        let line = lines
            .get(state.content_scroll().saturating_add(index))
            .map(String::as_str)
            .unwrap_or("");
        let visible: String = line.chars().take(body.width as usize).collect();
        let is_caret_row = focused == LibraryCreateModalField::Content
            && caret_row == state.content_scroll().saturating_add(index);
        let style = if is_caret_row {
            Style::default().fg(theme.text).bg(theme.surface)
        } else {
            Style::default().fg(theme.text)
        };
        frame.render_widget(
            Paragraph::new(visible).style(style),
            Rect {
                x: body.x,
                y,
                width: body.width,
                height: 1,
            },
        );
        if is_caret_row {
            let (cx, cy) = util::caret_position(
                body.x,
                y,
                caret_col.min(body.width.saturating_sub(1) as usize),
                body.width,
            );
            frame.set_cursor_position((cx, cy));
        }
    }

    let (_, cancel, create) = create_buttons_row(area, is_script, expanded);
    if cancel.y < bottom {
        util::render_confirm_buttons(
            frame,
            cancel,
            create,
            "Cancel",
            "Create",
            state.button_selection() == ButtonSelection::Cancel,
            theme,
        );
    }

    let error_y = cancel.y.saturating_add(1);
    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    };
    if error_y < bottom {
        frame.render_widget(
            Paragraph::new(state.error().unwrap_or("")).style(feedback_style),
            Rect {
                x: body.x,
                y: error_y,
                width: body.width,
                height: 1,
            },
        );
    }
}

fn render_library_export_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryExportModalState,
) {
    let popup = export_popup(area);
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
    let popup = import_popup(area);
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
    let popup = import_run_variables_popup(area);
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
    let popup = import_result_popup(area, state);
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
    let popup = export_result_popup(area);
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
            icon: String::new(),
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
    let icons = state.icons();
    let entries: Vec<util::OverlayEntry> = state
        .options()
        .iter()
        .zip(state.details())
        .zip(icons.iter())
        .map(|((option, detail), icon)| util::OverlayEntry {
            label: option.clone(),
            detail: detail.clone(),
            icon: icon.clone(),
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

/// Tags builder overlay on the shared popup geometry: the
/// trigger's tags as a wrapping cloud of colored chips with a `+`
/// button trailing it; the `+` swaps for an inline `#` input while
/// typing. Empty triggers show `No tags available. +`.
fn render_library_tags_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryTagsModalState,
) {
    use ratatui::style::Color::Rgb;
    use ratatui::widgets::Block;

    let popup = util::overlay_popup(area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(Rgb(0x14, 0x14, 0x14))),
        popup,
    );
    let body = util::overlay_body(popup);
    if body.width == 0 || body.height == 0 {
        return;
    }
    render_tags_chips(frame, body, theme, state);
    // honey: no hint footer; errors alone take the bottom row so a
    // failed write stays visible inside the menu.
    if let Some(error) = state.error() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                util::truncate_to_width(error, body.width),
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ))),
            Rect {
                x: body.x,
                y: body.y.saturating_add(body.height).saturating_sub(1),
                width: body.width,
                height: 1,
            },
        );
    }
}

/// Chips cloud: title row, one blank line, then wrapped `#tag`
/// chips in the Properties pane colors (text only, no
/// background), the `+` button trailing the cloud (or the live
/// `#` input while typing). Only the hovered chip swaps its `#`
/// for the close icon, in the same cell; keyboard focus
/// underlines instead, keeping the color.
fn render_tags_chips(frame: &mut Frame, body: Rect, theme: &Theme, state: &LibraryTagsModalState) {
    use crate::widgets::library::state::{TAGS_EMPTY_LINE, tag_style};

    util::render_overlay_title(frame, body, theme, "Tags");
    // honey: one blank line separates the title from the cloud.
    let max_rows = body.height.saturating_sub(3);
    let origin_y = body.y.saturating_add(2);
    if state.is_empty() && !state.input_active() {
        let empty = format!("{TAGS_EMPTY_LINE} ");
        let plus_focused = state.focus_is_add_row();
        let plus_style = if plus_focused {
            Style::default()
                .fg(theme.text)
                .bg(theme.surface)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_muted)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(empty, Style::default().fg(theme.text_muted)),
                Span::styled(" + ".to_string(), plus_style),
            ])),
            Rect {
                x: body.x,
                y: origin_y,
                width: body.width,
                height: 1,
            },
        );
        return;
    }
    let (painted, add) = state.paint_layout(body.width, max_rows);
    // honey: bucket painted chips by relative row for one Paragraph
    // per row; every chip keeps its Properties pane text color.
    let mut rows: Vec<Vec<(u16, String, Style)>> = Vec::new();
    for (cell, rel_y) in &painted {
        while rows.len() <= *rel_y as usize {
            rows.push(Vec::new());
        }
        let tag = state
            .checked()
            .get(cell.index)
            .map(String::as_str)
            .unwrap_or_default();
        // honey: only the hovered chip swaps its `#` for the close
        // icon, in the same cell so nothing shifts.
        let hovered = state.hover() == Some(cell.index);
        let raw = if hovered {
            format!("{}{tag}", crate::widgets::library::icons::CLOSE_ICON)
        } else {
            format!("#{tag}")
        };
        let avail = body.width.saturating_sub(cell.x).max(1);
        let shown = util::truncate_to_width(&raw, avail);
        rows[*rel_y as usize].push((cell.x, shown, tag_style(theme, cell.index)));
    }
    for (offset, chips) in rows.iter().enumerate() {
        let mut spans = Vec::new();
        let mut cursor = 0u16;
        for (x, text, style) in chips {
            if *x > cursor {
                spans.push(Span::raw(" ".repeat(x.saturating_sub(cursor) as usize)));
                cursor = *x;
            }
            spans.push(Span::styled(text.clone(), *style));
            cursor = cursor.saturating_add(text.chars().count() as u16);
        }
        let rest = (body.width as usize).saturating_sub(cursor as usize);
        spans.push(Span::raw(" ".repeat(rest)));
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect {
                x: body.x,
                y: origin_y.saturating_add(offset as u16),
                width: body.width,
                height: 1,
            },
        );
    }
    if let Some(add_rel) = add {
        let (plus_x, _) = state.plus_cell(body.width);
        let row_y = origin_y.saturating_add(add_rel);
        if state.input_active() {
            // honey: the live `#` input replaces the `+` in place;
            // the caret sits right after the `#`.
            let avail = body.width.saturating_sub(plus_x).saturating_sub(1).max(1);
            let (visible, caret) = state.input().window(avail);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("#".to_string(), Style::default().fg(theme.text)),
                    Span::styled(visible.to_string(), Style::default().fg(theme.text)),
                ])),
                Rect {
                    x: body.x.saturating_add(plus_x),
                    y: row_y,
                    width: body.width.saturating_sub(plus_x),
                    height: 1,
                },
            );
            if !crate::overlay_fx::is_cursor_suppressed() {
                let (caret_x, caret_y) = util::caret_position(
                    body.x.saturating_add(plus_x).saturating_add(1),
                    row_y,
                    caret,
                    avail,
                );
                frame.set_cursor_position((caret_x, caret_y));
            }
        } else {
            let focused = state.focus_is_add_row();
            let style = if focused {
                Style::default()
                    .fg(theme.text)
                    .bg(theme.surface)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text_muted)
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("+".to_string(), style))),
                Rect {
                    x: body.x.saturating_add(plus_x),
                    y: row_y,
                    width: 1,
                    height: 1,
                },
            );
        }
    }
}

/// App-filter picker on its own taller popup: search results left
/// (stored filters, up to eight foreground apps as two-line `exe`
/// plus window-title rows), empty right pane behind a divider, search
/// box pinned to the absolute bottom at the left pane width.
/// Opposite-list rows render dimmed with their reason and never take
/// focus. Validation errors ride the title row; there is no footer.
fn render_library_app_filter_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryAppFilterState,
) {
    use ratatui::style::Color::Rgb;
    use ratatui::widgets::Block;

    use crate::widgets::library::state::{
        APP_FILTER_PAD, APP_FILTER_ROWS_TOP, FilterRow, app_filter_geometry,
    };

    let (popup, body, max_lines) = app_filter_geometry(area, state);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(Rgb(0x14, 0x14, 0x14))),
        popup,
    );
    if body.width == 0 || body.height < 5 {
        return;
    }
    // honey: title row carries the validation error when one exists;
    // the bottom row belongs to the search box, so no footer.
    let title = state.side().title();
    let hint = "Esc";
    let error = state.error().unwrap_or("");
    let err_width = (error.chars().count() as u16).min(body.width);
    let title_w = body.width.saturating_sub(2);
    let gap = title_w
        .saturating_sub(title.chars().count() as u16)
        .saturating_sub(err_width)
        .saturating_sub(hint.chars().count() as u16);
    let mut title_spans = vec![Span::styled(
        title.to_string(),
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    )];
    if !error.is_empty() {
        title_spans.push(Span::raw(" ".repeat(gap as usize)));
        title_spans.push(Span::styled(
            util::truncate_to_width(error, err_width.max(1)),
            Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        title_spans.push(Span::raw(" ".repeat(gap as usize)));
    }
    title_spans.push(Span::styled(
        hint.to_string(),
        Style::default().fg(theme.text_muted),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(title_spans)),
        Rect {
            x: body.x.saturating_add(1),
            y: body.y,
            width: title_w,
            height: 1,
        },
    );
    let origin_y = body.y.saturating_add(APP_FILTER_ROWS_TOP);
    let (left_w, divider_x, _, _) = state.panes(body.width);
    // honey: content sits one padded cell in with one blank cell left
    // before the divider; the divider runs the full popup edge to edge.
    let content_x = body.x.saturating_add(APP_FILTER_PAD);
    let content_w = left_w.saturating_sub(APP_FILTER_PAD.saturating_mul(2));
    let divider_abs_x = body.x.saturating_add(divider_x);
    for y in popup.y..popup.y.saturating_add(popup.height) {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "│".to_string(),
                Style::default().fg(theme.border),
            ))),
            Rect {
                x: divider_abs_x,
                y,
                width: 1,
                height: 1,
            },
        );
    }
    let focused = state.rows().get(state.cursor()).copied();
    if state.checked().is_empty() && state.matching_indices().is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No foreground apps.".to_string(),
                Style::default().fg(theme.text_muted),
            ))),
            Rect {
                x: body.x,
                y: origin_y,
                width: left_w,
                height: 1,
            },
        );
    }
    // honey: full-bleed highlight band for the focused row, no fg
    // override — the overlay option-menu language. Bands pad to the
    // content width so both lines fill the pane edge to padding.
    let band = || {
        Style::default()
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    };
    let fill = |text: String, style: Style| -> Line {
        let rest = content_w.saturating_sub(text.chars().count() as u16) as usize;
        Line::from(vec![
            Span::styled(text, style),
            Span::styled(" ".repeat(rest), style),
        ])
    };
    for (row, rel) in state.layout(max_lines) {
        let row_y = origin_y.saturating_add(rel);
        let is_focused = Some(row) == focused;
        match row {
            FilterRow::Checked(index) => {
                let value = state.checked().get(index).map(String::as_str).unwrap_or("");
                let text = util::truncate_to_width(&format!("× {value}"), content_w.max(1));
                let style = if is_focused {
                    band().fg(theme.text)
                } else {
                    Style::default().fg(theme.text)
                };
                frame.render_widget(
                    Paragraph::new(fill(text, style)),
                    Rect {
                        x: content_x,
                        y: row_y,
                        width: content_w,
                        height: 1,
                    },
                );
            }
            FilterRow::Foreground(index) => {
                let Some(app) = state.foreground().get(index) else {
                    continue;
                };
                let gray = state.is_gray(index);
                let exe = util::truncate_to_width(&app.exe, content_w.max(1));
                let exe_style = if gray {
                    Style::default().fg(theme.text_muted)
                } else if is_focused {
                    band().fg(theme.text)
                } else {
                    Style::default().fg(theme.text)
                };
                frame.render_widget(
                    Paragraph::new(fill(exe, exe_style)),
                    Rect {
                        x: content_x,
                        y: row_y,
                        width: content_w,
                        height: 1,
                    },
                );
                let detail = if gray {
                    format!("{} · {}", app.title, state.side().opposite_hint())
                } else {
                    app.title.clone()
                };
                let detail = util::truncate_to_width(&detail, content_w.max(1));
                // honey: the focused row bands across both lines; the
                // description keeps its dimmed tone on the band.
                let detail_style = if is_focused && !gray {
                    band().fg(theme.text_muted)
                } else {
                    Style::default().fg(theme.text_muted)
                };
                frame.render_widget(
                    Paragraph::new(fill(detail, detail_style)),
                    Rect {
                        x: content_x,
                        y: row_y.saturating_add(1),
                        width: content_w,
                        height: 1,
                    },
                );
            }
        }
    }
    // honey: the right pane stays empty for now; the divider still
    // splits the overlay with padding on both sides.
    // honey: the search box sits exactly one blank line below the
    // visible rows at the left pane width; typing always filters.
    // The caret always shows.
    let search_y = origin_y.saturating_add(state.search_rel(max_lines));
    let search_w = content_w.max(1);
    let (visible, caret) = state.search().window(search_w);
    let (text, style) = if state.search().is_empty() {
        (
            "Search apps".to_string(),
            Style::default().fg(theme.text_muted),
        )
    } else {
        (visible.to_string(), Style::default().fg(theme.text))
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, style)))
            .style(Style::default().bg(theme.surface)),
        Rect {
            x: content_x,
            y: search_y,
            width: search_w,
            height: 1,
        },
    );
    if !crate::overlay_fx::is_cursor_suppressed() {
        let (caret_x, caret_y) = util::caret_position(content_x, search_y, caret, search_w);
        frame.set_cursor_position((caret_x, caret_y));
    }
}
