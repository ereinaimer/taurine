use crate::library::{
    ButtonSelection, LibraryExportModalField, LibraryExportModalState, LibraryImportModalField,
    LibraryImportModalState, LibrarySelectState,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};
use taurine_core::exchange::{AutomationExport, ExistingAutomationConflict};

const ACCENT: Color = Color::White;
const MUTED: Color = Color::Gray;
const SELECTED_BG: Color = Color::DarkGray;

fn padded(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

fn row_input(frame: &mut Frame, area: Rect, value: &str, cursor: usize, focused: bool) {
    let bg = if focused { SELECTED_BG } else { Color::Reset };
    let text_style = if focused {
        Style::default()
            .fg(ACCENT)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT).bg(bg)
    };

    let block = Block::default().style(Style::default().bg(bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = if focused {
        let total_chars = value.chars().count();
        let safe_cursor = cursor.min(total_chars);
        let chars: Vec<char> = value.chars().collect();
        let before: String = chars.iter().take(safe_cursor).collect();

        if safe_cursor == total_chars {
            Paragraph::new(Line::from(vec![Span::raw(before)]))
        } else {
            let at_cursor = chars[safe_cursor];
            let after: String = chars.iter().skip(safe_cursor + 1).collect();
            Paragraph::new(Line::from(vec![
                Span::raw(before),
                Span::styled(
                    at_cursor.to_string(),
                    Style::default()
                        .fg(ACCENT)
                        .bg(SELECTED_BG)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(after),
            ]))
        }
    } else {
        Paragraph::new(value.to_string())
    };
    frame.render_widget(text.style(text_style), inner);
}

fn row_key_value(frame: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let bg = if focused { SELECTED_BG } else { Color::Reset };
    let text_style = if focused {
        Style::default()
            .fg(ACCENT)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT)
    };
    let value_style = if focused {
        Style::default()
            .fg(MUTED)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };

    frame.render_widget(Block::default().style(Style::default().bg(bg)), area);

    let value_width = value.chars().count() as u16;
    frame.render_widget(Paragraph::new(label).style(text_style), area);
    frame.render_widget(
        Paragraph::new(format!(" {value} ")).style(value_style),
        Rect {
            x: area.x + area.width.saturating_sub(value_width).saturating_sub(2),
            y: area.y,
            width: value_width + 2,
            height: area.height,
        },
    );
}

fn row_password(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    disabled: bool,
    red_asterisk: bool,
) {
    if disabled {
        let dimmed = Style::default().fg(MUTED).add_modifier(Modifier::DIM);
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::Reset)),
            area,
        );
        frame.render_widget(Paragraph::new(label).style(dimmed), area);
        frame.render_widget(Paragraph::new(value).style(dimmed), area);
        return;
    }
    let bg = if focused { SELECTED_BG } else { Color::Reset };
    let label_style = if focused {
        Style::default()
            .fg(ACCENT)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let val_style = if focused {
        Style::default()
            .fg(ACCENT)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT)
    };

    frame.render_widget(Block::default().style(Style::default().bg(bg)), area);

    let label_line = if red_asterisk {
        Line::from(vec![
            Span::styled(label, label_style),
            Span::styled("*", Style::default().fg(Color::Red)),
        ])
    } else {
        Line::from(vec![Span::styled(label, label_style)])
    };
    frame.render_widget(Paragraph::new(label_line), area);

    let value_width = value.chars().count() as u16;
    frame.render_widget(
        Paragraph::new(format!(" {value} ")).style(val_style),
        Rect {
            x: area.x + area.width.saturating_sub(value_width).saturating_sub(2),
            y: area.y,
            width: value_width + 2,
            height: area.height,
        },
    );
}

fn render_select_list(frame: &mut Frame, selector: &LibrarySelectState) {
    let inner = padded(frame.area());

    let block = Block::default()
        .style(Style::default().fg(ACCENT))
        .title(selector.title());
    let list_area = block.inner(inner);
    frame.render_widget(block, inner);

    let items: Vec<Line> = selector
        .options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            if i == selector.selected {
                Line::from(vec![Span::styled(
                    format!("> {opt}"),
                    Style::default()
                        .fg(ACCENT)
                        .bg(SELECTED_BG)
                        .add_modifier(Modifier::BOLD),
                )])
            } else {
                Line::from(vec![Span::styled(
                    format!("  {opt}"),
                    Style::default().fg(ACCENT),
                )])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(items), list_area);
}

