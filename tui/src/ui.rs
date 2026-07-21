use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Padding, Paragraph, Row, Table,
    },
};
use taurine_core::stats::{HomeStats, MostUsedAutomation};

use crate::{
    app::{App, Page},
    control,
    library::{
        ButtonSelection, LibraryAutomation, LibraryDeleteModalState, LibraryEditorModalState,
        LibraryExportModalField, LibraryExportModalState, LibraryExportResultModalState,
        LibraryImportModalField, LibraryImportModalState, LibraryImportResultModalState,
        LibraryImportRunVariablesModalState, LibraryMetadataRow, LibraryModal, LibraryModalField,
        LibraryPageState, LibrarySelectState,
    },
    settings::{
        ConfirmResetModalState, InputModalState, SelectModalState, SettingKey, SettingsModal,
    },
};

use crate::theme::DARK_THEME;

const OUTER_HORIZONTAL_PADDING: u16 = 2;
const OUTER_VERTICAL_PADDING: u16 = 1;
const HEADER_GAP_HEIGHT: u16 = 1;
const FOOTER_GAP_HEIGHT: u16 = 1;
const EXPORT_RESULT_MODAL_TITLE: &str = "Export complete";
const IMPORT_RUN_VARIABLES_WARNING_LINES: [&str; 3] = [
    "CAUTION: This import contains [exec] variables that execute",
    "shell commands. Untrusted scripts can damage your system.",
    "Continue? [y/N]",
];
const FOOTER_HEIGHT: u16 = 1;
const PANEL_GAP_WIDTH: u16 = 1;
const NAV_WIDTH: u16 = 22;
const PANEL_PADDING: u16 = 1;
const NAV_TOGGLE_HINT: &str = "Ctrl+B Nav";
const LIBRARY_ITEM_HEIGHT: u16 = 2;
const LIBRARY_ITEM_GAP: u16 = 1;

pub(crate) fn render(frame: &mut Frame, app: &App) {
    frame.render_widget(
        ratatui::widgets::Block::default().style(Style::default().bg(DARK_THEME.bg)),
        frame.area(),
    );
    let area = frame.area().inner(Margin {
        vertical: OUTER_VERTICAL_PADDING,
        horizontal: OUTER_HORIZONTAL_PADDING,
    });
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(HEADER_GAP_HEIGHT),
            Constraint::Min(0),
            Constraint::Length(FOOTER_GAP_HEIGHT),
            Constraint::Length(FOOTER_HEIGHT),
        ])
        .split(area);

    render_header(frame, sections[0], app);
    render_body(frame, sections[2], app);
    render_footer(frame, sections[4], app);

    if let Some(msg) = app.notification() {
        render_notification(frame, msg);
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let status_width = app.daemon_status().label().len() as u16;
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(status_width)])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Taurine",
                Style::default()
                    .fg(DARK_THEME.text)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("v{}", env!("CARGO_PKG_VERSION")),
                Style::default()
                    .fg(DARK_THEME.muted)
                    .add_modifier(Modifier::DIM),
            ),
        ])),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(app.daemon_status().label())
            .alignment(Alignment::Right)
            .style(app.daemon_status().style().add_modifier(Modifier::BOLD)),
        sections[1],
    );
}

fn render_body(frame: &mut Frame, area: Rect, app: &App) {
    if app.nav_visible() {
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(NAV_WIDTH),
                Constraint::Length(PANEL_GAP_WIDTH),
                Constraint::Min(0),
            ])
            .split(area);

        render_navigation(frame, sections[0], app);
        render_content(frame, sections[2], app);
    } else {
        render_content(frame, area, app);
    }
}

fn render_navigation(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = Page::ALL
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let shortcut = char::from_digit((index + 1) as u32, 10).unwrap_or(' ');
            let line = Line::from(vec![
                Span::styled(
                    shortcut.to_string(),
                    Style::default()
                        .fg(DARK_THEME.muted)
                        .add_modifier(Modifier::DIM),
                ),
                Span::raw(" "),
                Span::raw(page.title()),
            ]);

            ListItem::new(line)
        })
        .collect();

    let navigation_block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(DARK_THEME.border))
        .padding(Padding::new(
            PANEL_PADDING,
            PANEL_PADDING,
            PANEL_PADDING,
            PANEL_PADDING,
        ));

    let navigation = List::new(items)
        .block(navigation_block)
        .highlight_symbol("")
        .highlight_style(
            Style::default()
                .bg(DARK_THEME.border)
                .fg(DARK_THEME.text)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    state.select(Some(app.active_page().nav_index()));
    frame.render_stateful_widget(navigation, area, &mut state);
}

