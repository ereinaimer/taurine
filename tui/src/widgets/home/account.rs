//! Home's right pane: the signed-in account and this week's quota.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use taurine_core::db::crud::UserTier;

use crate::theme::Theme;
use crate::widgets::wrap::wrap_line;

/// Horizontal breathing room inside the pane.
const PANE_PAD_X: u16 = 2;

pub(crate) const DEFAULT_RATIO: f32 = 0.3;
pub(crate) const MIN_RATIO: f32 = 0.0;
pub(crate) const MAX_RATIO: f32 = 2.0;

const WEEK_SECONDS: i64 = 7 * 86_400;

/// Snapshot of the account and the current quota window. Deliberately
/// carries no account identifier: only what the pane is allowed to show.
#[derive(Debug, Clone, Default)]
pub(crate) struct AccountInfo {
    pub(crate) authenticated: bool,
    pub(crate) email: Option<String>,
    pub(crate) tier: UserTier,
    pub(crate) quota: Option<QuotaWindow>,
    pub(crate) device: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct QuotaWindow {
    pub(crate) remaining_percentage: f64,
    pub(crate) reset_epoch: i64,
}

impl QuotaWindow {
    pub(crate) fn from_row(row: &taurine_core::db::crud::QuotaLedgerRow) -> Self {
        Self {
            remaining_percentage: row.remaining_percentage,
            reset_epoch: row.week_start_epoch.saturating_add(WEEK_SECONDS),
        }
    }
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    account: &AccountInfo,
    now_epoch: i64,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let content = Rect {
        x: area.x.saturating_add(PANE_PAD_X),
        y: area.y,
        width: area.width.saturating_sub(PANE_PAD_X.saturating_mul(2)),
        ..area
    };
    if content.width == 0 {
        return;
    }
    let width = content.width as usize;
    let mut lines = Vec::new();

    lines.push(label("Account", theme));
    let identity = if account.authenticated {
        account.email.as_deref().unwrap_or("Signed in")
    } else {
        "Not signed in"
    };
    push_wrapped(&mut lines, identity, width, Style::default().fg(theme.text));
    lines.push(value(plan_label(account.tier), theme));

    let device = account.device.trim();
    if !device.is_empty() {
        lines.push(Line::default());
        lines.push(label("Device", theme));
        push_wrapped(&mut lines, device, width, Style::default().fg(theme.text));
    }

    lines.push(Line::default());
    lines.push(label("Weekly Quota", theme));
    if account.tier.is_unlimited() {
        lines.push(value("Unlimited", theme));
    } else if let Some(quota) = account.quota {
        lines.push(Line::from(Span::styled(
            format_percent(quota.remaining_percentage),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )));
        lines.push(quota_bar(quota.remaining_percentage, width, theme));
        lines.push(Line::from(Span::styled(
            format!(
                "Resets in {}",
                format_reset_in(quota.reset_epoch - now_epoch)
            ),
            Style::default().fg(theme.description),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "No quota data",
            Style::default().fg(theme.description),
        )));
    }

    frame.render_widget(Paragraph::new(lines), content);
}

fn label(text: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        text.to_uppercase(),
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::BOLD),
    ))
}

fn value(text: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(theme.text),
    ))
}

fn push_wrapped(lines: &mut Vec<Line<'static>>, text: &str, width: usize, style: Style) {
    for (row, _) in wrap_line(text, width, width) {
        lines.push(Line::from(Span::styled(row, style)));
    }
}

fn plan_label(tier: UserTier) -> &'static str {
    match tier {
        UserTier::Free => "Free plan",
        UserTier::Pro => "Pro plan",
        UserTier::Max => "Max plan",
        UserTier::Team => "Team plan",
    }
}

/// Weekly quota left, from one decimal down to a whole percent.
pub(crate) fn format_percent(remaining: f64) -> String {
    let rounded = format!("{:.1}", remaining.clamp(0.0, 100.0));
    let trimmed = rounded.strip_suffix(".0").unwrap_or(&rounded);
    format!("{trimmed}% left")
}

