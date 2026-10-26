use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph},
};

use crate::theme::Theme;
use crate::widgets::hotkey;
use crate::widgets::labeled;
use crate::widgets::library::icons::CHEVRON_DOWN;
use crate::widgets::settings::state::{EditorKind, InputModalState, SettingKey, SettingKeyMeta};
use crate::widgets::slider;
use crate::widgets::toggle::{ToggleAnimations, ToggleId, render_toggle, toggle_rect};
use crate::widgets::util;

/// Right-aligned switch cell for a boolean row, shared by rendering
/// and hover hit-testing.
pub(crate) fn setting_toggle_rect(area: Rect, control_width: u16) -> Rect {
    let (_, value) = labeled::sections(area, control_width);
    if value.width == 0 || value.height == 0 {
        return Rect::default();
    }
    toggle_rect(Rect {
        x: value.x,
        y: value.y,
        width: value.width,
        height: 1,
    })
}

/// Rect covered by the keycap pills of a hotkey setting, if it has any.
/// Shared by rendering and hit-testing so clicking a pill opens the
/// on-screen recorder.
pub(crate) fn hotkey_value_rect(
    area: Rect,
    control_width: u16,
    key: SettingKey,
    settings: &taurine_core::settings::Settings,
) -> Option<Rect> {
    if key.editor_kind() != EditorKind::HotkeyCapture {
        return None;
    }
    let (_, value_column) = labeled::sections(area, control_width);
    if value_column.width == 0 || value_column.height == 0 {
        return None;
    }
    let chord = hotkey::display_chord(&key.display_value(settings));
    let spans = hotkey::render_hotkey_spans(
        &chord,
        value_column.width as usize,
        Color::Reset,
        Color::Reset,
        Color::Reset,
    );
    let width = hotkey::spans_width(&spans) as u16;
    if width == 0 {
        return None;
    }
    Some(Rect {
        x: value_column
            .x
            .saturating_add(value_column.width.saturating_sub(width)),
        y: value_column.y,
        width,
        height: 1,
    })
}

/// Filled value field for a setting edited directly in the row: a pill
/// hugging just the text, right-aligned in the value column.
fn inline_field_bounds(column: Rect, text_width: u16) -> Rect {
    let width = text_width.saturating_add(2).max(2).min(column.width);
    Rect {
        x: column.right().saturating_sub(width),
        y: column.y,
        width,
        height: 1,
    }
}

/// Rect covered by the text pill of an inline-editable setting. Shared
/// by rendering and hit-testing so the clickable area matches the paint.
pub(crate) fn inline_value_rect(
    area: Rect,
    control_width: u16,
    key: SettingKey,
    settings: &taurine_core::settings::Settings,
    inline_edit: Option<&InputModalState>,
) -> Option<Rect> {
    if !key.inline_edit() {
        return None;
    }
    let (_, value) = labeled::sections(area, control_width);
    if value.width == 0 || value.height == 0 {
        return None;
    }
    let max_text = value.width.saturating_sub(2);
    let text_width = match inline_edit {
        Some(state) if state.key() == key => {
            state.field().window(max_text).0.chars().count() as u16
        }
        _ => key
            .display_value(settings)
            .chars()
            .count()
            .min(max_text as usize) as u16,
    };
    Some(inline_field_bounds(value, text_width))
}

/// Whole value column for a setting whose value opens a centered picker.
pub(crate) fn select_value_rect(area: Rect, control_width: u16, key: SettingKey) -> Option<Rect> {
    if !key.editor_kind().opens_select() {
        return None;
    }
    let (_, value) = labeled::sections(area, control_width);
    if value.width == 0 || value.height == 0 {
        return None;
    }
    Some(Rect {
        x: value.x,
        y: value.y,
        width: value.width,
        height: 1,
    })
}