fn render_content(frame: &mut Frame, area: Rect, app: &App) {
    let content_block = Block::default()
        .title(Span::styled(
            format!(" {} ", app.active_page().title()),
            Style::default()
                .fg(DARK_THEME.text)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(DARK_THEME.border))
        .padding(Padding::new(
            PANEL_PADDING,
            PANEL_PADDING,
            PANEL_PADDING,
            PANEL_PADDING,
        ));

    let inner = content_block.inner(area);
    frame.render_widget(content_block, area);

    match app.active_page() {
        Page::Home => render_home_content(frame, inner, app.home_stats()),
        Page::Library => {
            render_library_content(frame, inner, app.library_page());
            if let Some(modal) = app.library_page().modal() {
                match modal {
                    LibraryModal::Editor(state) => render_library_editor_modal(frame, inner, state),
                    LibraryModal::Export(state) => render_library_export_modal(frame, inner, state),
                    LibraryModal::ExportResult(state) => {
                        render_library_export_result_modal(frame, inner, state)
                    }
                    LibraryModal::Import(state) => render_library_import_modal(frame, inner, state),
                    LibraryModal::ImportResult(state) => {
                        render_library_import_result_modal(frame, inner, state)
                    }
                    LibraryModal::ConfirmImportRunVariables(state) => {
                        render_library_import_run_variables_modal(frame, inner, state)
                    }
                    LibraryModal::ConfirmDelete(state) => {
                        render_library_delete_modal(frame, inner, state)
                    }
                }
            }
        }
        Page::Settings => {
            render_settings_content(frame, inner, app);
            if let Some(modal) = app.settings_page().modal() {
                render_settings_modal(frame, inner, modal);
            }
        }
    }
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let footer_text = footer_text(app);
    let footer_line = Line::from(vec![Span::styled(
        footer_text,
        Style::default()
            .fg(DARK_THEME.muted)
            .add_modifier(Modifier::DIM),
    )]);

    frame.render_widget(Paragraph::new(footer_line).alignment(Alignment::Left), area);
}

fn footer_text(app: &App) -> String {
    match app.active_page() {
        Page::Home => home_footer_with_nav(control::home_footer_label(app.daemon_status())),
        Page::Library => library_footer_with_nav(app.library_page().footer_text()),
        Page::Settings => settings_footer_with_nav(app.settings_page().footer_text()),
    }
}

fn home_footer_with_nav(home_footer: &str) -> String {
    if home_footer.is_empty() {
        NAV_TOGGLE_HINT.to_string()
    } else if home_footer.contains(NAV_TOGGLE_HINT) {
        home_footer.to_string()
    } else {
        format!("{NAV_TOGGLE_HINT}   {home_footer}")
    }
}

fn library_footer_with_nav(library_footer: &str) -> String {
    if library_footer.is_empty() {
        NAV_TOGGLE_HINT.to_string()
    } else if library_footer.contains(NAV_TOGGLE_HINT) {
        library_footer.to_string()
    } else {
        format!("{NAV_TOGGLE_HINT}   {library_footer}")
    }
}

fn settings_footer_with_nav(settings_footer: &str) -> String {
    if settings_footer.is_empty() {
        NAV_TOGGLE_HINT.to_string()
    } else if settings_footer.starts_with("Ctrl+B") {
        settings_footer.to_string()
    } else {
        format!("Ctrl+B Nav   {settings_footer}")
    }
}

fn render_home_content(frame: &mut Frame, area: Rect, stats: &HomeStats) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    render_stat_cards(frame, sections[0], stats);

    let activity_sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(PANEL_GAP_WIDTH),
            Constraint::Percentage(50),
        ])
        .split(sections[2]);

    render_most_used_list(
        frame,
        activity_sections[0],
        "TOP AUTOMATIONS",
        &stats.most_used_words,
    );
    render_most_used_list(
        frame,
        activity_sections[2],
        "TOP HOTKEYS",
        &stats.most_used_hotkeys,
    );
}

fn render_library_content(frame: &mut Frame, area: Rect, library_page: &LibraryPageState) {
    if let Some(message) = library_page.load_error() {
        frame.render_widget(
            Paragraph::new(message).style(
                Style::default()
                    .fg(DARK_THEME.error)
                    .add_modifier(Modifier::BOLD),
            ),
            area,
        );
        return;
    }

    let has_status = library_page.status_message().is_some();
    let sections = if has_status {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(area)
    };

    let search_index = if has_status {
        if let Some(message) = library_page.status_message() {
            frame.render_widget(
                Paragraph::new(message).style(
                    Style::default()
                        .fg(DARK_THEME.text)
                        .add_modifier(Modifier::BOLD),
                ),
                sections[0],
            );
        }
        1
    } else {
        0
    };

    render_library_search_bar(frame, sections[search_index], library_page);

    let list_area = sections[search_index + 2];
    if list_area.height == 0 {
        return;
    }

    if let Some(message) = library_page.empty_state_message() {
        frame.render_widget(
            Paragraph::new(message).style(
                Style::default()
                    .fg(DARK_THEME.muted)
                    .add_modifier(Modifier::DIM),
            ),
            list_area,
        );
        return;
    }

    let visible_count = visible_library_item_capacity(list_area.height);
    if visible_count == 0 {
        return;
    }

    let selected_index = library_page.selected_index().unwrap_or(0);
    let (start, end) =
        visible_library_range(library_page.filtered_len(), selected_index, visible_count);

    for (visible_index, filtered_index) in (start..end).enumerate() {
        let row_area = Rect {
            x: list_area.x,
            y: list_area.y + (visible_index as u16 * (LIBRARY_ITEM_HEIGHT + LIBRARY_ITEM_GAP)),
            width: list_area.width,
            height: LIBRARY_ITEM_HEIGHT,
        };

        let Some(item) = library_page.item_at_filtered(filtered_index) else {
            continue;
        };

        render_library_item(
            frame,
            row_area,
            item,
            library_page.selected_index() == Some(filtered_index),
        );
    }
}

fn render_library_search_bar(frame: &mut Frame, area: Rect, library_page: &LibraryPageState) {
    frame.render_widget(
        Block::default().style(Style::default().bg(DARK_THEME.highlight)),
        area,
    );

    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(area);

    let label_style = if library_page.is_search_active() {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(DARK_THEME.muted)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    };
    frame.render_widget(Paragraph::new("Search").style(label_style), sections[0]);

    let query_style = if library_page.search_query().is_empty() && !library_page.is_search_active()
    {
        Style::default()
            .fg(DARK_THEME.muted)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.highlight)
    };
    let query = if library_page.is_search_active() {
        input_cursor_line(
            library_page.search_query(),
            library_page.search_query().chars().count(),
        )
    } else if library_page.search_query().is_empty() {
        Line::from(" ")
    } else {
        Line::from(library_page.search_query().to_string())
    };
    frame.render_widget(Paragraph::new(query).style(query_style), sections[1]);
}

