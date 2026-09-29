use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::settings::state::SettingKeyMeta;
use crate::widgets::util;

#[allow(clippy::too_many_arguments)]
pub fn render_setting_row(
    frame: &mut Frame,
    area: Rect,
    key: &crate::widgets::settings::state::SettingKey,
    settings: &taurine_core::settings::Settings,
    selected: bool,
    control_width: u16,
    description_lines: &[String],
    theme: &Theme,
) {
    let row_style = if selected {
        Style::default().bg(theme.surface)
    } else {
        Style::default()
    };
    frame.render_widget(Block::default().style(row_style), area);

    let content = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    if content.width == 0 || content.height == 0 {
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(control_width)])
        .split(content);

    let label_style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    };
    let value_style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };

    let title_area = Rect {
        x: sections[0].x,
        y: content.y,
        width: sections[0].width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(key.display_name()).style(label_style),
        title_area,
    );
    let value_area = Rect {
        x: sections[1].x,
        y: content.y,
        width: sections[1].width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(format!(
            "[ {} ]",
            util::truncate_to_width(
                &key.display_value(settings),
                control_width.saturating_sub(4)
            )
        ))
        .alignment(Alignment::Right)
        .style(value_style),
        value_area,
    );

    let description_style = Style::default().fg(theme.description);
    for (index, line) in description_lines
        .iter()
        .take(usize::from(content.height.saturating_sub(1)))
        .enumerate()
    {
        let description_area = Rect {
            x: content.x,
            y: content.y + 1 + index as u16,
            width: content.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(line.as_str()).style(description_style),
            description_area,
        );
    }
}

/// Maximum wrapped description lines shown under a setting title.
pub(crate) const MAX_DESCRIPTION_LINES: usize = 3;

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
}
