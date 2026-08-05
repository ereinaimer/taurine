use crate::theme::builtin::DARK_THEME;
use crate::widgets::library::LibrarySelectState;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
pub(crate) fn padded(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

pub(crate) fn row_input(frame: &mut Frame, area: Rect, value: &str, cursor: usize, focused: bool) {
    let bg = if focused {
        DARK_THEME.surface
    } else {
        DARK_THEME.background
    };
    let text_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text).bg(bg)
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
                        .fg(DARK_THEME.text)
                        .bg(DARK_THEME.surface)
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

pub(crate) fn row_key_value(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
) {
    let bg = if focused {
        DARK_THEME.surface
    } else {
        DARK_THEME.background
    };
    let text_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text)
    };
    let value_style = if focused {
        Style::default()
            .fg(DARK_THEME.text_muted)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text_muted)
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

pub(crate) fn row_password(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    disabled: bool,
    red_asterisk: bool,
) {
    if disabled {
        let dimmed = Style::default()
            .fg(DARK_THEME.text_muted)
            .add_modifier(Modifier::DIM);
        frame.render_widget(
            Block::default().style(Style::default().bg(DARK_THEME.background)),
            area,
        );
        frame.render_widget(Paragraph::new(label).style(dimmed), area);
        frame.render_widget(Paragraph::new(value).style(dimmed), area);
        return;
    }
    let bg = if focused {
        DARK_THEME.surface
    } else {
        DARK_THEME.background
    };
    let label_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text_muted)
    };
    let val_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text)
    };

    frame.render_widget(Block::default().style(Style::default().bg(bg)), area);

    let label_line = if red_asterisk {
        Line::from(vec![
            Span::styled(label, label_style),
            Span::styled("*", Style::default().fg(DARK_THEME.error)),
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

pub(crate) fn render_select_list(frame: &mut Frame, selector: &LibrarySelectState) {
    let inner = padded(frame.area());

    let block = Block::default()
        .style(Style::default().fg(DARK_THEME.text))
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
                        .fg(DARK_THEME.text)
                        .bg(DARK_THEME.surface)
                        .add_modifier(Modifier::BOLD),
                )])
            } else {
                Line::from(vec![Span::styled(
                    format!("  {opt}"),
                    Style::default().fg(DARK_THEME.text),
                )])
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(items), list_area);
}

pub(crate) fn render_path_row(
    frame: &mut Frame,
    area: Rect,
    value: &str,
    cursor: usize,
    focused: bool,
) -> Option<(u16, u16)> {
    let bg = if focused {
        DARK_THEME.surface
    } else {
        DARK_THEME.background
    };
    let label_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text)
    };
    let val_style = if focused {
        Style::default()
            .fg(DARK_THEME.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DARK_THEME.text_muted)
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

pub(crate) fn desc_area(section: Rect) -> (Rect, Rect) {
    let sub = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(section);
    (sub[0], sub[1])
}

pub(crate) fn render_desc(frame: &mut Frame, area: Rect, text: &str, focused: bool) {
    let bg = if focused {
        DARK_THEME.surface
    } else {
        DARK_THEME.background
    };
    frame.render_widget(Block::default().style(Style::default().bg(bg)), area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::raw(format!(" {text} "))])).style(
            Style::default()
                .fg(DARK_THEME.text_muted)
                .bg(bg)
                .add_modifier(Modifier::DIM),
        ),
        area,
    );
}