fn render_library_item(frame: &mut Frame, area: Rect, item: &LibraryAutomation, selected: bool) {
    let row_bg = if selected {
        DARK_THEME.highlight
    } else {
        Color::Reset
    };
    frame.render_widget(Block::default().style(Style::default().bg(row_bg)), area);

    let kind_width = (item.kind_label().chars().count() as u16).min(area.width.saturating_sub(2));
    let metadata = item.metadata_label();
    let metadata_width = (metadata.chars().count() as u16).min(area.width.saturating_sub(2));

    let top_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let bottom_area = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: 1,
    };

    let top_sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(kind_width)])
        .split(top_area);
    let bottom_sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(metadata_width)])
        .split(bottom_area);

    let trigger_style = if selected {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(row_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(DARK_THEME.text)
            .add_modifier(Modifier::BOLD)
    };
    let kind_style = if selected {
        Style::default().fg(DARK_THEME.muted).bg(row_bg)
    } else {
        Style::default().fg(DARK_THEME.muted)
    };
    let preview_style = if selected {
        Style::default()
            .fg(DARK_THEME.muted)
            .bg(row_bg)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default()
            .fg(DARK_THEME.muted)
            .add_modifier(Modifier::DIM)
    };

    frame.render_widget(
        Paragraph::new(truncate_to_width(item.trigger(), top_sections[0].width))
            .style(trigger_style),
        top_sections[0],
    );
    frame.render_widget(
        Paragraph::new(truncate_to_width(item.kind_label(), top_sections[1].width))
            .alignment(Alignment::Right)
            .style(kind_style),
        top_sections[1],
    );
    frame.render_widget(
        Paragraph::new(truncate_to_width(item.preview(), bottom_sections[0].width))
            .style(preview_style),
        bottom_sections[0],
    );
    frame.render_widget(
        Paragraph::new(truncate_to_width(&metadata, bottom_sections[1].width))
            .alignment(Alignment::Right)
            .style(preview_style),
        bottom_sections[1],
    );
}