fn render_path_row(
    frame: &mut Frame,
    area: Rect,
    value: &str,
    cursor: usize,
    focused: bool,
) -> Option<(u16, u16)> {
    let bg = if focused { SELECTED_BG } else { Color::Reset };
    let label_style = if focused {
        Style::default()
            .fg(ACCENT)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(ACCENT)
    };
    let val_style = if focused {
        Style::default()
            .fg(ACCENT)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };

    frame.render_widget(Block::default().style(Style::default().bg(bg)), area);

    let value_width = value.chars().count() as u16;
    let label = " Path";
    frame.render_widget(Paragraph::new(label).style(label_style), area);

    let val_x = area.x + area.width.saturating_sub(value_width).saturating_sub(2);
    frame.render_widget(
        Paragraph::new(format!(" {value} ")).style(val_style),
        Rect {
            x: val_x,
            y: area.y,
            width: value_width + 2,
            height: area.height,
        },
    );

    if focused {
        Some((val_x + 1 + cursor as u16, area.y))
    } else {
        None
    }
}

fn desc_area(section: Rect) -> (Rect, Rect) {
    let sub = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(section);
    (sub[0], sub[1])
}

fn render_desc(frame: &mut Frame, area: Rect, text: &str, focused: bool) {
    let bg = if focused { SELECTED_BG } else { Color::Reset };
    frame.render_widget(Block::default().style(Style::default().bg(bg)), area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::raw(format!(" {text} "))])).style(
            Style::default()
                .fg(MUTED)
                .bg(bg)
                .add_modifier(Modifier::DIM),
        ),
        area,
    );
}

fn render_action_buttons_overlay(
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
            .fg(ACCENT)
            .bg(SELECTED_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    frame.render_widget(
        Paragraph::new(cancel_text).style(cancel_style),
        btn_layout[1],
    );

    let confirm_style = if is_focused && selection == ButtonSelection::Confirm {
        Style::default()
            .fg(ACCENT)
            .bg(SELECTED_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    frame.render_widget(
        Paragraph::new(confirm_text).style(confirm_style),
        btn_layout[3],
    );
}

pub(crate) fn render_export_popup(frame: &mut Frame, state: &LibraryExportModalState) {
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

    let title_style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new("Export Automation").style(title_style),
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
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED).add_modifier(Modifier::DIM)
    };
    let feedback_text = state.error().unwrap_or("");
    frame.render_widget(
        Paragraph::new(feedback_text).style(feedback_style),
        sections[8],
    );

    render_action_buttons_overlay(
        frame,
        sections[10],
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

    let title_style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new("Import Automations").style(title_style),
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
        Paragraph::new("").style(Style::default().fg(MUTED).add_modifier(Modifier::DIM)),
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

    let title_style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    frame.render_widget(Paragraph::new(label).style(title_style), sections[0]);

    let pw_label_style = Style::default().fg(MUTED);
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
        let confirm_label_style = Style::default().fg(MUTED);
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
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            "Enter Next Field   Tab Switch   Ctrl+S Confirm   Esc Cancel".to_string(),
            Style::default().fg(MUTED).add_modifier(Modifier::DIM),
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

pub(crate) fn render_conflict_popup(
    frame: &mut Frame,
    incoming: &AutomationExport,
    existing: &ExistingAutomationConflict,
    selected: usize,
) {
    let inner = padded(frame.area());

    let title_style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new("Conflict Detected").style(title_style),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    let sub_style = Style::default().fg(MUTED);
    frame.render_widget(
        Paragraph::new(format!("  \"{}\" already exists.", incoming.name)).style(sub_style),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    let header_style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    let val_style = Style::default().fg(MUTED);

    let incoming_start = inner.y + 3;
    let incoming_lines = vec![
        Line::from(vec![Span::styled("Incoming:", header_style)]),
        Line::from(vec![
            Span::styled("  Trigger: ", Style::default().fg(ACCENT)),
            Span::styled(&incoming.trigger, val_style),
        ]),
        Line::from(vec![
            Span::styled("  Output: ", Style::default().fg(ACCENT)),
            Span::styled(&incoming.output, val_style),
        ]),
    ];

    let existing_lines = vec![
        Line::from(vec![Span::styled("Existing:", header_style)]),
        Line::from(vec![
            Span::styled("  Trigger: ", Style::default().fg(ACCENT)),
            Span::styled(&existing.trigger, val_style),
        ]),
        Line::from(vec![
            Span::styled("  Output: ", Style::default().fg(ACCENT)),
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
                .fg(ACCENT)
                .bg(SELECTED_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(ACCENT)
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