/// Countdown to the weekly rollover: `5d 11h`, `11h 5m`, `5m`.
pub(crate) fn format_reset_in(seconds: i64) -> String {
    if seconds <= 0 {
        return "now".to_string();
    }
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "under 1m".to_string()
    }
}

fn quota_bar(remaining: f64, width: usize, theme: &Theme) -> Line<'static> {
    let filled = ((remaining.clamp(0.0, 100.0) / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    Line::from(vec![
        Span::styled("█".repeat(filled), Style::default().fg(theme.text_muted)),
        Span::styled(
            "░".repeat(width.saturating_sub(filled)),
            Style::default().fg(theme.track),
        ),
    ])
}

pub(crate) fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::theme::default::DARK_THEME;

    fn account() -> AccountInfo {
        AccountInfo {
            authenticated: true,
            email: Some("user@example.com".to_string()),
            tier: UserTier::Free,
            quota: Some(QuotaWindow {
                remaining_percentage: 74.5,
                reset_epoch: 5 * 86_400 + 11 * 3_600,
            }),
            device: "DESKTOP-TAURINE".to_string(),
        }
    }

    fn draw(account: &AccountInfo, width: u16, height: u16, now: i64) -> String {
        let area = Rect::new(0, 0, width, height);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, area, &DARK_THEME, account, now))
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
    fn reset_countdown_reads_days_hours_and_minutes() {
        assert_eq!(format_reset_in(5 * 86_400 + 11 * 3_600), "5d 11h");
        assert_eq!(format_reset_in(11 * 3_600 + 5 * 60), "11h 5m");
        assert_eq!(format_reset_in(5 * 60), "5m");
        assert_eq!(format_reset_in(30), "under 1m");
        assert_eq!(format_reset_in(0), "now");
        assert_eq!(format_reset_in(-1), "now");
    }

    #[test]
    fn quota_percent_trims_the_trailing_zero() {
        assert_eq!(format_percent(74.5), "74.5% left");
        assert_eq!(format_percent(100.0), "100% left");
        assert_eq!(format_percent(0.0), "0% left");
        assert_eq!(format_percent(120.0), "100% left");
    }

    #[test]
    fn signed_in_pane_shows_email_plan_and_quota() {
        let text = draw(&account(), 40, 20, 0);

        assert!(text.contains("ACCOUNT"));
        assert!(text.contains("user@example.com"));
        assert!(text.contains("Free plan"));
        assert!(text.contains("WEEKLY QUOTA"));
        assert!(text.contains("74.5% left"), "{text}");
        assert!(text.contains("Resets in 5d 11h"));
        assert!(text.contains("█"));
        assert!(text.contains("DESKTOP"));
        assert!(!text.contains("Pro plan"));
    }

    #[test]
    fn signed_in_without_an_email_never_shows_an_account_id() {
        let mut account = account();
        account.email = None;

        let text = draw(&account, 40, 12, 0);

        assert!(text.contains("Signed in"));
        assert!(!text.contains("Not signed in"));
        // There is no account id in the model at all; guard the shape.
        assert!(!text.contains("user-id"));
        assert!(!text.contains("0000"));
    }

    #[test]
    fn signed_out_pane_asks_for_sign_in() {
        let mut account = account();
        account.authenticated = false;
        account.email = None;
        account.tier = UserTier::Free;
        account.quota = Some(QuotaWindow {
            remaining_percentage: 100.0,
            reset_epoch: 86_400,
        });

        let text = draw(&account, 40, 20, 0);

        assert!(text.contains("Not signed in"));
        assert!(text.contains("Free plan"));
        assert!(text.contains("100% left"));
        assert!(text.contains("Resets in 1d 0h"));
    }

    #[test]
    fn unlimited_plans_show_no_percentage() {
        let mut account = account();
        account.tier = UserTier::Max;

        let text = draw(&account, 40, 20, 0);

        assert!(text.contains("Max plan"));
        assert!(text.contains("Unlimited"));
        assert!(!text.contains("% left"));
        assert!(!text.contains("Resets in"));
        assert_eq!(plan_label(UserTier::Pro), "Pro plan");
        assert_eq!(plan_label(UserTier::Team), "Team plan");
    }
}