fn render_library_export_modal(frame: &mut Frame, area: Rect, state: &LibraryExportModalState) {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(76)
    } else {
        area.width.max(1)
    };
    let height = area.height.clamp(1, 14);
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let inner = render_modal_block(frame, popup, "Export Automations");

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

    render_modal_field_label(
        frame,
        sections[0],
        "Path",
        state.focus() == LibraryExportModalField::Path,
        None,
    );
    render_modal_input_field(
        frame,
        sections[1],
        state.path(),
        state.path_cursor(),
        state.focus() == LibraryExportModalField::Path,
    );

    let encrypt = state.encrypt();
    let focused = |field| state.focus() == field;

    render_modal_key_value_row(
        frame,
        sections[2],
        "Encrypt",
        yes_no_label(encrypt),
        focused(LibraryExportModalField::Encrypt),
        false,
    );

    let password_focused = focused(LibraryExportModalField::Password);
    render_modal_password_row(
        frame,
        sections[3],
        "Password",
        &state.password_display_value(),
        state.password_cursor(),
        password_focused && encrypt,
        !encrypt,
        false,
    );

    render_modal_key_value_row(
        frame,
        sections[4],
        "Settings",
        yes_no_label(state.include_settings()),
        focused(LibraryExportModalField::IncludeSettings),
        false,
    );
    let sensitive_focused = focused(LibraryExportModalField::IncludeSensitiveSettings);
    render_modal_key_value_row(
        frame,
        sections[5],
        "Sensitive",
        yes_no_label(state.include_sensitive_settings()),
        sensitive_focused && encrypt,
        !encrypt,
    );
    render_modal_key_value_row(
        frame,
        sections[6],
        "Stats",
        yes_no_label(state.include_stats()),
        focused(LibraryExportModalField::IncludeStats),
        false,
    );

    let feedback_area = sections[7];
    let (text, style) = if let Some(error) = state.error() {
        (
            error,
            Style::default()
                .fg(DARK_THEME.error)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            "",
            Style::default()
                .fg(DARK_THEME.muted)
                .add_modifier(Modifier::DIM),
        )
    };
    frame.render_widget(Paragraph::new(text).style(style), feedback_area);

    let buttons_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(0),
            Constraint::Length(1),
            Constraint::Length(0),
        ])
        .split(sections[8]);
    render_action_buttons(
        frame,
        buttons_area[1],
        "Cancel",
        "Export",
        focused(LibraryExportModalField::ActionButton),
        state.button_selection(),
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

fn render_library_import_modal(frame: &mut Frame, area: Rect, state: &LibraryImportModalState) {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(76)
    } else {
        area.width.max(1)
    };
    let popup = centered_rect(width, area.height.clamp(1, 14), area);
    frame.render_widget(Clear, popup);
    let inner = render_modal_block(frame, popup, "Import Automations");

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

    render_modal_field_label(
        frame,
        sections[0],
        "Path",
        state.focus() == LibraryImportModalField::Path,
        None,
    );
    render_modal_input_field(
        frame,
        sections[1],
        state.path(),
        state.path_cursor(),
        state.focus() == LibraryImportModalField::Path,
    );

    let password_focused = state.focus() == LibraryImportModalField::Password;
    let password_disabled = state.is_encrypted() == Some(false);
    let show_red_asterisk = state.is_encrypted() == Some(true)
        && state.password().is_empty()
        && state.error().is_some();
    render_modal_password_row(
        frame,
        sections[3],
        "Password",
        &state.password_display_value(),
        state.password_cursor(),
        password_focused && !password_disabled,
        password_disabled,
        show_red_asterisk,
    );

    render_modal_key_value_row(
        frame,
        sections[4],
        "Settings",
        yes_no_label(state.include_settings()),
        state.focus() == LibraryImportModalField::IncludeSettings,
        false,
    );
    render_modal_key_value_row(
        frame,
        sections[5],
        "Sensitive",
        yes_no_label(state.include_sensitive_settings()),
        state.focus() == LibraryImportModalField::IncludeSensitiveSettings,
        false,
    );
    render_modal_key_value_row(
        frame,
        sections[6],
        "Stats",
        state.stats_mode_label(),
        state.focus() == LibraryImportModalField::StatsMode,
        false,
    );
    render_modal_key_value_row(
        frame,
        sections[7],
        "Conflicts",
        state.conflict_mode_label(),
        state.focus() == LibraryImportModalField::ConflictMode,
        false,
    );

    frame.render_widget(
        Paragraph::new("").style(
            Style::default()
                .fg(DARK_THEME.muted)
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
    render_action_buttons(
        frame,
        buttons_area[1],
        "Cancel",
        "Import",
        state.focus() == LibraryImportModalField::ActionButton,
        state.button_selection(),
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
        render_library_select_modal(frame, area, selector);
    }
}

fn render_library_import_run_variables_modal(
    frame: &mut Frame,
    area: Rect,
    state: &LibraryImportRunVariablesModalState,
) {
    let width = if area.width > 52 {
        area.width.saturating_sub(6).min(72)
    } else {
        area.width.max(1)
    };
    let popup = centered_rect(width, 9.min(area.height.max(1)), area);
    frame.render_widget(Clear, popup);
    let inner = render_modal_block(frame, popup, "Run Variables Warning");

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
        .style(Style::default().fg(DARK_THEME.muted)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(truncate_to_width(state.path(), sections[1].width)).style(
            Style::default()
                .fg(DARK_THEME.text)
                .add_modifier(Modifier::BOLD),
        ),
        sections[1],
    );

    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(DARK_THEME.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(DARK_THEME.muted)
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
    let inner = render_modal_block(frame, popup, "Import complete");

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
        Paragraph::new(lines).style(Style::default().fg(DARK_THEME.muted)),
        sections[0],
    );
}

fn render_library_export_result_modal(
    frame: &mut Frame,
    area: Rect,
    state: &LibraryExportResultModalState,
) {
    let width = if area.width > 48 {
        area.width.saturating_sub(6).min(76)
    } else {
        area.width.max(1)
    };
    let popup = centered_rect(width, 5.min(area.height.max(1)), area);
    frame.render_widget(Clear, popup);
    let inner = render_modal_block(frame, popup, EXPORT_RESULT_MODAL_TITLE);

    frame.render_widget(
        Paragraph::new(vec![Line::from(state.body())]).style(Style::default().fg(DARK_THEME.muted)),
        inner,
    );
}

fn render_library_editor_modal(frame: &mut Frame, area: Rect, state: &LibraryEditorModalState) {
    let width = ((area.width as u32 * 4) / 5) as u16;
    let height = ((area.height as u32 * 4) / 5) as u16;
    let popup = centered_rect(width.max(48), height.max(12), area);
    frame.render_widget(Clear, popup);
    let inner = render_modal_block(frame, popup, "Automation");

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

    render_modal_field_label(
        frame,
        sections[0],
        "Trigger",
        state.focus() == LibraryModalField::Trigger,
        None,
    );
    render_modal_input_field(
        frame,
        sections[1],
        state.trigger(),
        state.trigger_cursor(),
        state.focus() == LibraryModalField::Trigger,
    );

    render_modal_field_label(
        frame,
        sections[2],
        state.content_label(),
        state.focus() == LibraryModalField::Content,
        state.content_line_indicator(sections[3].height),
    );
    render_modal_content_field(frame, sections[3], state);

    render_modal_metadata(frame, sections[4], state, state.metadata_rows());
    render_library_editor_feedback(frame, sections[5], state);

    if state.focus() == LibraryModalField::Trigger {
        frame.set_cursor_position((
            sections[1].x + 1 + state.trigger_cursor() as u16,
            sections[1].y,
        ));
    } else if state.focus() == LibraryModalField::Content {
        let (cursor_line, cursor_col) =
            crate::library::line_col_for_char_index(state.content(), state.content_cursor());
        let scroll = state.effective_content_scroll(sections[3].height);
        frame.set_cursor_position((
            sections[3].x + 1 + cursor_col as u16,
            sections[3].y + cursor_line.saturating_sub(scroll) as u16,
        ));
    }

    if let Some(selector) = state.selector() {
        render_library_select_modal(frame, area, selector);
    }
}

fn render_library_delete_modal(frame: &mut Frame, area: Rect, state: &LibraryDeleteModalState) {
    let width = if area.width > 44 {
        area.width.saturating_sub(4).min(64)
    } else {
        area.width.max(1)
    };
    let height = 8;
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let inner = render_modal_block(frame, popup, "Delete Automation");

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
        Paragraph::new("Do you want to delete this automation?")
            .style(Style::default().fg(DARK_THEME.muted)),
        sections[0],
    );

    frame.render_widget(
        Paragraph::new(truncate_to_width(state.name(), sections[1].width)).style(
            Style::default()
                .fg(DARK_THEME.text)
                .add_modifier(Modifier::BOLD),
        ),
        sections[1],
    );

    let yes_style = if state.selected_yes() {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.muted)
    };
    let no_style = if !state.selected_yes() {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.muted)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Yes  ", yes_style),
            Span::raw("    "),
            Span::styled("  No  ", no_style),
        ]))
        .alignment(Alignment::Center),
        sections[2],
    );

    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(DARK_THEME.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(DARK_THEME.muted)
            .add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        Paragraph::new(state.error().unwrap_or("")).style(feedback_style),
        sections[3],
    );
}

fn render_modal_field_label(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    focused: bool,
    indicator: Option<String>,
) {
    let indicator_width = indicator
        .as_ref()
        .map(|value| value.chars().count() as u16)
        .unwrap_or_default();
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(indicator_width)])
        .split(area);

    let label_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.muted)
    };
    frame.render_widget(Paragraph::new(label).style(label_style), sections[0]);

    if let Some(indicator) = indicator {
        frame.render_widget(
            Paragraph::new(indicator).alignment(Alignment::Right).style(
                Style::default()
                    .fg(DARK_THEME.muted)
                    .add_modifier(Modifier::DIM),
            ),
            sections[1],
        );
    }
}

