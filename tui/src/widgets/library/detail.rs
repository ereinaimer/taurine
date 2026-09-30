use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::theme::Theme;
use crate::widgets::library::icons::{MIC_ICON, os_icon};
use crate::widgets::library::state::LibraryPageState;
use crate::widgets::util;

use super::{content_halves, page_area};

const PREVIEW_LINES: usize = 8;

/// Right-pane area with page padding, mirroring the left pane.
pub(crate) fn right_content(area: Rect) -> Rect {
    let (_, right) = content_halves(area);
    page_area(right)
}

pub(crate) fn render_detail(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &LibraryPageState,
) {
    let content = right_content(area);
    if content.width == 0 || content.height == 0 {
        return;
    }
    if state.load_error().is_some() {
        return;
    }

    let Some(index) = state.selected_index() else {
        render_empty(
            frame,
            content,
            theme,
            state
                .empty_state_message()
                .unwrap_or("No trigger selected."),
        );
        return;
    };
    let Some(item) = state.item_at_filtered(index) else {
        render_empty(frame, content, theme, "No trigger selected.");
        return;
    };

    render_trigger_row(frame, content, theme, item);
    render_kind_row(frame, content, theme, item);
    render_content_rows(frame, content, theme, item);
}

fn render_empty(frame: &mut Frame, area: Rect, theme: &Theme, message: &str) {
    frame.render_widget(
        Paragraph::new(message).style(
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1.min(area.height),
        },
    );
}

fn render_trigger_row(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &crate::widgets::library::state::LibraryTrigger,
) {
    let style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);
    if item.is_voice() {
        let line = Line::from(vec![
            Span::styled(MIC_ICON, style),
            Span::raw("  "),
            Span::styled(
                util::truncate_to_width(item.trigger(), area.width.saturating_sub(4)),
                style,
            ),
        ]);
        frame.render_widget(Paragraph::new(line), row_area(area, 0));
    } else if item.is_hotkey() {
        frame.render_widget(
            Paragraph::new(hotkey_line(item.trigger(), area.width, theme)),
            row_area(area, 0),
        );
    } else {
        frame.render_widget(
            Paragraph::new(util::truncate_to_width(item.trigger(), area.width)).style(style),
            row_area(area, 0),
        );
    }
}

fn render_kind_row(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &crate::widgets::library::state::LibraryTrigger,
) {
    if area.height < 2 {
        return;
    }
    let icon = os_icon(&item.target_os);
    let kind = item.kind_label();
    let icon_width = 1u16;
    let gap = 2u16;
    let kind_width = area.width.saturating_sub(icon_width + gap);
    let text = util::truncate_to_width(kind, kind_width);
    let line = Line::from(vec![
        Span::styled(text, Style::default().fg(theme.text_muted)),
        Span::raw(" ".repeat(gap as usize)),
        Span::styled(icon, Style::default().fg(theme.text_muted)),
    ]);
    frame.render_widget(Paragraph::new(line), row_area(area, 1));
}

fn render_content_rows(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    item: &crate::widgets::library::state::LibraryTrigger,
) {
    if area.height < 4 {
        return;
    }
    let divider = Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(theme.border),
    ));
    frame.render_widget(Paragraph::new(divider), row_area(area, 2));

    let start_y = area.y.saturating_add(3);
    let available = area.height.saturating_sub(3) as usize;
    for (index, line) in item
        .preview()
        .lines()
        .take(PREVIEW_LINES.min(available))
        .enumerate()
    {
        let row = Rect {
            x: area.x,
            y: start_y.saturating_add(index as u16),
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(util::truncate_to_width(line.trim(), area.width))
                .style(Style::default().fg(theme.description)),
            row,
        );
    }
}

fn hotkey_line(trigger: &str, width: u16, theme: &Theme) -> Line<'static> {
    let chip = Style::default()
        .fg(theme.text)
        .bg(theme.surface)
        .add_modifier(Modifier::BOLD);
    let plus = Style::default().fg(theme.text_muted);
    let mut spans = Vec::new();
    let parts: Vec<&str> = trigger
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Line::from(util::truncate_to_width(trigger, width));
    }
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" + ", plus));
        }
        spans.push(Span::styled(format!("[{part}]"), chip));
    }
    Line::from(spans)
}

fn row_area(area: Rect, offset: u16) -> Rect {
    Rect {
        x: area.x,
        y: area.y.saturating_add(offset),
        width: area.width,
        height: 1,
    }
}
