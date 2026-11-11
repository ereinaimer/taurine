use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::theme::Theme;
use crate::widgets::settings::state::{
    ConfirmResetModalState, HotkeyCaptureModalState, InputModalState, SelectModalState,
    SettingKeyMeta, SettingsModal,
};
use crate::widgets::util;

pub fn render_settings_modal(frame: &mut Frame, area: Rect, theme: &Theme, modal: &SettingsModal) {
    match modal {
        SettingsModal::Input(state) => render_input_modal(frame, area, theme, state),
        SettingsModal::Select(state) => render_select_modal(frame, area, theme, state),
        SettingsModal::ConfirmReset(state) => render_confirm_reset_modal(frame, area, theme, state),
        SettingsModal::HotkeyCapture(state) => {
            render_hotkey_capture_modal(frame, area, theme, state)
        }
    }
}

/// Footer for the editor modals: the single source both renderers and
/// the click branch read, so cells land on the labels as drawn. None
/// for modals without a confirm/cancel band.
const SAVE_FOOTER: util::OverlayFooter = util::OverlayFooter {
    confirm_label: "Save",
    enter_hint: true,
    hot: None,
    danger: false,
};

pub(crate) fn modal_footer(modal: &SettingsModal) -> Option<util::OverlayFooter> {
    match modal {
        SettingsModal::Input(_) | SettingsModal::HotkeyCapture(_) => Some(SAVE_FOOTER),
        SettingsModal::Select(_) | SettingsModal::ConfirmReset(_) => None,
    }
}

/// Flat popup shell shared by the editor modals: the shared header
/// band and gap, then content. Mirrors the library overlays. Returns
/// the content rect below the gap, or None when too small. A footer
/// builds the chrome with the bottom band and trims content above it.
fn overlay_shell(
    frame: &mut Frame,
    area: Rect,
    width: u16,
    height: u16,
    title: &str,
    theme: &Theme,
    footer: Option<&util::OverlayFooter>,
) -> Option<Rect> {
    let popup = util::centered_rect(width, height, area);
    let chrome = match footer {
        Some(footer) => {
            let chrome = util::OverlayChrome::with_footer(popup);
            chrome.render_with_footer(frame, theme, title, footer);
            chrome
        }
        None => {
            let chrome = util::OverlayChrome::new(popup);
            chrome.render(frame, theme, title);
            chrome
        }
    };
    if chrome.body.width == 0 || chrome.content.height == 0 {
        return None;
    }
    Some(chrome.content)
}

/// Popup rects shared by rendering and the overlay open-transition so
/// the sweep covers exactly what was painted.
pub(crate) fn input_popup(area: Rect) -> Rect {
    let width = if area.width > 32 {
        area.width.saturating_sub(4).min(64)
    } else {
        area.width.max(1)
    };
    let height = if area.height >= 12 {
        12
    } else {
        area.height.max(1)
    };
    util::centered_rect(width, height, area)
}

pub(crate) fn confirm_popup(area: Rect) -> Rect {
    let width = if area.width > 44 {
        area.width.saturating_sub(4).min(64)
    } else {
        area.width.max(1)
    };
    let height = if area.height >= 9 {
        9
    } else {
        area.height.max(1)
    };
    util::centered_rect(width, height, area)
}

pub(crate) fn hotkey_popup(area: Rect) -> Rect {
    let content_width = if area.width > 32 {
        area.width.saturating_sub(4).min(64)
    } else {
        area.width.max(1)
    };
    util::centered_rect(content_width.saturating_add(4), 5 + 8, area)
}

