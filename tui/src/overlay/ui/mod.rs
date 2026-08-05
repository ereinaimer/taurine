mod actions;
mod rows;

use crate::theme::builtin::DARK_THEME;
use crate::widgets::library::{
    ButtonSelection, LibraryExportModalField, LibraryExportModalState, LibraryImportModalField,
    LibraryImportModalState,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use taurine_core::exchange::{ExistingTriggerConflict, TriggerExport};

use self::actions::{fill_bg, render_action_buttons_overlay};
use self::rows::{
    desc_area, padded, render_desc, render_path_row, render_select_list, row_input, row_key_value,
    row_password,
};
pub(crate) fn render_export_popup(frame: &mut Frame, state: &LibraryExportModalState) {
    fill_bg(frame);
    let inner = padded(frame.area());

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    let title_style = Style::default()
        .fg(DARK_THEME.text)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(" Export Trigger").style(title_style),
        sections[0],
    );

    let (path_area, path_desc) = desc_area(sections[2]);
    let path_focused = state.focus() == LibraryExportModalField::Path;
    let path_cursor = render_path_row(
        frame,
        path_area,
        state.path(),
        state.path_cursor(),
        path_focused,
    );
    render_desc(frame, path_desc, "file path for the export", path_focused);

    let (enc_area, enc_desc) = desc_area(sections[3]);
    let encrypt_focused = state.focus() == LibraryExportModalField::Encrypt;
    let encrypt_label = if state.encrypt() { "yes" } else { "no" };
    row_key_value(frame, enc_area, " Encrypt", encrypt_label, encrypt_focused);
    render_desc(
        frame,
        enc_desc,
        "password to encrypt the export",
        encrypt_focused,
    );

    let encrypt = state.encrypt();
    let (pw_area, pw_desc) = desc_area(sections[4]);
    let password_focused = state.focus() == LibraryExportModalField::Password;
    row_password(
        frame,
        pw_area,
        " Password",
        state.password(),
        password_focused && encrypt,
        !encrypt,
        false,
    );
    render_desc(
        frame,
        pw_desc,
        if encrypt {
            "password used for encryption"
        } else {
            "encryption disabled"
        },
        password_focused && encrypt,
    );

    let (set_area, set_desc) = desc_area(sections[5]);
    let settings_focused = state.focus() == LibraryExportModalField::IncludeSettings;
    let settings_label = if state.include_settings() {
        "yes"
    } else {
        "no"
    };
    row_key_value(
        frame,
        set_area,
        " Settings",
        settings_label,
        settings_focused,
    );
    render_desc(frame, set_desc, "include preferences", settings_focused);

    let (sen_area, sen_desc) = desc_area(sections[6]);
    let sensitive_focused = state.focus() == LibraryExportModalField::IncludeSensitiveSettings;
    let sensitive_label = if state.include_sensitive_settings() {
        "yes"
    } else {
        "no"
    };
    row_key_value(
        frame,
        sen_area,
        " Sensitive",
        sensitive_label,
        sensitive_focused && encrypt,
    );
    render_desc(
        frame,
        sen_desc,
        if encrypt {
            "include sensitive data (API keys)"
        } else {
            "encryption disabled"
        },
        sensitive_focused && encrypt,
    );

    let (stat_area, stat_desc) = desc_area(sections[7]);
    let stats_focused = state.focus() == LibraryExportModalField::IncludeStats;
    let stats_label = if state.include_stats() { "yes" } else { "no" };
    row_key_value(frame, stat_area, " Stats", stats_label, stats_focused);
    render_desc(frame, stat_desc, "include usage history", stats_focused);

    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(DARK_THEME.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(DARK_THEME.text_muted)
            .add_modifier(Modifier::DIM)
    };
    let feedback_text = state.error().unwrap_or("");
    frame.render_widget(
        Paragraph::new(feedback_text).style(feedback_style),
        sections[8],
    );

    render_action_buttons_overlay(
        frame,
        sections[9],
        "Cancel",
        "Export",
        state.focus() == LibraryExportModalField::ActionButton,
        state.button_selection(),
    );

    match state.focus() {
        LibraryExportModalField::Path => {
            if let Some((cx, cy)) = path_cursor {
                frame.set_cursor_position((cx, cy));
            }
        }
        LibraryExportModalField::Password if state.encrypt() => {
            let (pw_area, _) = desc_area(sections[4]);
            let val_x = pw_area.x
                + pw_area
                    .width
                    .saturating_sub(state.password().len() as u16)
                    .saturating_sub(2);
            frame.set_cursor_position((val_x + 1 + state.password_cursor() as u16, pw_area.y));
        }
        _ => {}
    }
}

