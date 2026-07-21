use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::library::state::ButtonSelection;

pub(crate) fn truncate_to_width(value: &str, max_chars: u16) -> String {
    if value.chars().count() <= max_chars as usize {
        value.to_string()
    } else {
        format!(
            "{}…",
            value
                .chars()
                .take(max_chars.saturating_sub(1) as usize)
                .collect::<String>()
        )
    }
}

pub(crate) fn format_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 10_000 {
        format!("{}k", value / 1_000)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

pub(crate) fn format_time_saved(time_saved_ms: u64) -> String {
    let total_seconds = time_saved_ms / 1000;
    if total_seconds >= 3600 {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        format!("{hours}h {minutes}m")
    } else if total_seconds >= 60 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{minutes}m {seconds}s")
    } else {
        format!("{total_seconds}s")
    }
}

pub(crate) fn input_cursor_line(value: &str, cursor: usize) -> Line<'static> {
    let before = value.chars().take(cursor).collect::<String>();
    let at = value.chars().nth(cursor);
    let after = value
        .chars()
        .skip(cursor.saturating_add(1))
        .collect::<String>();

    let mut spans = vec![Span::raw(before)];
    if let Some(c) = at {
        spans.push(Span::styled(
            c.to_string(),
            Style::default().add_modifier(Modifier::REVERSED),
        ));
    } else {
        spans.push(Span::styled(
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        ));
    }
    spans.push(Span::raw(after));
    Line::from(spans)
}

pub(crate) fn yes_no_label(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

pub(crate) fn visible_range(total: usize, selected: usize, visible_count: usize) -> (usize, usize) {
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

pub(crate) fn visible_library_item_capacity(available_height: u16) -> usize {
    const LIBRARY_ITEM_HEIGHT: u16 = 2;
    const LIBRARY_ITEM_GAP: u16 = 1;
    if available_height < LIBRARY_ITEM_HEIGHT {
        return 0;
    }
    usize::from((available_height + LIBRARY_ITEM_GAP) / (LIBRARY_ITEM_HEIGHT + LIBRARY_ITEM_GAP))
}

pub(crate) fn render_modal_block(
    frame: &mut Frame,
    popup: Rect,
    title: &str,
    theme: &Theme,
) -> Rect {
    let block = Block::default()
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(Style::default().fg(theme.border));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    inner
}

pub(crate) fn render_action_buttons(
    frame: &mut Frame,
    area: Rect,
    cancel_label: &str,
    confirm_label: &str,
    is_focused: bool,
    selection: ButtonSelection,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
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
            .fg(theme.text)
            .bg(theme.button.active_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(theme.surface)
    };
    frame.render_widget(
        Paragraph::new(cancel_text).style(cancel_style),
        btn_layout[1],
    );

    let confirm_style = if is_focused && selection == ButtonSelection::Confirm {
        Style::default()
            .fg(theme.text)
            .bg(theme.button.active_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(theme.surface)
    };
    frame.render_widget(
        Paragraph::new(confirm_text).style(confirm_style),
        btn_layout[3],
    );
}

pub(crate) fn render_modal_field_label(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    focused: bool,
    indicator: Option<String>,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let indicator_width = indicator
        .as_ref()
        .map(|value| value.chars().count() as u16)
        .unwrap_or_default();
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(indicator_width)])
        .split(area);

    let label_style = if focused {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };
    frame.render_widget(Paragraph::new(label).style(label_style), sections[0]);

    if let Some(indicator) = indicator {
        frame.render_widget(
            Paragraph::new(indicator)
                .alignment(ratatui::layout::Alignment::Right)
                .style(
                    Style::default()
                        .fg(theme.text_muted)
                        .add_modifier(Modifier::DIM),
                ),
            sections[1],
        );
    }
}

pub(crate) fn render_modal_input_field(
    frame: &mut Frame,
    area: Rect,
    value: &str,
    cursor: usize,
    focused: bool,
    theme: &Theme,
) {
    let bg = if focused {
        theme.surface
    } else {
        theme.background
    };
    let text_style = if focused {
        Style::default()
            .fg(theme.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(bg)
    };

    let block = Block::default().style(Style::default().bg(bg));
    frame.render_widget(block, area);
    let text = if focused {
        Paragraph::new(input_cursor_line(value, cursor))
    } else {
        Paragraph::new(value.to_string())
    };
    frame.render_widget(text.style(text_style), area);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_modal_password_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    cursor: usize,
    focused: bool,
    disabled: bool,
    red_asterisk: bool,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let label_width = area.width.min(12);
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(label_width), Constraint::Min(0)])
        .split(area);

    let (label_style, value_style) = if disabled {
        let dimmed = Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM);
        (dimmed, dimmed)
    } else if focused {
        (
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(theme.text),
            Style::default().fg(theme.text),
        )
    };

    let label_line = if red_asterisk {
        Line::from(vec![
            Span::styled(label, label_style),
            Span::styled("*", Style::default().fg(theme.error)),
        ])
    } else {
        Line::from(vec![Span::styled(label, label_style)])
    };
    frame.render_widget(Paragraph::new(label_line), sections[0]);

    let text = Paragraph::new(input_cursor_line(value, cursor));
    frame.render_widget(text.style(value_style), sections[1]);
}

pub(crate) fn render_modal_key_value_row(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    quiet: bool,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let bg = if focused {
        theme.surface
    } else {
        ratatui::style::Color::Reset
    };
    frame.render_widget(Block::default().style(Style::default().bg(bg)), area);

    let label_width = area.width.min(12);
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(label_width), Constraint::Min(0)])
        .split(area);

    let label_style = if focused {
        Style::default()
            .fg(theme.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else if quiet {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let value_style = if focused {
        Style::default().fg(theme.text).bg(bg)
    } else if quiet {
        Style::default()
            .fg(theme.text_muted)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(theme.text)
    };

    frame.render_widget(Paragraph::new(label).style(label_style), sections[0]);
    frame.render_widget(
        Paragraph::new(truncate_to_width(value, sections[1].width)).style(value_style),
        sections[1],
    );
}