fn render_input_modal(frame: &mut Frame, area: Rect, theme: &Theme, state: &InputModalState) {
    let popup = input_popup(area);
    let Some(content) = overlay_shell(
        frame,
        area,
        popup.width,
        popup.height,
        state.key().display_name(),
        theme,
        Some(&SAVE_FOOTER),
    ) else {
        return;
    };

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(content);

    frame.render_widget(
        Paragraph::new(state.key().description()).style(Style::default().fg(theme.description)),
        sections[0],
    );
    let (visible, caret) = state.field().window(sections[1].width);
    frame.render_widget(
        Paragraph::new(visible.to_string())
            .style(Style::default().fg(theme.text).bg(theme.surface)),
        sections[1],
    );
    if sections[1].width > 0 && sections[1].height > 0 && !crate::overlay_fx::is_cursor_suppressed()
    {
        let (cx, cy) = util::caret_position(sections[1].x, sections[1].y, caret, sections[1].width);
        frame.set_cursor_position((cx, cy));
    }

    // honey: the feedback row is error-only; the save/cancel hints live
    // in the footer band.
    if let Some(error) = state.error() {
        frame.render_widget(
            Paragraph::new(error).style(
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            sections[2],
        );
    }
}

fn render_hotkey_capture_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &HotkeyCaptureModalState,
) {
    let popup = hotkey_popup(area);
    let Some(content) = overlay_shell(
        frame,
        area,
        popup.width,
        popup.height,
        state.key().display_name(),
        theme,
        Some(&SAVE_FOOTER),
    ) else {
        return;
    };

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(content);

    frame.render_widget(
        Paragraph::new(state.key().description()).style(Style::default().fg(theme.description)),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(format!("Current: {}", state.current())).style(
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        sections[1],
    );

    let capture_line = match state.captured() {
        Some(captured) => Span::styled(
            captured.to_string(),
            Style::default()
                .fg(theme.text)
                .bg(theme.surface)
                .add_modifier(Modifier::BOLD),
        ),
        None => Span::styled(
            state.preview().unwrap_or("Press keys…").to_string(),
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        ),
    };
    frame.render_widget(Paragraph::new(Line::from(capture_line)), sections[2]);

    if let Some(error) = state.error() {
        frame.render_widget(
            Paragraph::new(error).style(
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            sections[3],
        );
    }
}

fn render_select_modal(frame: &mut Frame, area: Rect, theme: &Theme, state: &SelectModalState) {
    // honey: options render through the shared overlay list; errors ride
    // the bottom row like the tags builder.
    let entries: Vec<util::OverlayEntry> = state
        .options()
        .iter()
        .zip(state.details())
        .map(|(option, detail)| util::OverlayEntry {
            label: option.clone(),
            detail: detail.clone(),
            icon: String::new(),
        })
        .collect();
    util::render_overlay_select(
        frame,
        area,
        theme,
        state.key().display_name(),
        &entries,
        state.selected_index(),
    );
    if let Some(error) = state.error() {
        let popup = util::overlay_popup(area);
        let body = util::overlay_body(popup);
        if body.width == 0 || body.height == 0 {
            return;
        }
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

fn render_confirm_reset_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &ConfirmResetModalState,
) {
    let popup = confirm_popup(area);
    let Some(content) = overlay_shell(
        frame,
        area,
        popup.width,
        popup.height,
        "Reset Setting",
        theme,
        None,
    ) else {
        return;
    };

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(content);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::settings::state::SettingKey;
    use ratatui::{Terminal, backend::TestBackend};

    fn rendered_rows(modal: &SettingsModal) -> Vec<String> {
        let theme = &crate::theme::default::DARK_THEME;
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_settings_modal(frame, frame.area(), theme, modal);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..30)
            .map(|y| {
                (0..100)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn rendered(modal: &SettingsModal) -> String {
        rendered_rows(modal).join("\n")
    }

    #[test]
    fn input_popup_grows_for_footer_band() {
        let area = Rect::new(0, 0, 100, 30);
        assert_eq!(input_popup(area).height, 12);
        // clamps below the guard so tiny terminals still show the modal
        assert_eq!(input_popup(Rect::new(0, 0, 100, 11)).height, 11);
    }

    #[test]
    fn hotkey_popup_grows_for_footer_band() {
        assert_eq!(hotkey_popup(Rect::new(0, 0, 100, 30)).height, 13);
    }

    #[test]
    fn input_modal_footer_carries_save_and_cancel() {
        let modal = SettingsModal::Input(InputModalState::new(SettingKey::Wpm, "120".to_string()));
        let popup = input_popup(Rect::new(0, 0, 100, 30));
        let rows = rendered_rows(&modal);
        let footer_row = &rows[(popup.y + popup.height - 2) as usize];
        assert!(footer_row.trim_start().starts_with("Esc Cancel"));
        assert!(footer_row.trim_end().ends_with("Enter Save"));
        // honey: the feedback row is error-only; the hint text lives in
        // the footer. Row = body.y + header rows + description + field.
        let feedback_row = &rows[(popup.y + 1 + 3 + 2 + 1) as usize];
        assert!(feedback_row.trim().is_empty());
        assert!(!rendered(&modal).contains("Enter Save   Esc Cancel"));
        assert!(!rendered(&modal).contains('╭'));
    }

    #[test]
    fn hotkey_capture_modal_footer_carries_save_and_cancel() {
        let modal = SettingsModal::HotkeyCapture(HotkeyCaptureModalState::new(
            SettingKey::PauseHotkey,
            "Alt + `".to_string(),
        ));
        let popup = hotkey_popup(Rect::new(0, 0, 100, 30));
        let rows = rendered_rows(&modal);
        let footer_row = &rows[(popup.y + popup.height - 2) as usize];
        assert!(footer_row.trim_start().starts_with("Esc Cancel"));
        assert!(footer_row.trim_end().ends_with("Enter Save"));
        assert!(!rendered(&modal).contains('╭'));
    }

    #[test]
    fn modal_footer_targets_only_editor_modals() {
        let save = util::OverlayFooter {
            confirm_label: "Save",
            enter_hint: true,
            hot: None,
            danger: false,
        };
        assert_eq!(
            modal_footer(&SettingsModal::Input(InputModalState::new(
                SettingKey::Wpm,
                "120".to_string()
            ))),
            Some(save)
        );
        assert_eq!(
            modal_footer(&SettingsModal::HotkeyCapture(HotkeyCaptureModalState::new(
                SettingKey::PauseHotkey,
                "Alt + `".to_string()
            ))),
            Some(save)
        );
        assert_eq!(
            modal_footer(&SettingsModal::Select(SelectModalState::new(
                SettingKey::SpinnerStyle,
                vec!["classic".to_string()],
                "classic".to_string()
            ))),
            None
        );
        assert_eq!(
            modal_footer(&SettingsModal::ConfirmReset(ConfirmResetModalState::new(
                SettingKey::Wpm
            ))),
            None
        );
    }

    #[test]
    fn input_modal_uses_flat_overlay_chrome() {
        let modal = SettingsModal::Input(InputModalState::new(SettingKey::Wpm, "120".to_string()));
        let screen = rendered(&modal);
        assert!(screen.contains("Words Per Minute"));
        assert!(screen.contains("Esc"));
        assert!(screen.contains("Enter Save"));
        // honey: shared flat chrome carries no border glyphs.
        assert!(!screen.contains('╭'));
    }

    #[test]
    fn select_modal_renders_through_shared_overlay_list() {
        let modal = SettingsModal::Select(SelectModalState::new(
            SettingKey::SpinnerStyle,
            vec![
                "classic".to_string(),
                "braille".to_string(),
                "arc".to_string(),
            ],
            "classic".to_string(),
        ));
        let screen = rendered(&modal);
        assert!(screen.contains("Spinner Style"));
        assert!(screen.contains("braille"));
        // honey: shared flat chrome carries no border glyphs.
        assert!(!screen.contains('╭'));
    }

    #[test]
    fn confirm_reset_modal_uses_flat_overlay_chrome() {
        let modal = SettingsModal::ConfirmReset(ConfirmResetModalState::new(SettingKey::Wpm));
        let screen = rendered(&modal);
        assert!(screen.contains("Reset Setting"));
        assert!(screen.contains("Yes"));
        assert!(screen.contains("No"));
        // honey: shared flat chrome carries no border glyphs.
        assert!(!screen.contains('╭'));
    }

    #[test]
    fn hotkey_capture_modal_uses_flat_overlay_chrome() {
        let modal = SettingsModal::HotkeyCapture(HotkeyCaptureModalState::new(
            SettingKey::PauseHotkey,
            "Alt + `".to_string(),
        ));
        let screen = rendered(&modal);
        assert!(screen.contains("Pause Hotkey"));
        assert!(screen.contains("Current:"));
        // honey: shared flat chrome carries no border glyphs.
        assert!(!screen.contains('╭'));
    }
}
