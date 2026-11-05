//! Rendering and hit-testing for the account gates.

use ratatui::{
    Frame,
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::home::account::{format_percent, format_reset_in, plan_label, quota_bar};
use crate::widgets::library::ButtonSelection;
use crate::widgets::util::{OVERLAY_BG, action_button_cells, centered_rect, render_action_buttons};

use super::{Gate, Phase, SignInState};

const SIGNED_OUT_WIDTH: u16 = 56;
const QUOTA_WIDTH: u16 = 60;
const QUOTA_TEXT_HEIGHT: u16 = 11;
const QUOTA_HEIGHT: u16 = QUOTA_TEXT_HEIGHT + 6;
const MIN_WIDTH: u16 = 44;
const MIN_HEIGHT: u16 = 12;

/// Full-screen sign-in door. Nothing is drawn behind it: the workspace
/// is not usable until an account exists.
pub(crate) fn render_signed_out(frame: &mut Frame, area: Rect, theme: &Theme, state: &SignInState) {
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );
    let lines = signed_out_lines(state, theme);
    debug_assert_eq!(
        lines.len() as u16,
        signed_out_text_height(&state.phase),
        "gate text height and layout must agree"
    );
    let Some((column, button_row)) = signed_out_geometry(area, &state.phase) else {
        return;
    };
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), column);
    if let Some(row) = button_row {
        render_action_buttons(
            frame,
            row,
            "Quit",
            "Sign in with Google",
            true,
            state.selected,
            theme,
        );
        render_hint(
            frame,
            Rect {
                y: row.y.saturating_add(2),
                ..row
            },
            "Enter select   q quit",
            theme,
        );
    }
}

/// Depleted-quota notice over the dimmed workspace. Flat fill, no border,
/// no header band: this is a page, not a dialog.
pub(crate) fn render_quota(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &super::QuotaState,
    account: &crate::widgets::home::account::AccountInfo,
    now_epoch: i64,
) {
    let Some((card, body)) = quota_geometry(area) else {
        return;
    };
    frame.render_widget(Clear, card);
    frame.render_widget(
        Block::default().style(Style::default().bg(OVERLAY_BG)),
        card,
    );
    let lines = quota_lines(account, now_epoch, body.width, theme);
    frame.render_widget(Paragraph::new(lines), body);
    let row = Rect {
        y: body.y.saturating_add(QUOTA_TEXT_HEIGHT).saturating_add(1),
        height: 1,
        ..body
    };
    render_action_buttons(frame, row, "Quit", "Continue", true, state.selected, theme);
    render_hint(
        frame,
        Rect {
            y: row.y.saturating_add(2),
            ..body
        },
        "Enter select   q quit",
        theme,
    );
}

/// Button under a pointer, shared by both gates.
pub(crate) fn hit(area: Rect, gate: &Gate, column: u16, row: u16) -> Option<ButtonSelection> {
    let (button_row, cancel, confirm) = match gate {
        Gate::SignedOut(state) => {
            let (_, button_row) = signed_out_geometry(area, &state.phase)?;
            (button_row?, "Quit", "Sign in with Google")
        }
        Gate::Quota(_) => {
            let (_, body) = quota_geometry(area)?;
            (
                Rect {
                    y: body.y.saturating_add(QUOTA_TEXT_HEIGHT).saturating_add(1),
                    height: 1,
                    ..body
                },
                "Quit",
                "Continue",
            )
        }
    };
    let (cancel_cell, confirm_cell) = action_button_cells(button_row, cancel, confirm);
    if contains(cancel_cell, column, row) {
        Some(ButtonSelection::Cancel)
    } else if contains(confirm_cell, column, row) {
        Some(ButtonSelection::Confirm)
    } else {
        None
    }
}

fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn signed_out_text_height(phase: &Phase) -> u16 {
    match phase {
        Phase::Idle => 6,
        Phase::Waiting { .. } => 10,
        Phase::Failed(_) => 8,
    }
}

fn signed_out_geometry(area: Rect, phase: &Phase) -> Option<(Rect, Option<Rect>)> {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return None;
    }
    let text_height = signed_out_text_height(phase);
    let has_buttons = !matches!(phase, Phase::Waiting { .. });
    let height = text_height + if has_buttons { 4 } else { 0 };
    if area.height < height {
        return None;
    }
    let width = SIGNED_OUT_WIDTH.min(area.width);
    let column = centered_rect(width, height, area);
    let button_row = has_buttons.then(|| Rect {
        y: column.y.saturating_add(text_height).saturating_add(1),
        height: 1,
        ..column
    });
    Some((column, button_row))
}

fn quota_geometry(area: Rect) -> Option<(Rect, Rect)> {
    if area.width < MIN_WIDTH || area.height < QUOTA_HEIGHT {
        return None;
    }
    let width = QUOTA_WIDTH.min(area.width.saturating_sub(4));
    let card = centered_rect(width, QUOTA_HEIGHT, area);
    let body = card.inner(Margin::new(3, 1));
    Some((card, body))
}

fn signed_out_lines(state: &SignInState, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            "T A U R I N E",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            "Sign in to Taurine",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            "An account keeps your library in sync",
            Style::default().fg(theme.text_muted),
        )),
        Line::from(Span::styled(
            "and tracks weekly usage.",
            Style::default().fg(theme.text_muted),
        )),
    ];
    match &state.phase {
        Phase::Idle => {}
        Phase::Waiting { started } => {
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::styled(spinner(started), Style::default().fg(theme.primary)),
                Span::styled(
                    " Waiting for your browser…",
                    Style::default().fg(theme.text),
                ),
            ]));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "Esc cancel",
                Style::default().fg(theme.description),
            )));
        }
        Phase::Failed(error) => {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                error.clone(),
                Style::default().fg(theme.warning),
            )));
        }
    }
    lines
}