fn render_modal_input_field(
    frame: &mut Frame,
    area: Rect,
    value: &str,
    cursor: usize,
    focused: bool,
) {
    let bg = if focused {
        DARK_THEME.highlight
    } else {
        DARK_THEME.bg
    };
    let text_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text).bg(bg)
    };

    let block = Block::default()
        .style(Style::default().bg(bg))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let text = if focused {
        Paragraph::new(input_cursor_line(value, cursor))
    } else {
        Paragraph::new(value.to_string())
    };
    frame.render_widget(text.style(text_style), inner);
}

#[allow(clippy::too_many_arguments)]
fn render_modal_password_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    cursor: usize,
    focused: bool,
    disabled: bool,
    red_asterisk: bool,
) {
    let label_width = area.width.min(12);
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(label_width), Constraint::Min(0)])
        .split(area);

    let (label_style, value_style) = if disabled {
        let dimmed = Style::default()
            .fg(DARK_THEME.muted)
            .add_modifier(Modifier::DIM);
        (dimmed, dimmed)
    } else if focused {
        (
            Style::default()
                .fg(DARK_THEME.text)
                .add_modifier(Modifier::BOLD),
            Style::default()
                .fg(DARK_THEME.text)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(DARK_THEME.text),
            Style::default().fg(DARK_THEME.text),
        )
    };

    let label_line = if red_asterisk {
        Line::from(vec![
            Span::styled(label, label_style),
            Span::styled("*", Style::default().fg(DARK_THEME.error)),
        ])
    } else {
        Line::from(vec![Span::styled(label, label_style)])
    };
    frame.render_widget(Paragraph::new(label_line), sections[0]);

    let text = Paragraph::new(input_cursor_line(value, cursor));
    frame.render_widget(text.style(value_style), sections[1]);
}

fn render_modal_content_field(frame: &mut Frame, area: Rect, state: &LibraryEditorModalState) {
    let focused = state.focus() == LibraryModalField::Content;
    let bg = if focused {
        DARK_THEME.highlight
    } else {
        DARK_THEME.bg
    };
    let block = Block::default()
        .style(Style::default().bg(bg))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let scroll = state.effective_content_scroll(inner.height);
    let (cursor_line, cursor_col) =
        crate::library::line_col_for_char_index(state.content(), state.content_cursor());
    let visible_lines = state.visible_content_lines(inner.height);
    let rendered = visible_lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let absolute_line = scroll + index;
            if focused && absolute_line == cursor_line {
                input_cursor_line(&line, cursor_col)
            } else {
                Line::from(line)
            }
        })
        .collect::<Vec<_>>();

    let content_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text).bg(bg)
    };
    frame.render_widget(Paragraph::new(rendered).style(content_style), inner);
}

fn render_modal_metadata(
    frame: &mut Frame,
    area: Rect,
    state: &LibraryEditorModalState,
    metadata_rows: &[LibraryMetadataRow],
) {
    let mut rows = Vec::with_capacity(metadata_rows.len() + 2);
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
        render_modal_key_value_row(frame, row_area, label, &value, focused, quiet);
    }
}

fn render_library_editor_feedback(frame: &mut Frame, area: Rect, state: &LibraryEditorModalState) {
    let (text, style) = if let Some(error) = state.error() {
        (
            error,
            Style::default()
                .fg(DARK_THEME.error)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            "",
            Style::default()
                .fg(DARK_THEME.muted)
                .add_modifier(Modifier::DIM),
        )
    };

    frame.render_widget(Paragraph::new(text).style(style), area);
}

fn render_modal_key_value_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    quiet: bool,
) {
    let bg = if focused {
        DARK_THEME.highlight
    } else {
        Color::Reset
    };
    frame.render_widget(Block::default().style(Style::default().bg(bg)), area);

    let label_width = area.width.min(12);
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(label_width), Constraint::Min(0)])
        .split(area);

    let label_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else if quiet {
        Style::default()
            .fg(DARK_THEME.muted)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(DARK_THEME.muted)
    };
    let value_style = if focused {
        Style::default().fg(DARK_THEME.text).bg(bg)
    } else if quiet {
        Style::default()
            .fg(DARK_THEME.muted)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(DARK_THEME.text)
    };

    frame.render_widget(Paragraph::new(label).style(label_style), sections[0]);
    frame.render_widget(
        Paragraph::new(truncate_to_width(value, sections[1].width)).style(value_style),
        sections[1],
    );
}

fn yes_no_label(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn render_stat_cards(frame: &mut Frame, area: Rect, stats: &HomeStats) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Length(2),
            Constraint::Percentage(34),
            Constraint::Length(2),
            Constraint::Percentage(33),
        ])
        .split(area);

    render_stat_card(
        frame,
        sections[0],
        "keystrokes saved",
        &format_number(stats.keystrokes_saved),
    );
    render_stat_card(
        frame,
        sections[2],
        "time saved",
        &format_time_saved(stats.time_saved_ms),
    );
    render_stat_card(
        frame,
        sections[4],
        "expansions run",
        &format_number(stats.expansions_run),
    );
}

fn render_stat_card(frame: &mut Frame, area: Rect, label: &str, value: &str) {
    let block = Block::default()
        .style(Style::default().bg(DARK_THEME.highlight))
        .padding(Padding::new(2, 2, 1, 1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(value).style(
            Style::default()
                .fg(DARK_THEME.text)
                .add_modifier(Modifier::BOLD),
        ),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(DARK_THEME.muted)),
        sections[1],
    );
}