pub(crate) fn render_import_popup(frame: &mut Frame, state: &LibraryImportModalState) {
    fill_bg(frame);
    if let Some(selector) = state.selector() {
        render_select_list(frame, selector);
        return;
    }

    let inner = padded(frame.area());

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    let title_style = Style::default()
        .fg(DARK_THEME.text)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(" Import Triggers").style(title_style),
        sections[0],
    );

    let (path_area, path_desc) = desc_area(sections[2]);
    let path_focused = state.focus() == LibraryImportModalField::Path;
    let path_cursor = render_path_row(
        frame,
        path_area,
        state.path(),
        state.path_cursor(),
        path_focused,
    );
    render_desc(frame, path_desc, "file path to import from", path_focused);

    let (pw_area, pw_desc) = desc_area(sections[3]);
    let password_focused = state.focus() == LibraryImportModalField::Password;
    let password_disabled = state.is_encrypted() == Some(false);
    let show_red_asterisk = state.is_encrypted() == Some(true)
        && state.password().is_empty()
        && state.error().is_some();
    row_password(
        frame,
        pw_area,
        " Password",
        state.password(),
        password_focused && !password_disabled,
        password_disabled,
        show_red_asterisk,
    );
    render_desc(
        frame,
        pw_desc,
        if password_disabled {
            "file is not encrypted"
        } else {
            "password to decrypt the file"
        },
        password_focused && !password_disabled,
    );

    let (set_area, set_desc) = desc_area(sections[4]);
    let settings_focused = state.focus() == LibraryImportModalField::IncludeSettings;
    let settings_label = if state.include_settings() {
        "yes"
    } else {
        "no"
    };
    row_key_value(
        frame,
        set_area,
        " Settings",
        settings_label,
        settings_focused,
    );
    render_desc(frame, set_desc, "restore preferences", settings_focused);

    let (sen_area, sen_desc) = desc_area(sections[5]);
    let sensitive_focused = state.focus() == LibraryImportModalField::IncludeSensitiveSettings;
    let sensitive_label = if state.include_sensitive_settings() {
        "yes"
    } else {
        "no"
    };
    row_key_value(
        frame,
        sen_area,
        " Sensitive",
        sensitive_label,
        sensitive_focused,
    );
    render_desc(
        frame,
        sen_desc,
        "restore sensitive data (API keys)",
        sensitive_focused,
    );

    let (stat_area, stat_desc) = desc_area(sections[6]);
    let stats_focused = state.focus() == LibraryImportModalField::StatsMode;
    row_key_value(
        frame,
        stat_area,
        " Stats",
        state.stats_mode_label(),
        stats_focused,
    );
    render_desc(
        frame,
        stat_desc,
        "how usage data should be imported",
        stats_focused,
    );

    let (conf_area, conf_desc) = desc_area(sections[7]);
    let conflict_focused = state.focus() == LibraryImportModalField::ConflictMode;
    row_key_value(
        frame,
        conf_area,
        " on-conflict strategy",
        state.conflict_mode_label(),
        conflict_focused,
    );
    render_desc(
        frame,
        conf_desc,
        "action when a conflict happens",
        conflict_focused,
    );

    frame.render_widget(
        Paragraph::new("").style(
            Style::default()
                .fg(DARK_THEME.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        sections[8],
    );

    render_action_buttons_overlay(
        frame,
        sections[10],
        "Cancel",
        "Import",
        state.focus() == LibraryImportModalField::ActionButton,
        state.button_selection(),
    );

    match state.focus() {
        LibraryImportModalField::Path => {
            if let Some((cx, cy)) = path_cursor {
                frame.set_cursor_position((cx, cy));
            }
        }
        LibraryImportModalField::Password if state.is_encrypted() != Some(false) => {
            let (pw_area, _) = desc_area(sections[3]);
            let val_x = pw_area.x
                + pw_area
                    .width
                    .saturating_sub(state.password().len() as u16)
                    .saturating_sub(2);
            frame.set_cursor_position((val_x + 1 + state.password_cursor() as u16, pw_area.y));
        }
        _ => {}
    }
}

pub(crate) fn render_password_popup(
    frame: &mut Frame,
    label: &str,
    with_confirmation: bool,
    password: &str,
    confirm: &str,
    focus: usize,
    error: Option<&str>,
) {
    fill_bg(frame);
    let inner = padded(frame.area());

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

    let title_style = Style::default()
        .fg(DARK_THEME.text)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(Paragraph::new(label).style(title_style), sections[0]);

    let pw_label_style = Style::default().fg(DARK_THEME.text_muted);
    frame.render_widget(
        Paragraph::new(" Password").style(pw_label_style),
        sections[2],
    );

    let pw_focused = focus == 0;
    row_input(
        frame,
        sections[3],
        password,
        password.chars().count(),
        pw_focused,
    );

    if with_confirmation {
        let confirm_label_style = Style::default().fg(DARK_THEME.text_muted);
        frame.render_widget(
            Paragraph::new(" Confirm").style(confirm_label_style),
            sections[4],
        );

        let confirm_focused = focus == 1;
        row_input(
            frame,
            sections[5],
            confirm,
            confirm.chars().count(),
            confirm_focused,
        );
    }

    let footer_area = if with_confirmation {
        sections[6]
    } else {
        sections[4]
    };
    let (text, style) = if let Some(err) = error {
        (
            err.to_string(),
            Style::default()
                .fg(DARK_THEME.error)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            "Enter Next Field   Tab Switch   Ctrl+S Confirm   Esc Cancel".to_string(),
            Style::default()
                .fg(DARK_THEME.text_muted)
                .add_modifier(Modifier::DIM),
        )
    };
    frame.render_widget(Paragraph::new(text).style(style), footer_area);

    if focus == 0 {
        frame.set_cursor_position((
            sections[3].x + 1 + password.chars().count() as u16,
            sections[3].y,
        ));
    } else if with_confirmation && focus == 1 {
        frame.set_cursor_position((
            sections[5].x + 1 + confirm.chars().count() as u16,
            sections[5].y,
        ));
    }
}

pub(crate) fn export_field_at(
    col: u16,
    row: u16,
    term_width: u16,
    term_height: u16,
) -> Option<(LibraryExportModalField, Option<ButtonSelection>)> {
    let inner_x = 1u16;
    let inner_y = 1u16;
    let inner_w = term_width.saturating_sub(2);
    if col < inner_x || col >= inner_x + inner_w {
        return None;
    }
    if row < inner_y || row >= inner_y + term_height.saturating_sub(2) {
        return None;
    }
    let inner_row = row - inner_y;
    let section = match inner_row {
        2..=3 => 2,
        4..=5 => 3,
        6..=7 => 4,
        8..=9 => 5,
        10..=11 => 6,
        12..=13 => 7,
        15..=15 => 9,
        _ => return None,
    };
    let field = match section {
        2 => LibraryExportModalField::Path,
        3 => LibraryExportModalField::Encrypt,
        4 => LibraryExportModalField::Password,
        5 => LibraryExportModalField::IncludeSettings,
        6 => LibraryExportModalField::IncludeSensitiveSettings,
        7 => LibraryExportModalField::IncludeStats,
        9 => LibraryExportModalField::ActionButton,
        _ => return None,
    };
    let button = if section == 9 {
        let center = inner_x + inner_w / 2;
        Some(if col < center {
            ButtonSelection::Cancel
        } else {
            ButtonSelection::Confirm
        })
    } else {
        None
    };
    Some((field, button))
}

pub(crate) fn import_field_at(
    col: u16,
    row: u16,
    term_width: u16,
    term_height: u16,
) -> Option<(LibraryImportModalField, Option<ButtonSelection>)> {
    let inner_x = 1u16;
    let inner_y = 1u16;
    let inner_w = term_width.saturating_sub(2);
    if col < inner_x || col >= inner_x + inner_w {
        return None;
    }
    if row < inner_y || row >= inner_y + term_height.saturating_sub(2) {
        return None;
    }
    let inner_row = row - inner_y;
    let section = match inner_row {
        2..=3 => 2,
        4..=5 => 3,
        6..=7 => 4,
        8..=9 => 5,
        10..=11 => 6,
        12..=13 => 7,
        16..=16 => 10,
        _ => return None,
    };
    let field = match section {
        2 => LibraryImportModalField::Path,
        3 => LibraryImportModalField::Password,
        4 => LibraryImportModalField::IncludeSettings,
        5 => LibraryImportModalField::IncludeSensitiveSettings,
        6 => LibraryImportModalField::StatsMode,
        7 => LibraryImportModalField::ConflictMode,
        10 => LibraryImportModalField::ActionButton,
        _ => return None,
    };
    let button = if section == 10 {
        let center = inner_x + inner_w / 2;
        Some(if col < center {
            ButtonSelection::Cancel
        } else {
            ButtonSelection::Confirm
        })
    } else {
        None
    };
    Some((field, button))
}

pub(crate) fn render_conflict_popup(
    frame: &mut Frame,
    incoming: &TriggerExport,
    existing: &ExistingTriggerConflict,
    selected: usize,
) {
    fill_bg(frame);
    let inner = padded(frame.area());

    let title_style = Style::default()
        .fg(DARK_THEME.error)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new("Conflict Detected").style(title_style),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    let sub_style = Style::default().fg(DARK_THEME.text_muted);
    frame.render_widget(
        Paragraph::new(format!("  \"{}\" already exists.", incoming.name)).style(sub_style),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    let header_style = Style::default()
        .fg(DARK_THEME.text)
        .add_modifier(Modifier::BOLD);
    let val_style = Style::default().fg(DARK_THEME.text_muted);

    let incoming_start = inner.y + 3;
    let incoming_lines = vec![
        Line::from(vec![Span::styled("Incoming:", header_style)]),
        Line::from(vec![
            Span::styled("  Trigger: ", Style::default().fg(DARK_THEME.text)),
            Span::styled(&incoming.trigger, val_style),
        ]),
        Line::from(vec![
            Span::styled("  Output: ", Style::default().fg(DARK_THEME.text)),
            Span::styled(&incoming.output, val_style),
        ]),
    ];

    let existing_lines = vec![
        Line::from(vec![Span::styled("Existing:", header_style)]),
        Line::from(vec![
            Span::styled("  Trigger: ", Style::default().fg(DARK_THEME.text)),
            Span::styled(&existing.trigger, val_style),
        ]),
        Line::from(vec![
            Span::styled("  Output: ", Style::default().fg(DARK_THEME.text)),
            Span::styled(&existing.output, val_style),
        ]),
    ];

    let max_height = inner.height as usize;
    let all_lines: Vec<Line> = incoming_lines.into_iter().chain(existing_lines).collect();

    frame.render_widget(
        Paragraph::new(all_lines).wrap(Wrap { trim: false }),
        Rect {
            x: inner.x,
            y: incoming_start,
            width: inner.width,
            height: (max_height.saturating_sub(6)).max(1) as u16,
        },
    );

    let options = ["Overwrite", "Skip", "Overwrite All", "Skip All"];
    let options_start = inner.y + inner.height - options.len() as u16 - 2;

    for (i, opt) in options.iter().enumerate() {
        let y = options_start + i as u16;
        let opt_style = if i == selected {
            Style::default()
                .fg(DARK_THEME.text)
                .bg(DARK_THEME.surface)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DARK_THEME.text)
        };
        frame.render_widget(
            Paragraph::new(format!("  {opt}")).style(opt_style),
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            },
        );
    }
}
