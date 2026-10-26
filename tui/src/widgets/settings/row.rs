use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::hotkey;
use crate::widgets::library::icons::CHEVRON_DOWN;
use crate::widgets::settings::state::{EditorKind, SettingKey, SettingKeyMeta};
use crate::widgets::toggle::{ToggleAnimations, ToggleId, render_toggle, toggle_rect};
use crate::widgets::util;

fn setting_content(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

/// Right-aligned switch cell for a boolean row, shared by rendering
/// and hover hit-testing.
pub(crate) fn setting_toggle_rect(area: Rect, control_width: u16) -> Rect {
    let content = setting_content(area);
    if content.width == 0 || content.height == 0 {
        return Rect::default();
    }
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(control_width)])
        .split(content);
    toggle_rect(Rect {
        x: sections[1].x,
        y: content.y,
        width: sections[1].width,
        height: 1,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn render_setting_row(
    frame: &mut Frame,
    area: Rect,
    key: &SettingKey,
    settings: &taurine_core::settings::Settings,
    selected: bool,
    control_width: u16,
    description_lines: &[String],
    theme: &Theme,
    animations: &ToggleAnimations,
    hovered: Option<ToggleId>,
) {
    let row_style = if selected {
        Style::default().bg(theme.surface)
    } else {
        Style::default()
    };
    frame.render_widget(Block::default().style(row_style), area);

    let content = setting_content(area);
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
    if key.editor_kind() == EditorKind::Toggle
        && let Some(is_on) = key.toggle_value(settings)
    {
        let toggle = setting_toggle_rect(area, control_width);
        let id = ToggleId::Setting(key.storage_key());
        render_toggle(
            frame.buffer_mut(),
            toggle.x,
            toggle.y,
            is_on,
            hovered == Some(id),
            animations.progress_now(id),
        );
    } else {
        let value_area = Rect {
            x: sections[1].x,
            y: content.y,
            width: sections[1].width,
            height: 1,
        };
        if key.editor_kind() == EditorKind::HotkeyCapture {
            render_hotkey_value(
                frame,
                theme,
                key,
                settings,
                selected,
                value_area,
                value_style,
            );
        } else {
            let value = util::truncate_to_width(
                &key.display_value(settings),
                control_width.saturating_sub(2),
            );
            // The pill chevron flags a value that opens its own editor overlay.
            frame.render_widget(
                Paragraph::new(format!("{value} {CHEVRON_DOWN}"))
                    .alignment(Alignment::Right)
                    .style(value_style),
                value_area,
            );
        }
    }

    let description_style = Style::default().fg(theme.description);
    for (index, line) in description_lines
        .iter()
        .take(usize::from(content.height.saturating_sub(1)))
        .enumerate()
    {
        let description_area = Rect {
            x: content.x,
            y: content.y + 1 + index as u16,
            width: sections[0].width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(line.as_str()).style(description_style),
            description_area,
        );
    }
}

/// Hotkey values render as right-aligned keycap pills, like the
/// command palette hints, instead of text plus the editor chevron.
fn render_hotkey_value(
    frame: &mut Frame,
    theme: &Theme,
    key: &SettingKey,
    settings: &taurine_core::settings::Settings,
    selected: bool,
    area: Rect,
    style: Style,
) {
    let chord = hotkey::display_chord(&key.display_value(settings));
    let (keycap_bg, base_bg) = if selected {
        (theme.keycap.on_surface, theme.surface)
    } else {
        (theme.keycap.bg, util::OVERLAY_BG)
    };
    let spans =
        hotkey::render_hotkey_spans(&chord, area.width as usize, keycap_bg, theme.text, base_bg);
    let width = hotkey::spans_width(&spans) as u16;
    if width == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .alignment(Alignment::Right)
            .style(style),
        Rect {
            x: area.x.saturating_add(area.width.saturating_sub(width)),
            y: area.y,
            width,
            height: 1,
        },
    );
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

    #[test]
    fn switch_sits_at_the_right_content_edge() {
        let area = Rect::new(10, 20, 60, 8);
        let content = setting_content(area);
        let toggle = setting_toggle_rect(area, 20);
        assert_eq!(toggle.width, 5);
        assert_eq!(toggle.right(), content.right());
        assert_eq!(toggle.y, content.y);
    }

    fn row_buffer(key: SettingKey) -> ratatui::buffer::Buffer {
        use ratatui::{Terminal, backend::TestBackend};

        let area = Rect::new(0, 0, 60, 8);
        let settings = taurine_core::settings::Settings::default();
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_setting_row(
                    frame,
                    area,
                    &key,
                    &settings,
                    false,
                    20,
                    &[],
                    &crate::theme::default::DARK_THEME,
                    &ToggleAnimations::default(),
                    None,
                );
            })
            .expect("test draw");
        terminal.backend().buffer().clone()
    }

    /// Value line of a rendered row (the title sits one row above).
    fn row_line(key: SettingKey) -> String {
        let buffer = row_buffer(key);
        (0..buffer.area.width)
            .map(|x| buffer[(x, buffer.area.y + 1)].symbol())
            .collect()
    }

    #[test]
    fn boolean_row_renders_the_switch_cells() {
        let toggle = setting_toggle_rect(Rect::new(0, 0, 60, 8), 20);
        let buffer = row_buffer(SettingKey::StartOnBoot);
        for offset in 0..toggle.width {
            let cell = &buffer[(toggle.x + offset, toggle.y)];
            assert_eq!(cell.symbol(), " ");
            assert_ne!(cell.bg, ratatui::style::Color::Reset);
        }
    }

    #[test]
    fn hotkey_value_renders_as_keycaps_without_brackets() {
        let line = row_line(SettingKey::PauseHotkey);
        assert!(line.contains(" Alt "), "{line:?}");
        assert!(!line.contains(CHEVRON_DOWN), "{line:?}");
        assert!(!line.contains('['));
    }

    #[test]
    fn voice_hotkey_uses_modifier_labels() {
        let line = row_line(SettingKey::VoicePttHotkey);
        assert!(line.contains(" Win "), "{line:?}");
        assert!(line.contains(" Ctrl "), "{line:?}");
        assert!(!line.contains("win+lctrl"), "{line:?}");
        assert!(!line.contains(CHEVRON_DOWN), "{line:?}");
    }

    #[test]
    fn overlay_editors_get_the_pill_chevron() {
        assert!(
            row_line(SettingKey::SpinnerStyle)
                .trim_end()
                .ends_with(CHEVRON_DOWN)
        );
        assert!(!row_line(SettingKey::StartOnBoot).contains(CHEVRON_DOWN));
    }
}
