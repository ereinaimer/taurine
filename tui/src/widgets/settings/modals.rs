use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
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

/// Flat popup shell shared by the editor modals: borderless fill,
/// title row, then one blank line. Mirrors the library overlays.
/// Returns the content rect below the title, or None when too small.
fn overlay_shell(
    frame: &mut Frame,
    area: Rect,
    width: u16,
    height: u16,
    title: &str,
    theme: &Theme,
) -> Option<Rect> {
    use ratatui::{style::Color::Rgb, widgets::Block};

    let popup = util::centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(Rgb(0x14, 0x14, 0x14))),
        popup,
    );
    let body = popup.inner(Margin::new(3, 1));
    if body.width == 0 || body.height < 2 {
        return None;
    }
    util::render_overlay_title(frame, body, theme, title);
    Some(Rect {
        x: body.x,
        y: body.y.saturating_add(2),
        width: body.width,
        height: body.height.saturating_sub(2),
    })
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
    let Some(content) = overlay_shell(
        frame,
        area,
        width,
        height,
        state.key().display_name(),
        theme,
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
    if sections[1].width > 0 && sections[1].height > 0 {
        let (cx, cy) = util::caret_position(sections[1].x, sections[1].y, caret, sections[1].width);
        frame.set_cursor_position((cx, cy));
    }

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

fn render_hotkey_capture_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &HotkeyCaptureModalState,
) {
    let content_width = if area.width > 32 {
        area.width.saturating_sub(4).min(64)
    } else {
        area.width.max(1)
    };
    let Some(content) = overlay_shell(
        frame,
        area,
        content_width.saturating_add(4),
        5 + 4,
        state.key().display_name(),
        theme,
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
        .map(|option| util::OverlayEntry {
            label: option.clone(),
            detail: String::new(),
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
    let Some(content) = overlay_shell(frame, area, width, height, "Reset Setting", theme) else {
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

    fn rendered(modal: &SettingsModal) -> String {
        let theme = &crate::theme::builtin::DARK_THEME;
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
            .collect::<Vec<_>>()
            .join("\n")
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
