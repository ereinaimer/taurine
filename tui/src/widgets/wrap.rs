//! Shared word wrapping for content panes.
//!
//! Both the library content box and the home activity feed measure rows in
//! terminal cells, move whole words to the next row, and hard-split only a
//! word that is wider than the row it sits on.

use std::ops::Range;

use ratatui::text::Span;

/// Terminal cells a string occupies; wide glyphs count as two.
pub(crate) fn cell_width(text: &str) -> usize {
    Span::raw(text).width()
}

fn char_width(ch: char) -> usize {
    let mut buf = [0u8; 4];
    cell_width(ch.encode_utf8(&mut buf))
}

/// Wraps one logical line into visual rows of at most `first` cells on the
/// first row and `rest` on every later row.
///
/// Words move whole to the next row; only a word wider than its row is
/// split, and never through a glyph. Interior spacing is preserved, the
/// space a row breaks on is dropped, tabs flatten to two cells, other
/// control characters flatten to one, and a blank line stays one blank row.
///
/// Each row carries the char range it came from so callers can map a caret
/// back onto the source line. Ranges index the flattened line, where a tab
/// occupies two chars.
pub(crate) fn wrap_line(line: &str, first: usize, rest: usize) -> Vec<(String, Range<usize>)> {
    let first = first.max(1);
    let rest = rest.max(1);
    let flat: String = line
        .chars()
        .flat_map(|ch| match ch {
            '\t' => vec![' ', ' '],
            ch if ch.is_control() => vec![' '],
            ch => vec![ch],
        })
        .collect();
    let trimmed = flat.trim_end();
    if trimmed.is_empty() {
        return vec![(String::new(), 0..0)];
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let widths: Vec<usize> = chars.iter().map(|&ch| char_width(ch)).collect();
    let mut rows = Vec::new();
    let mut start = 0usize;
    let mut budget = first;

    while start < chars.len() {
        let mut used = 0usize;
        let mut end = start;
        while end < chars.len() && used.saturating_add(widths[end]) <= budget {
            used += widths[end];
            end += 1;
        }
        if end == chars.len() {
            rows.push((chars[start..].iter().collect(), start..chars.len()));
            break;
        }
        if end == start {
            // A glyph wider than the row still gets a row of its own.
            end += 1;
            rows.push((chars[start..end].iter().collect(), start..end));
            start = end;
        } else if let Some(gap) = chars[start..end].iter().rposition(|&ch| ch == ' ') {
            rows.push((
                chars[start..start + gap].iter().collect(),
                start..start + gap,
            ));
            start += gap + 1;
        } else {
            // No space fits: hard-break the overlong word at the last cell
            // that fits, so paths and URLs still show every character.
            rows.push((chars[start..end].iter().collect(), start..end));
            start = end;
            while start < chars.len() && chars[start] == ' ' {
                start += 1;
            }
        }
        budget = rest;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(rows: &[(String, Range<usize>)]) -> Vec<&str> {
        rows.iter().map(|(text, _)| text.as_str()).collect()
    }

    #[test]
    fn moves_whole_words_to_the_next_row() {
        let text = "even though AGENTS.md prevents you from making commits";
        let rows = wrap_line(text, 20, 20);

        assert_eq!(
            texts(&rows),
            vec![
                "even though",
                "AGENTS.md prevents",
                "you from making",
                "commits",
            ]
        );
        assert!(rows.iter().all(|(row, _)| cell_width(row) <= 20));
    }

    #[test]
    fn overlong_word_splits_without_losing_a_character() {
        let rows = wrap_line("abcdefghijklmnop", 5, 4);

        assert_eq!(texts(&rows), vec!["abcde", "fghi", "jklm", "nop"]);
        assert_eq!(
            rows.iter().map(|(row, _)| row.as_str()).collect::<String>(),
            "abcdefghijklmnop"
        );
    }

    #[test]
    fn first_row_can_be_narrower_than_the_rest() {
        let rows = wrap_line("alpha beta gamma delta", 8, 12);

        assert_eq!(texts(&rows), vec!["alpha", "beta gamma", "delta"]);
    }

    #[test]
    fn counts_terminal_cells_not_chars() {
        let rows = wrap_line("日本語テキスト", 4, 4);

        assert_eq!(texts(&rows), vec!["日本", "語テ", "キス", "ト"]);
        assert!(rows.iter().all(|(row, _)| cell_width(row) <= 4));
    }

    #[test]
    fn blank_lines_and_tabs_keep_their_shape() {
        assert_eq!(texts(&wrap_line("", 10, 10)), vec![""]);
        assert_eq!(texts(&wrap_line("   ", 10, 10)), vec![""]);
        assert_eq!(texts(&wrap_line("\tb", 10, 10)), vec!["  b"]);
    }

    #[test]
    fn ranges_map_rows_back_to_the_source() {
        let line = "one two three four";
        let rows = wrap_line(line, 8, 8);

        assert_eq!(texts(&rows), vec!["one two", "three", "four"]);
        let source: Vec<char> = line.chars().collect();
        for (text, range) in rows {
            let slice: String = source[range.clone()].iter().collect();
            assert_eq!(slice, text, "range {range:?}");
        }
    }
}
