use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;

/// Maximum wrapped description lines shown under a title.
pub(crate) const MAX_DESCRIPTION_LINES: usize = 3;

/// Inner content of a row: one cell of padding on every side.
pub(crate) fn content(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

/// Label column and right control column inside a row.
pub(crate) fn sections(area: Rect, control_width: u16) -> (Rect, Rect) {
    let content = content(area);
    if content.width == 0 || content.height == 0 {
        return (Rect::default(), Rect::default());
    }
    split(content, control_width)
}

fn split(content: Rect, control_width: u16) -> (Rect, Rect) {
    let control_width = control_width.min(content.width);
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(control_width)])
        .split(content);
    (sections[0], sections[1])
}

/// Width left for wrapped descriptions after the control column.
pub(crate) fn description_width(content_width: u16, control_width: u16) -> u16 {
    content_width.saturating_sub(control_width.min(content_width))
}

/// Height of a row: top pad, title, descriptions, bottom pad.
pub(crate) fn row_height(description_lines: usize) -> u16 {
    3 + description_lines as u16
}

pub(crate) fn paint_selection(frame: &mut Frame, area: Rect, theme: &Theme, selected: bool) {
    if selected {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.surface)),
            area,
        );
    }
}

/// Paint a label plus its wrapped description and split off the right
/// control column. The same component serves settings fields and command
/// palette entries.
pub(crate) fn render(
    frame: &mut Frame,
    content: Rect,
    theme: &Theme,
    label: &str,
    description_lines: &[String],
    selected: bool,
    control_width: u16,
) -> Rect {
    if content.width == 0 || content.height == 0 {
        return Rect::default();
    }
    let (label_section, control_section) = split(content, control_width);
    let label_area = Rect {
        x: label_section.x,
        y: content.y,
        width: label_section.width,
        height: 1,
    };
    let label_style = if selected {
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    };
    frame.render_widget(Paragraph::new(label).style(label_style), label_area);

    let description_style = Style::default().fg(theme.description);
    for (index, line) in description_lines
        .iter()
        .take(usize::from(content.height.saturating_sub(1)))
        .enumerate()
    {
        let description_area = Rect {
            x: label_section.x,
            y: content.y.saturating_add(1 + index as u16),
            width: label_section.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(line.as_str()).style(description_style),
            description_area,
        );
    }

    Rect {
        x: control_section.x,
        y: content.y,
        width: control_section.width,
        height: 1,
    }
}

/// Word-wrap `text` to `width` columns (character count, not bytes).
/// Always returns at least one line so callers keep a minimum 2-row layout.
/// Longer text is capped at `MAX_DESCRIPTION_LINES` with an ellipsis.
pub(crate) fn wrap_description_lines(text: &str, width: u16) -> Vec<String> {
    let width = width as usize;
    if width == 0 {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    let mut truncated = false;

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        if word_len > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_len = 0;
            }
            // honey: hard-break overlong words by chars; fine for paths/hotkeys.
            let chars: Vec<char> = word.chars().collect();
            for chunk in chars.chunks(width) {
                lines.push(chunk.iter().collect());
            }
            continue;
        }
        if current_len == 0 {
            current.push_str(word);
            current_len = word_len;
        } else if current_len + 1 + word_len <= width {
            current.push(' ');
            current.push_str(word);
            current_len += 1 + word_len;
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            current_len = word_len;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    if lines.len() > MAX_DESCRIPTION_LINES {
        truncated = true;
        lines.truncate(MAX_DESCRIPTION_LINES);
    }
    if truncated && let Some(last) = lines.pop() {
        let capped: String = last.chars().take(width.saturating_sub(1)).collect();
        lines.push(format!("{capped}…"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_description_stays_on_one_line() {
        assert_eq!(wrap_description_lines("Short text", 40), vec!["Short text"]);
    }

    #[test]
    fn long_description_wraps_on_word_boundaries() {
        let lines = wrap_description_lines("aaa bbb ccc ddd", 7);
        assert_eq!(lines, vec!["aaa bbb", "ccc ddd"]);
    }

    #[test]
    fn overlong_word_hard_breaks() {
        assert_eq!(
            wrap_description_lines("abcdefghij", 4),
            vec!["abcd", "efgh", "ij"]
        );
    }

    #[test]
    fn caps_at_three_lines_with_ellipsis() {
        let lines = wrap_description_lines(
            "one two three four five six seven eight nine ten eleven twelve",
            10,
        );
        assert_eq!(lines.len(), MAX_DESCRIPTION_LINES);
        assert!(lines[MAX_DESCRIPTION_LINES - 1].ends_with('…'));
    }

    #[test]
    fn zero_width_still_returns_one_line() {
        assert_eq!(wrap_description_lines("anything", 0), vec![""]);
    }

    #[test]
    fn row_height_includes_padding_title_and_descriptions() {
        assert_eq!(row_height(0), 3);
        assert_eq!(row_height(2), 5);
    }

    #[test]
    fn description_width_reserves_the_control_column() {
        assert_eq!(description_width(30, 10), 20);
        assert_eq!(description_width(8, 20), 0);
    }
}