fn render_most_used_list(frame: &mut Frame, area: Rect, title: &str, rows: &[MostUsedAutomation]) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(title).style(
            Style::default()
                .fg(DARK_THEME.muted)
                .add_modifier(Modifier::BOLD),
        ),
        sections[0],
    );

    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No activity recorded yet.").style(
                Style::default()
                    .fg(DARK_THEME.muted)
                    .add_modifier(Modifier::DIM),
            ),
            sections[2],
        );
        return;
    }

    let header = Row::new([Cell::from(" TRIGGER"), Cell::from("USES ")])
        .style(
            Style::default()
                .fg(DARK_THEME.text)
                .bg(DARK_THEME.highlight)
                .add_modifier(Modifier::BOLD),
        )
        .height(1);

    let table_rows = rows.iter().take(8).map(|automation| {
        Row::new([
            Cell::from(format!(" {}", automation.trigger))
                .style(Style::default().fg(DARK_THEME.text)),
            Cell::from(format!("{} ", format_number(automation.uses)))
                .style(Style::default().fg(DARK_THEME.muted)),
        ])
    });

    let table = Table::new(table_rows, [Constraint::Min(15), Constraint::Length(8)])
        .header(header)
        .column_spacing(1);

    frame.render_widget(table, sections[2]);
}

fn render_settings_content(frame: &mut Frame, area: Rect, app: &App) {
    let settings_page = app.settings_page();
    if let Some(message) = settings_page.load_error() {
        frame.render_widget(
            Paragraph::new(message).style(
                Style::default()
                    .fg(DARK_THEME.error)
                    .add_modifier(Modifier::BOLD),
            ),
            area,
        );
        return;
    }

    let status_message = settings_page.status_message();
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
                    .fg(DARK_THEME.error)
                    .add_modifier(Modifier::BOLD),
            ),
            sections[0],
        );
    }

    let list_area = sections[sections.len() - 1];
    if list_area.height == 0 {
        return;
    }

    let all_keys = settings_page.visible_keys();
    let spacious = use_spacious_settings_layout(list_area.height, all_keys.len(), 0);
    let row_height = if spacious { 2 } else { 1 };
    let visible_count = usize::from((list_area.height / row_height).max(1));
    let (start, end) = visible_setting_range(
        all_keys.len(),
        settings_page.selected_index(),
        visible_count,
    );
    let control_width = control_column_width(settings_page.settings(), list_area.width);

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
            *key,
            settings_page.settings(),
            settings_page.selected_key() == *key,
            spacious,
            control_width,
        );
    }
}

fn render_setting_row(
    frame: &mut Frame,
    area: Rect,
    key: SettingKey,
    settings: &taurine_core::settings::Settings,
    selected: bool,
    spacious: bool,
    control_width: u16,
) {
    let row_style = if selected {
        Style::default().bg(DARK_THEME.highlight)
    } else {
        Style::default()
    };
    frame.render_widget(Block::default().style(row_style), area);

    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(control_width)])
        .split(area);

    let label_style = if selected {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text)
    };
    let value_style = if selected {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.muted)
    };

    frame.render_widget(
        Paragraph::new(key.display_name()).style(label_style),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "[ {} ]",
            truncate_to_width(
                &key.display_value(settings),
                control_width.saturating_sub(4)
            )
        ))
        .alignment(Alignment::Right)
        .style(value_style),
        sections[1],
    );

    if spacious && area.height > 1 {
        let description_area = Rect {
            x: sections[0].x,
            y: area.y + 1,
            width: sections[0].width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(key.description()).style(
                Style::default()
                    .fg(DARK_THEME.muted)
                    .bg(if selected {
                        DARK_THEME.highlight
                    } else {
                        Color::Reset
                    })
                    .add_modifier(Modifier::DIM),
            ),
            description_area,
        );
    }
}

fn render_settings_modal(frame: &mut Frame, area: Rect, modal: &SettingsModal) {
    match modal {
        SettingsModal::Input(state) => render_input_modal(frame, area, state),
        SettingsModal::Select(state) => render_select_modal(frame, area, state),
        SettingsModal::ConfirmReset(state) => render_confirm_reset_modal(frame, area, state),
    }
}

fn render_input_modal(frame: &mut Frame, area: Rect, state: &InputModalState) {
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
    let inner = render_modal_block(frame, popup, state.key().display_name());

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
                .fg(DARK_THEME.muted)
                .add_modifier(Modifier::DIM),
        ),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(input_cursor_line(state.value(), state.cursor())).style(
            Style::default()
                .fg(DARK_THEME.text)
                .bg(DARK_THEME.highlight),
        ),
        sections[1],
    );

    let feedback = state.error().unwrap_or("Enter Save   Esc Cancel");
    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(DARK_THEME.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(DARK_THEME.muted)
            .add_modifier(Modifier::DIM)
    };
    frame.render_widget(Paragraph::new(feedback).style(feedback_style), sections[2]);
}

fn render_library_select_modal(frame: &mut Frame, area: Rect, state: &LibrarySelectState) {
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
    let inner = render_modal_block(frame, popup, state.title());

    let items: Vec<ListItem> = state
        .options
        .iter()
        .map(|option| ListItem::new(option.as_str()))
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected));

    let list = List::new(items).highlight_symbol("").highlight_style(
        Style::default()
            .bg(DARK_THEME.highlight)
            .fg(DARK_THEME.text)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(list, inner, &mut list_state);
}

fn render_select_modal(frame: &mut Frame, area: Rect, state: &SelectModalState) {
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
    let inner = render_modal_block(frame, popup, state.key().display_name());

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    let visible_rows = usize::from(sections[0].height.max(1));
    let (start, end) =
        visible_setting_range(state.options().len(), state.selected_index(), visible_rows);
    let items: Vec<ListItem> = state.options()[start..end]
        .iter()
        .map(|option| ListItem::new(option.as_str()))
        .collect();
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_index().saturating_sub(start)));

    let list = List::new(items).highlight_symbol("").highlight_style(
        Style::default()
            .bg(DARK_THEME.highlight)
            .fg(DARK_THEME.text)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(list, sections[0], &mut list_state);

    let feedback = state.error().unwrap_or("Enter Save   Esc Cancel");
    let feedback_style = if state.error().is_some() {
        Style::default()
            .fg(DARK_THEME.error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(DARK_THEME.muted)
            .add_modifier(Modifier::DIM)
    };
    frame.render_widget(Paragraph::new(feedback).style(feedback_style), sections[1]);
}

fn render_confirm_reset_modal(frame: &mut Frame, area: Rect, state: &ConfirmResetModalState) {
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
    let inner = render_modal_block(frame, popup, "Reset Setting");

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
        .style(Style::default().fg(DARK_THEME.muted)),
        sections[0],
    );

    let yes_style = if state.selected_yes() {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.muted)
    };
    let no_style = if !state.selected_yes() {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.muted)
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
                    .fg(DARK_THEME.error)
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