/// Track of the audio volume slider. Shared by rendering and hit-testing
/// so a click lands on the cell it paints.
pub(crate) fn volume_slider_rect(area: Rect, control_width: u16, key: SettingKey) -> Option<Rect> {
    if key.editor_kind() != EditorKind::VolumeSlider {
        return None;
    }
    let (_, value) = labeled::sections(area, control_width);
    slider::track_rect(value)
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
    inline_edit: Option<&InputModalState>,
) {
    labeled::paint_selection(frame, area, theme, selected);
    let content = labeled::content(area);
    if content.width == 0 || content.height == 0 {
        return;
    }

    let value_style = if selected {
        Style::default()
            .fg(theme.text)
            .bg(theme.surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let value_area = labeled::render(
        frame,
        content,
        theme,
        key.display_name(),
        description_lines,
        selected,
        control_width,
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
    } else if key.editor_kind() == EditorKind::HotkeyCapture {
        render_hotkey_value(
            frame,
            theme,
            key,
            settings,
            selected,
            value_area,
            value_style,
        );
    } else if key.editor_kind() == EditorKind::VolumeSlider {
        render_volume_slider(frame, value_style, settings.audio_volume, value_area);
    } else if key.inline_edit() {
        render_inline_value(
            frame,
            theme,
            key,
            settings,
            selected,
            value_area,
            inline_edit.filter(|state| state.key() == *key),
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

/// Inline values sit in a filled pill that hugs their text; the active
/// editor shows its live text plus the terminal caret.
fn render_inline_value(
    frame: &mut Frame,
    theme: &Theme,
    key: &SettingKey,
    settings: &taurine_core::settings::Settings,
    selected: bool,
    area: Rect,
    inline_edit: Option<&InputModalState>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let max_text = area.width.saturating_sub(2);

    let (text, caret) = match inline_edit {
        Some(state) => {
            let (visible, caret) = state.field().window(max_text);
            (visible.to_string(), Some(caret))
        }
        None => (
            util::truncate_to_width(&key.display_value(settings), max_text),
            None,
        ),
    };
    let field = inline_field_bounds(area, text.chars().count() as u16);
    let bg = if inline_edit.is_some() {
        theme.keycap.on_surface
    } else {
        theme.keycap.bg
    };
    frame.render_widget(Block::default().style(Style::default().bg(bg)), field);

    let text_area = Rect {
        x: field.x.saturating_add(1),
        y: field.y,
        width: field.width.saturating_sub(2),
        height: 1,
    };
    let mut style = Style::default().fg(theme.text).bg(bg);
    if selected {
        style = style.add_modifier(Modifier::BOLD);
    }
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Right)
            .style(style),
        text_area,
    );

    if let Some(caret) = caret
        && !crate::overlay_fx::is_cursor_suppressed()
    {
        // honey: an end-of-text caret belongs in the pill's right
        // padding cell, after the last character, rather than on top
        // of it (which reads as the left side of the text with a bar
        // cursor).
        let x = text_area
            .x
            .saturating_add(caret as u16)
            .min(field.right().saturating_sub(1));
        frame.set_cursor_position((x, text_area.y));
    }
}

/// Audio volume renders as a right-aligned slider that extends the switch
/// language: filled track, empty tail, and the same two-cell knob.
fn render_volume_slider(frame: &mut Frame, style: Style, volume: u32, area: Rect) {
    let Some(track) = slider::track_rect(area) else {
        return;
    };
    let label = format!(
        "{:>width$}%",
        volume.min(100),
        width = usize::from(slider::PERCENT_WIDTH.saturating_sub(1))
    );
    frame.render_widget(
        Paragraph::new(label)
            .alignment(Alignment::Right)
            .style(style),
        slider::label_rect(track),
    );
    frame.render_widget(
        Paragraph::new(Line::from(slider::spans(volume, track.width))),
        track,
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_sits_at_the_right_content_edge() {
        let area = Rect::new(10, 20, 60, 8);
        let content = labeled::content(area);
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
    fn volume_row_renders_a_slider_with_a_percent_label() {
        use crate::widgets::toggle::{DISABLED_TRACK, ENABLED_KNOB, ENABLED_TRACK};

        let line = row_line(SettingKey::AudioVolume);
        assert!(line.contains("50%"), "{line:?}");
        assert!(!line.contains(CHEVRON_DOWN), "{line:?}");

        let track = volume_slider_rect(Rect::new(0, 0, 60, 8), 20, SettingKey::AudioVolume)
            .expect("volume slider track");
        let buffer = row_buffer(SettingKey::AudioVolume);
        let bgs: Vec<_> = (0..track.width)
            .map(|offset| buffer[(track.x + offset, track.y)].bg)
            .collect();
        assert_eq!(bgs[0], ENABLED_TRACK);
        assert_eq!(*bgs.last().unwrap(), DISABLED_TRACK);
        assert!(bgs.contains(&ENABLED_KNOB));
    }

    #[test]
    fn hotkey_value_renders_as_keycaps_without_brackets() {
        let line = row_line(SettingKey::PauseHotkey);
        assert!(line.contains(" Alt "), "{line:?}");
        assert!(!line.contains(CHEVRON_DOWN), "{line:?}");
        assert!(!line.contains('['));
    }

    #[test]
    fn inline_values_render_as_filled_fields_without_the_chevron() {
        let key = SettingKey::ClipboardHistoryRetentionSecs;
        let settings = taurine_core::settings::Settings::default();
        let buffer = row_buffer(key);
        let field = inline_value_rect(Rect::new(0, 0, 60, 8), 20, key, &settings, None)
            .expect("inline field");
        let cell = &buffer[(field.x + field.width / 2, field.y)];
        assert_eq!(cell.bg, crate::theme::default::DARK_THEME.keycap.bg);
        // The pill hugs the value instead of filling the value column.
        assert_eq!(
            field.width as usize,
            key.display_value(&settings).chars().count() + 2
        );
        assert!(!row_line(key).contains(CHEVRON_DOWN));
    }

    #[test]
    fn active_inline_caret_sits_at_the_text_end() {
        use ratatui::{Terminal, backend::TestBackend};

        let key = SettingKey::ClipboardHistoryRetentionSecs;
        let settings = taurine_core::settings::Settings::default();
        let editor = InputModalState::new(key, "300".to_string());
        let area = Rect::new(0, 0, 60, 8);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_setting_row(
                    frame,
                    area,
                    &key,
                    &settings,
                    true,
                    20,
                    &[],
                    &crate::theme::default::DARK_THEME,
                    &ToggleAnimations::default(),
                    None,
                    Some(&editor),
                );
            })
            .expect("test draw");

        let field =
            inline_value_rect(area, 20, key, &settings, Some(&editor)).expect("inline field");
        let cursor = terminal.backend().cursor_position();
        assert_eq!(cursor.y, field.y);
        assert_eq!(
            terminal.backend().buffer()[(field.right().saturating_sub(2), field.y)].symbol(),
            "0",
            "the last digit should end one cell before the caret"
        );
        assert_eq!(
            cursor.x,
            field.right().saturating_sub(1),
            "end-of-text caret should sit after the text"
        );
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
