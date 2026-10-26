//! Hotkey chords rendered as split keycap badges: every key gets its
//! own flat pill, separated by one cell of the underlying background.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

/// Prefixes that mark a `+`-delimited string as a key chord.
const MODIFIERS: [&str; 18] = [
    "ctrl", "control", "alt", "shift", "win", "super", "cmd", "command", "meta", "option", "lctrl",
    "rctrl", "lshift", "rshift", "lalt", "ralt", "lmeta", "rmeta",
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

/// Friendly keycap label for one chord token. Modifiers collapse to the
/// names used by the rest of the UI; letters and function keys
/// capitalize; named keys use their familiar short spelling.
pub(crate) fn display_label(key: &str) -> String {
    let lower = key.trim().to_ascii_lowercase();
    let named = match lower.as_str() {
        "ctrl" | "control" | "lctrl" | "rctrl" => Some("Ctrl"),
        "shift" | "lshift" | "rshift" => Some("Shift"),
        "alt" | "option" | "lalt" | "ralt" => Some("Alt"),
        "win" | "meta" | "super" | "cmd" | "command" | "lmeta" | "rmeta" | "lwin" | "rwin" => {
            Some("Win")
        }
        "esc" => Some("Esc"),
        "space" => Some("Space"),
        "tab" => Some("Tab"),
        "enter" => Some("Enter"),
        "backspace" | "bksp" => Some("Bksp"),
        "delete" | "del" => Some("Del"),
        "insert" | "ins" => Some("Ins"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pageup" | "pgup" => Some("PgUp"),
        "pagedown" | "pgdn" => Some("PgDn"),
        "up" => Some("Up"),
        "down" => Some("Down"),
        "left" => Some("Left"),
        "right" => Some("Right"),
        "capslock" | "caps" => Some("Caps"),
        "menu" => Some("Menu"),
        "printscreen" | "prtsc" => Some("PrtSc"),
        "scrolllock" | "scrlk" => Some("ScrLk"),
        "pause" => Some("Pause"),
        _ => None,
    };
    if let Some(label) = named {
        return label.to_string();
    }
    let trimmed = key.trim();
    if trimmed.chars().count() == 1 {
        return trimmed.to_ascii_uppercase();
    }
    let function_key = trimmed
        .strip_prefix('f')
        .or_else(|| trimmed.strip_prefix('F'))
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()));
    if function_key {
        return trimmed.to_ascii_uppercase();
    }
    trimmed.to_string()
}

/// Chord rewritten with [`display_label`] names, for keycap editors
/// outside the recorder/palette.
pub(crate) fn display_chord(chord: &str) -> String {
    labels(chord)
        .into_iter()
        .map(display_label)
        .collect::<Vec<_>>()
        .join("+")
}

/// Cell width of the pills [`render_hotkey_spans`] produces for
/// `chord`, including the one-cell gaps between badges.
pub(crate) fn display_width(chord: &str) -> usize {
    let labels = labels(chord);
    labels
        .iter()
        .map(|key| display_label(key).chars().count() + 2)
        .sum::<usize>()
        + labels.len().saturating_sub(1)
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
            "lctrl+a",
            "rshift+f6",
            "lmeta+space",
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
    fn display_labels_collapse_modifiers_and_capitalize_keys() {
        assert_eq!(display_label("lctrl"), "Ctrl");
        assert_eq!(display_label("rshift"), "Shift");
        assert_eq!(display_label("win"), "Win");
        assert_eq!(display_label("lmeta"), "Win");
        assert_eq!(display_label("f6"), "F6");
        assert_eq!(display_label("a"), "A");
        assert_eq!(display_label("`"), "`");
        assert_eq!(display_chord("win+lctrl"), "Win+Ctrl");
        assert_eq!(display_width("win+lctrl"), 12);
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