fn render_action_buttons(
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
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text)
    };
    frame.render_widget(
        Paragraph::new(cancel_text).style(cancel_style),
        btn_layout[1],
    );

    let confirm_style = if is_focused && selection == ButtonSelection::Confirm {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(DARK_THEME.highlight)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text)
    };
    frame.render_widget(
        Paragraph::new(confirm_text).style(confirm_style),
        btn_layout[3],
    );
}

fn render_modal_block(frame: &mut Frame, popup: Rect, title: &str) -> Rect {
    let block = Block::default()
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(DARK_THEME.text)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(DARK_THEME.border))
        .padding(Padding::new(1, 1, 1, 1));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    inner
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
    visible_range(total, selected, visible_count)
}

fn visible_library_range(total: usize, selected: usize, visible_count: usize) -> (usize, usize) {
    visible_range(total, selected, visible_count)
}

fn visible_range(total: usize, selected: usize, visible_count: usize) -> (usize, usize) {
    if total <= visible_count {
        return (0, total);
    }

    let mut start = selected.saturating_sub(visible_count.saturating_sub(1));
    let mut end = (start + visible_count).min(total);
    if end - start < visible_count {
        start = end.saturating_sub(visible_count);
        end = total.min(start + visible_count);
    }
    (start, end)
}

fn visible_library_item_capacity(available_height: u16) -> usize {
    if available_height < LIBRARY_ITEM_HEIGHT {
        return 0;
    }

    usize::from((available_height + LIBRARY_ITEM_GAP) / (LIBRARY_ITEM_HEIGHT + LIBRARY_ITEM_GAP))
}

