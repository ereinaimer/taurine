//! Hotkey chords rendered as split keycap badges: every key gets its
//! own flat pill, separated by one cell of the underlying background.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

/// Prefixes that mark a `+`-delimited string as a key chord.
const MODIFIERS: [&str; 10] = [
    "ctrl", "control", "alt", "shift", "win", "super", "cmd", "command", "meta", "option",
];

/// True when `text` reads as `modifier+key`, e.g. `alt+f6` or
/// `ctrl+shift+p`. Plain words, regexes, and text like `c++` stay
/// text; the trigger's invocation type is authoritative when known.
pub(crate) fn is_hotkey_chord(text: &str) -> bool {
    let mut parts = text.split('+').map(str::trim);
    let Some(first) = parts.next() else {
        return false;
    };
    let modifier = first.to_ascii_lowercase();
    parts.next().is_some_and(|key| !key.is_empty()) && MODIFIERS.contains(&modifier.as_str())
}

/// Key labels in a chord, trimmed and without empty segments.
fn labels(hotkey: &str) -> Vec<&str> {
    hotkey
        .split('+')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .collect()
}

/// Split keycap spans, at most `budget` cells wide. Keys that do not
/// fit are dropped whole so a pill never splits mid-key.
pub(crate) fn render_hotkey_spans(
    hotkey: &str,
    budget: usize,
    keycap_bg: Color,
    keycap_fg: Color,
    base_bg: Color,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut used = 0usize;
    for key in labels(hotkey) {
        let badge = format!(" {key} ");
        let badge_width = badge.chars().count();
        let gap = usize::from(!spans.is_empty());
        if used + gap + badge_width > budget {
            break;
        }
        if gap == 1 {
            spans.push(Span::styled(" ", Style::default().bg(base_bg)));
            used += 1;
        }
        spans.push(Span::styled(
            badge,
            Style::default().bg(keycap_bg).fg(keycap_fg),
        ));
        used += badge_width;
    }
    spans
}

/// Rendered width of a badge run in cells.
pub(crate) fn spans_width(spans: &[Span<'static>]) -> usize {
    spans.iter().map(Span::width).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYCAP_BG: Color = Color::Rgb(0x24, 0x24, 0x28);
    const KEYCAP_FG: Color = Color::Rgb(0xCF, 0xCF, 0xCF);
    const BASE_BG: Color = Color::Rgb(0x0A, 0x0A, 0x0A);

    fn spans(hotkey: &str) -> Vec<Span<'static>> {
        render_hotkey_spans(hotkey, 80, KEYCAP_BG, KEYCAP_FG, BASE_BG)
    }

    #[test]
    fn detects_modifier_chords_only() {
        for chord in [
            "alt+f6",
            "ctrl+shift+p",
            "win+space",
            "super+x",
            "cmd+k",
            "Ctrl+Alt+Del",
        ] {
            assert!(is_hotkey_chord(chord), "{chord}");
        }
        for text in ["gaming mode", "gm", "c++", "ctrl", "alt+", "", "f6+alt"] {
            assert!(!is_hotkey_chord(text), "{text}");
        }
    }

    #[test]
    fn splits_a_chord_into_one_pill_per_key() {
        let spans = spans("alt+f6");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, " alt ");
        assert_eq!(spans[0].style.bg, Some(KEYCAP_BG));
        assert_eq!(spans[0].style.fg, Some(KEYCAP_FG));
        assert_eq!(spans[1].content, " ");
        assert_eq!(spans[1].style.bg, Some(BASE_BG));
        assert_eq!(spans[2].content, " f6 ");
        assert_eq!(spans[2].style.bg, Some(KEYCAP_BG));
        assert_eq!(spans_width(&spans), 10);
    }

    #[test]
    fn trims_segments_and_skips_empties() {
        let spans = spans(" ctrl + shift + p ");
        let keys: Vec<&str> = spans
            .iter()
            .filter(|span| span.content != " ")
            .map(|span| span.content.trim())
            .collect();
        assert_eq!(keys, ["ctrl", "shift", "p"]);
    }

    #[test]
    fn budget_drops_whole_keys() {
        // " alt " + gap = 6, so the second pill (4 more) needs 10.
        assert_eq!(spans("alt+f6").len(), 3);
        let tight = render_hotkey_spans("alt+f6", 6, KEYCAP_BG, KEYCAP_FG, BASE_BG);
        assert_eq!(tight.len(), 1);
        assert_eq!(tight[0].content, " alt ");
        assert!(render_hotkey_spans("alt+f6", 4, KEYCAP_BG, KEYCAP_FG, BASE_BG).is_empty());
    }
}
