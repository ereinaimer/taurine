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

/// Visible tail of a search query for a `width`-wide field. The caret
/// always sits at the end of the query (no mid-text movement), so the
/// viewport is the last `width` characters and overlong input slides
/// left instead of clipping invisibly.
pub(crate) fn search_viewport(query: &str, width: u16) -> &str {
    let count = query.chars().count();
    let keep = (width as usize).min(count);
    let skip = count.saturating_sub(keep);
    match query.char_indices().nth(skip) {
        Some((byte, _)) => &query[byte..],
        None => query,
    }
}

/// Caret column for a char-index cursor inside a one-line field starting
/// at `x` with `width` cells. Stays inside the field so the terminal
/// caret (which honors the configured cursor style) is what the user sees.
pub(crate) fn caret_position(x: u16, y: u16, cursor: usize, width: u16) -> (u16, u16) {
    let column = (cursor as u16).min(width.saturating_sub(1));
    (x.saturating_add(column), y)
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
    const LIBRARY_ITEM_PADDING: u16 = 1;
    const ROW_HEIGHT: u16 = LIBRARY_ITEM_HEIGHT + 2 * LIBRARY_ITEM_PADDING;
    if available_height < LIBRARY_ITEM_HEIGHT {
        return 0;
    }
    usize::from(available_height / ROW_HEIGHT).max(1)
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
    frame.render_widget(Paragraph::new(value.to_string()).style(text_style), area);
    if focused && area.width > 0 && area.height > 0 {
        let (cx, cy) = caret_position(area.x, area.y, cursor, area.width);
        frame.set_cursor_position((cx, cy));
    }
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

    frame.render_widget(
        Paragraph::new(value.to_string()).style(value_style),
        sections[1],
    );
    if focused && !disabled && sections[1].width > 0 && sections[1].height > 0 {
        let (cx, cy) = caret_position(sections[1].x, sections[1].y, cursor, sections[1].width);
        frame.set_cursor_position((cx, cy));
    }
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

/// Shared bottom search box: rounded border, three lines total including
/// the border. No background fill. `placeholder` shows when the query is
/// empty and inactive. The caret-anchored viewport keeps typing visible.
pub(crate) fn render_search_block(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    field: &super::field::TextField,
    is_active: bool,
    placeholder: &str,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let border_style = Style::default().fg(theme.border);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let title_style = if is_active {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else if field.is_empty() {
        Style::default().fg(theme.description)
    } else {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    };
    // honey: the real terminal caret shows here so the configured cursor
    // style applies; no painted fake block.
    // honey: one cell of horizontal padding inside the border.
    let line_area = Rect {
        x: inner.x.saturating_add(1),
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: 1,
    };
    if line_area.width == 0 {
        return;
    }
    let (visible, caret) = field.window(line_area.width);
    let title = if is_active || !field.is_empty() {
        Line::from(visible.to_string())
    } else {
        Line::from(placeholder.to_string())
    };
    frame.render_widget(Paragraph::new(title).style(title_style), line_area);
    if is_active {
        let (cx, cy) = caret_position(line_area.x, line_area.y, caret, line_area.width);
        frame.set_cursor_position((cx, cy));
    }
}

#[cfg(test)]
mod tests {
    use super::search_viewport;

    #[test]
    fn viewport_shows_full_query_when_it_fits() {
        assert_eq!(search_viewport("gm", 37), "gm");
        assert_eq!(search_viewport("", 37), "");
    }

    #[test]
    fn viewport_slides_to_tail_when_overlong() {
        let query = "a".repeat(50);
        assert_eq!(search_viewport(&query, 37), "a".repeat(37));
    }

    #[test]
    fn viewport_never_splits_a_char() {
        let query = "héllo wörld";
        let visible = search_viewport(query, 5);
        assert_eq!(visible, "wörld");
        assert!(visible.is_char_boundary(0));
    }
}