fn control_column_width(settings: &taurine_core::settings::Settings, area_width: u16) -> u16 {
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

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(width) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn input_cursor_line(value: &str, cursor: usize) -> Line<'static> {
    let total_chars = value.chars().count();
    if total_chars == 0 {
        return Line::from(vec![Span::styled(
            " ",
            Style::default()
                .fg(DARK_THEME.text)
                .bg(DARK_THEME.highlight)
                .add_modifier(Modifier::BOLD),
        )]);
    }

    let safe_cursor = cursor.min(total_chars);
    let chars: Vec<char> = value.chars().collect();
    let before: String = chars.iter().take(safe_cursor).collect();

    if safe_cursor == total_chars {
        return Line::from(vec![
            Span::raw(before),
            Span::styled(
                " ",
                Style::default()
                    .fg(DARK_THEME.text)
                    .bg(DARK_THEME.highlight)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
    }

    let current = chars[safe_cursor];
    let after: String = chars.iter().skip(safe_cursor + 1).collect();
    Line::from(vec![
        Span::raw(before),
        Span::styled(
            current.to_string(),
            Style::default()
                .fg(DARK_THEME.text)
                .bg(DARK_THEME.highlight)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(after),
    ])
}

fn truncate_to_width(value: &str, max_chars: u16) -> String {
    let limit = usize::from(max_chars);
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= limit {
        return value.to_string();
    }
    if limit <= 3 {
        return chars.into_iter().take(limit).collect();
    }

    let truncated: String = chars.into_iter().take(limit - 3).collect();
    format!("{truncated}...")
}

fn format_number(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }

    formatted.chars().rev().collect()
}

fn format_time_saved(time_saved_ms: u64) -> String {
    let total_minutes = time_saved_ms / 60_000;
    let total_hours = total_minutes / 60;
    let total_days = total_hours / 24;

    if total_days > 0 {
        let remaining_hours = total_hours % 24;
        if remaining_hours > 0 {
            format!("{total_days}d {remaining_hours}h")
        } else {
            format!("{total_days}d")
        }
    } else if total_hours > 0 {
        let remaining_minutes = total_minutes % 60;
        if remaining_minutes > 0 {
            format!("{total_hours}h {remaining_minutes}m")
        } else {
            format!("{total_hours}h")
        }
    } else {
        format!("{total_minutes}m")
    }
}

fn render_notification(frame: &mut Frame, message: &str) {
    let area = frame.area();
    let width = message.len() as u16 + 4;
    let x = area.right().saturating_sub(width).saturating_sub(2);
    let y = area.y.saturating_add(2);
    let popup = Rect {
        x,
        y,
        width: width.min(area.width.saturating_sub(x)),
        height: 3,
    };
    if popup.width < 3 {
        return;
    }
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(DARK_THEME.error))
        .style(Style::default().bg(DARK_THEME.bg).fg(DARK_THEME.error));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(message).style(
            Style::default()
                .fg(DARK_THEME.error)
                .add_modifier(Modifier::BOLD),
        ),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::{
        db::crud::{AutomationRow, TriggerType},
        engine::shell::{ScriptBehavior, ScriptInterpreter, compress},
    };

    fn sample_library_modal() -> crate::library::LibraryAutomationDetail {
        crate::library::LibraryAutomationDetail::from_row(AutomationRow {
            id: "library-modal".to_string(),
            name: "Library Modal".to_string(),
            description: Some("Open Reddit".to_string()),
            trigger_type: TriggerType::Hotkey,
            trigger: "alt+r".to_string(),
            output: "[Script: powershell]".to_string(),
            action_type: "script".to_string(),
            target_os: "win".to_string(),
            only_apps: None,
            except_apps: None,
            tags: "[]".to_string(),
            usage_count: 6,
            last_used_at: Some(1),
            created_at: 1,
            updated_at: 1,
            version: 1,
            is_deleted: false,
            is_synced: true,
            is_enabled: true,
            auto_case: false,
            interpreter: Some(ScriptInterpreter::PowerShell),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: Some(compress("Start-Process https://reddit.com").unwrap()),
        })
        .unwrap()
    }

    #[test]
    fn formats_zero_time_saved_as_zero_minutes() {
        assert_eq!(format_time_saved(0), "0m");
    }

    #[test]
    fn formats_one_hour_without_trailing_minutes() {
        assert_eq!(format_time_saved(3_600_000), "1h");
    }

    #[test]
    fn formats_one_minute() {
        assert_eq!(format_time_saved(60_000), "1m");
    }

    #[test]
    fn formats_hours_and_minutes() {
        assert_eq!(format_time_saved(3_660_000), "1h 1m");
    }

    #[test]
    fn compact_layout_hides_descriptions_when_height_is_small() {
        assert!(!use_spacious_settings_layout(10, SettingKey::ALL.len(), 0));
    }

    #[test]
    fn spacious_layout_shows_descriptions_when_height_is_sufficient() {
        assert!(use_spacious_settings_layout(80, SettingKey::ALL.len(), 0));
    }

    #[test]
    fn library_item_capacity_avoids_rendering_half_items() {
        assert_eq!(visible_library_item_capacity(1), 0);
        assert_eq!(visible_library_item_capacity(2), 1);
        assert_eq!(visible_library_item_capacity(4), 1);
        assert_eq!(visible_library_item_capacity(5), 2);
    }

    #[test]
    fn settings_footer_includes_reset_without_changing_library_footer() {
        let mut app = App::default();
        app.handle_key(
            crossterm::event::KeyCode::Char('3'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(footer_text(&app).contains("r Reset"));
        assert!(footer_text(&app).contains("Ctrl+B Nav"));

        app.handle_key(
            crossterm::event::KeyCode::Char('2'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert_eq!(
            footer_text(&app),
            "Ctrl+B Nav   / Search   n New   i Import   x Export   d Delete   Enter Edit   q Quit"
        );
    }

    #[test]
    fn settings_confirmation_footer_preserves_specific_keys_with_nav_hint() {
        let mut app = App::default();
        app.handle_key(
            crossterm::event::KeyCode::Char('3'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.settings_page_mut()
            .handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('r'),
                crossterm::event::KeyModifiers::NONE,
            ));

        assert_eq!(
            footer_text(&app),
            "Ctrl+B Nav   ←/h Yes   →/l No   y Confirm   n/Esc Cancel"
        );
    }

    #[test]
    fn home_footer_keeps_scope_without_navigation_shortcuts() {
        let app = App::default();
        let footer = footer_text(&app);
        assert!(footer.contains("Ctrl+B Nav"));
        assert!(!footer.contains("1 Home"));
        assert!(!footer.contains("2 Library"));
        assert!(!footer.contains("3 Settings"));
    }

    #[test]
    fn library_footer_does_not_include_page_navigation_shortcuts() {
        let mut app = App::default();
        app.handle_key(
            crossterm::event::KeyCode::Char('2'),
            crossterm::event::KeyModifiers::NONE,
        );

        let footer = footer_text(&app);
        assert!(footer.contains("/ Search"));
        assert!(footer.contains("i Import"));
        assert!(footer.contains("x Export"));
        assert!(!footer.contains("1 Home"));
        assert!(!footer.contains("2 Library"));
        assert!(!footer.contains("3 Settings"));
    }

    #[test]
    fn library_modal_footer_switches_to_close_and_focus_hints() {
        let mut app = App::default();
        app.handle_key(
            crossterm::event::KeyCode::Char('2'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.library_page_mut()
            .open_editor_modal(sample_library_modal());

        assert_eq!(
            footer_text(&app),
            "Ctrl+B Nav   Ctrl+S Save   Esc Cancel   Tab Next   Shift+Tab Prev"
        );
    }

    #[test]
    fn library_export_modal_footer_switches_to_export_hints() {
        let mut app = App::default();
        app.handle_key(
            crossterm::event::KeyCode::Char('2'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.library_page_mut().open_export_modal();

        assert_eq!(footer_text(&app), "Ctrl+B Nav   ↑/↓ Move   Tab Next");
    }

    #[test]
    fn library_export_result_modal_footer_switches_to_close_hints() {
        let mut app = App::default();
        app.handle_key(
            crossterm::event::KeyCode::Char('2'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.library_page_mut().open_export_result_modal(
            &std::path::PathBuf::from("backup.tau"),
            false,
            false,
            false,
        );

        assert_eq!(footer_text(&app), "Ctrl+B Nav   Enter Close   Esc Close");
    }

    #[test]
    fn library_export_result_modal_title_is_exact() {
        assert_eq!(EXPORT_RESULT_MODAL_TITLE, "Export complete");
    }

    #[test]
    fn library_import_modal_footer_switches_to_import_hints() {
        let mut app = App::default();
        app.handle_key(
            crossterm::event::KeyCode::Char('2'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.library_page_mut().open_import_modal();

        assert_eq!(footer_text(&app), "Ctrl+B Nav   ↑/↓ Move   Tab Next");
    }

    #[test]
    fn library_import_result_modal_footer_switches_to_close_hints() {
        let mut app = App::default();
        app.handle_key(
            crossterm::event::KeyCode::Char('2'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.library_page_mut()
            .open_import_result_modal(&crate::library::LibraryImportOutcome::new(5, false, false));

        assert_eq!(footer_text(&app), "Ctrl+B Nav   Enter Close   Esc Close");
    }

    #[test]
    fn import_run_variables_warning_text_is_explicit_and_cautionary() {
        let warning = IMPORT_RUN_VARIABLES_WARNING_LINES.join(" ");

        assert!(warning.contains("CAUTION:"));
        assert!(warning.contains("[exec]"));
        assert!(warning.contains("shell commands"));
        assert!(warning.contains("Untrusted scripts can damage your system."));
        assert!(warning.contains("[y/N]"));
    }
}