fn quota_lines(
    account: &crate::widgets::home::account::AccountInfo,
    now_epoch: i64,
    width: u16,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let remaining = account.quota.map(|q| q.remaining_percentage).unwrap_or(0.0);
    let reset = account
        .quota
        .map(|q| format_reset_in(q.reset_epoch.saturating_sub(now_epoch)))
        .unwrap_or_else(|| "next week".to_string());
    vec![
        Line::default(),
        Line::from(Span::styled(
            plan_label(account.tier).to_uppercase(),
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            "You've used this week's quota",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            format_percent(remaining),
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )),
        quota_bar(remaining, width as usize, theme),
        Line::from(Span::styled(
            format!("Resets in {reset}"),
            Style::default().fg(theme.description),
        )),
        Line::default(),
        Line::from(Span::styled(
            "Expansions and dictation are paused until",
            Style::default().fg(theme.text_muted),
        )),
        Line::from(Span::styled(
            "the quota resets. You can still edit your library.",
            Style::default().fg(theme.text_muted),
        )),
    ]
}

fn render_hint(frame: &mut Frame, area: Rect, text: &str, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(text).alignment(Alignment::Center).style(
            Style::default()
                .fg(theme.description)
                .add_modifier(Modifier::DIM),
        ),
        area,
    );
}

fn spinner(started: &std::time::Instant) -> &'static str {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    let frame = (started.elapsed().as_millis() / 100) as usize % FRAMES.len();
    FRAMES[frame]
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::theme::default::DARK_THEME;
    use crate::widgets::home::account::{AccountInfo, QuotaWindow};

    fn account() -> AccountInfo {
        AccountInfo {
            authenticated: true,
            email: Some("user@example.com".to_string()),
            tier: taurine_core::db::crud::UserTier::Free,
            quota: Some(QuotaWindow {
                remaining_percentage: 0.0,
                reset_epoch: 5 * 86_400 + 11 * 3_600,
            }),
            device: "DESKTOP-TAURINE".to_string(),
        }
    }

    fn draw(gate: &Gate, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| match gate {
                Gate::SignedOut(state) => render_signed_out(frame, area, &DARK_THEME, state),
                Gate::Quota(state) => {
                    render_quota(frame, area, &DARK_THEME, state, &account(), 0);
                }
            })
            .expect("test draw");
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..height {
            for x in 0..width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn signed_out_shows_google_and_quit_buttons_without_a_cursor_glyph() {
        let text = draw(&Gate::signed_out(), 80, 24);
        assert!(text.contains("T A U R I N E"));
        assert!(text.contains("Sign in to Taurine"));
        assert!(text.contains("Sign in with Google"));
        assert!(text.contains("Quit"));
        assert!(!text.contains('>'), "gates use buttons, not list rows");
    }

    #[test]
    fn quota_notice_shows_plan_reset_and_buttons() {
        let text = draw(&Gate::quota(5 * 86_400 + 11 * 3_600), 80, 24);
        assert!(text.contains("FREE PLAN"));
        assert!(text.contains("You've used this week's quota"));
        assert!(text.contains("0% left"));
        assert!(text.contains("Resets in 5d 11h"));
        assert!(text.contains("You can still edit your library"));
        assert!(text.contains("Continue"));
        assert!(text.contains("Quit"));
    }

    #[test]
    fn quota_hit_test_matches_the_rendered_button_row() {
        let gate = Gate::quota(1000);
        let area = Rect::new(0, 0, 80, 24);
        let (_, body) = quota_geometry(area).expect("gate geometry");
        let row = body.y + QUOTA_TEXT_HEIGHT + 1;
        let (cancel, confirm) = action_button_cells(
            Rect {
                y: row,
                height: 1,
                ..body
            },
            "Quit",
            "Continue",
        );
        assert_eq!(
            hit(area, &gate, cancel.x + 1, row),
            Some(ButtonSelection::Cancel)
        );
        assert_eq!(
            hit(area, &gate, confirm.x + 1, row),
            Some(ButtonSelection::Confirm)
        );
    }

    #[test]
    fn selected_button_uses_the_shared_active_fill() {
        let gate = Gate::quota(1000);
        let area = Rect::new(0, 0, 80, 24);
        let (_, body) = quota_geometry(area).expect("gate geometry");
        let row = body.y + QUOTA_TEXT_HEIGHT + 1;
        let (_, confirm) = action_button_cells(
            Rect {
                y: row,
                height: 1,
                ..body
            },
            "Quit",
            "Continue",
        );
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                let Gate::Quota(state) = &gate else {
                    return;
                };
                render_quota(frame, area, &DARK_THEME, state, &account(), 0);
            })
            .expect("test draw");
        let cell = &terminal.backend().buffer()[(confirm.x + 1, confirm.y)];
        assert_eq!(cell.bg, DARK_THEME.button.active_bg);
    }

    #[test]
    fn signed_out_hit_test_matches_the_rendered_button_row() {
        let gate = Gate::signed_out();
        let area = Rect::new(0, 0, 80, 24);
        let Gate::SignedOut(state) = &gate else {
            panic!("expected signed-out gate");
        };
        let (_, row) = signed_out_geometry(area, &state.phase).expect("gate geometry");
        let row = row.expect("idle gate shows buttons");
        let (cancel, confirm) = action_button_cells(row, "Quit", "Sign in with Google");
        assert_eq!(
            hit(area, &gate, cancel.x + 1, row.y),
            Some(ButtonSelection::Cancel)
        );
        assert_eq!(
            hit(area, &gate, confirm.x + 1, row.y),
            Some(ButtonSelection::Confirm)
        );
    }
}
