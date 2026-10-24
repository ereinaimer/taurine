/// Shared single-line text field: text plus a character-index caret.
/// One implementation for every editable text in the app (search boxes,
/// trigger editor, modal inputs, overlay inputs). Key handling stays at
/// each call site (keymaps differ); this owns state, viewport geometry,
/// and caret math only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TextField {
    text: String,
    cursor: usize,
}

impl TextField {
    /// Caret parked at the end, like freshly focused search.
    pub(crate) fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(crate) fn len_chars(&self) -> usize {
        self.text.chars().count()
    }

    pub(crate) fn insert(&mut self, ch: char) {
        let byte = byte_index(&self.text, self.cursor);
        self.text.insert(byte, ch);
        self.cursor = self.cursor.saturating_add(1);
    }

    /// Delete behind the caret. False when already at the start.
    pub(crate) fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let end = byte_index(&self.text, self.cursor);
        let start = byte_index(&self.text, self.cursor - 1);
        self.text.drain(start..end);
        self.cursor = self.cursor.saturating_sub(1);
        true
    }

    /// Delete at the caret. False when already at the end.
    pub(crate) fn delete_at(&mut self) -> bool {
        if self.cursor >= self.len_chars() {
            return false;
        }
        let start = byte_index(&self.text, self.cursor);
        let end = byte_index(&self.text, self.cursor + 1);
        self.text.drain(start..end);
        true
    }

    pub(crate) fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub(crate) fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len_chars());
    }

    pub(crate) fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.cursor = self.len_chars();
    }

    fn is_word_char(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    /// Jump back over the previous word (spaces, then word chars).
    pub(crate) fn move_word_left(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut index = self.cursor;
        while index > 0 && !Self::is_word_char(chars[index - 1]) {
            index -= 1;
        }
        while index > 0 && Self::is_word_char(chars[index - 1]) {
            index -= 1;
        }
        self.cursor = index;
    }

    /// Jump forward over the next word.
    pub(crate) fn move_word_right(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let len = chars.len();
        let mut index = self.cursor;
        while index < len && !Self::is_word_char(chars[index]) {
            index += 1;
        }
        while index < len && Self::is_word_char(chars[index]) {
            index += 1;
        }
        self.cursor = index;
    }

    /// Delete back over the previous word. False when already at the start.
    pub(crate) fn delete_word_before(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let target = self.cursor;
        self.move_word_left();
        self.text
            .drain(byte_index(&self.text, self.cursor)..byte_index(&self.text, target));
        true
    }

    /// Delete forward over the next word. False when already at the end.
    pub(crate) fn delete_word_after(&mut self) -> bool {
        if self.cursor >= self.len_chars() {
            return false;
        }
        let target = self.cursor;
        self.move_word_right();
        self.text
            .drain(byte_index(&self.text, target)..byte_index(&self.text, self.cursor));
        self.cursor = target;
        true
    }

    /// Caret placement, e.g. from a click column. Clamps past the end.
    pub(crate) fn place(&mut self, index: usize) {
        self.cursor = index.min(self.len_chars());
    }

    /// Visible window for a `width`-wide field plus the caret column
    /// inside it. The caret is always visible: head while everything
    /// fits, tail ending at the caret once text overflows. A caret past
    /// the last cell (typing at the end) clamps at render time.
    pub(crate) fn window(&self, width: u16) -> (&str, usize) {
        window_of(self.text(), self.cursor, width)
    }

    /// Click column (0-based from the field start) to caret index.
    /// Display width is char-count based, matching layout math everywhere.
    pub(crate) fn index_at(&self, column: usize) -> usize {
        column.min(self.len_chars())
    }

    /// Wrapped layout at `width` cells. `include_caret` adds the trailing
    /// line a caret needs when it sits past a full last line.
    pub(crate) fn wrapped(&self, width: u16, include_caret: bool) -> WrappedField {
        wrap(self.text(), self.cursor(), width, include_caret)
    }
}

/// Window math over raw display text plus a caret index, for masked
/// values whose stored text differs from what is shown.
pub(crate) fn window_of(text: &str, cursor: usize, width: u16) -> (&str, usize) {
    let width = (width.max(1)) as usize;
    let total = text.chars().count();
    let start = if total <= width {
        0
    } else if cursor >= total {
        total.saturating_sub(width)
    } else {
        (cursor + 1).saturating_sub(width).min(total)
    };
    let start_byte = byte_index(text, start);
    let end_byte = text
        .char_indices()
        .nth(start.saturating_add(width))
        .map(|(byte, _)| byte)
        .unwrap_or(text.len());
    (&text[start_byte..end_byte], cursor.saturating_sub(start))
}

fn byte_index(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(value.len())
}

/// Visual layout of a field wrapped to a cell width: the text split into
/// display lines plus the caret's visual row and column. `lines` carries
/// an empty trailing line when the caret sits past a full last line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WrappedField {
    lines: Vec<String>,
    starts: Vec<usize>,
    ends: Vec<usize>,
    caret_row: usize,
    caret_col: usize,
}

impl WrappedField {
    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }

    pub(crate) const fn caret(&self) -> (usize, usize) {
        (self.caret_row, self.caret_col)
    }

    /// Row offset that keeps the caret inside `visible_rows`.
    pub(crate) fn scroll(&self, visible_rows: usize) -> usize {
        if visible_rows == 0 || self.lines.len() <= visible_rows {
            0
        } else {
            self.caret_row.saturating_sub(visible_rows - 1)
        }
    }

    /// Char index for a click at visual `(row, column)`.
    pub(crate) fn index_at(&self, row: usize, column: usize) -> usize {
        let row = row.min(self.lines.len().saturating_sub(1));
        let start = self.starts[row];
        start + column.min(self.ends[row].saturating_sub(start))
    }
}

/// Greedy word wrap at `width` cells. Breaks at the last space that keeps
/// a word whole, hard-breaks words longer than the line, and skips the
/// space a break lands on so wrapped lines do not start with padding.
fn wrap(text: &str, cursor: usize, width: u16, include_caret: bool) -> WrappedField {
    let chars: Vec<char> = text.chars().collect();
    let width = usize::from(width.max(1));
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        // honey: wrap points collapse the space they land on, so wrapped
        // lines start at the first glyph instead of a blank cell.
        while start < chars.len() && start > 0 && chars[start] == ' ' {
            start += 1;
        }
        if start >= chars.len() {
            break;
        }
        let mut end = start;
        let mut last_space = None;
        while end < chars.len() && end - start < width {
            if chars[end] == ' ' {
                last_space = Some(end);
            }
            end += 1;
        }
        if end == chars.len() {
            starts.push(start);
            ends.push(end);
            break;
        }
        let (line_end, next_start) = match last_space {
            Some(space) if space > start => (space, space + 1),
            _ => (end, end),
        };
        starts.push(start);
        ends.push(line_end);
        start = next_start;
    }
    if starts.is_empty() {
        starts.push(0);
        ends.push(0);
    }

    let mut lines: Vec<String> = starts
        .iter()
        .zip(&ends)
        .map(|(&start, &end)| chars[start..end].iter().collect())
        .collect();

    let mut row = starts
        .partition_point(|&line_start| line_start <= cursor)
        .saturating_sub(1);
    let mut col = cursor
        .saturating_sub(starts[row])
        .min(ends[row].saturating_sub(starts[row]));
    if col == width {
        // Caret at the end of a full line belongs at the start of the next.
        row += 1;
        col = 0;
    }
    if row >= lines.len() {
        if include_caret {
            // A caret past a full last line needs a line of its own so the
            // block grows and the terminal cursor stays on screen.
            starts.push(chars.len());
            ends.push(chars.len());
            lines.push(String::new());
        } else {
            row = lines.len().saturating_sub(1);
            col = ends[row].saturating_sub(starts[row]).saturating_sub(1);
        }
    }

    WrappedField {
        lines,
        starts,
        ends,
        caret_row: row,
        caret_col: col,
    }
}

#[cfg(test)]
mod tests {
    use super::TextField;

    #[test]
    fn fresh_field_parks_caret_at_end() {
        let field = TextField::new("gm");
        assert_eq!(field.cursor(), 2);
        assert_eq!(field.text(), "gm");
    }

    #[test]
    fn insert_and_delete_round_trip_at_cursor() {
        let mut field = TextField::new("ac");
        field.move_left();
        field.insert('b');
        assert_eq!(field.text(), "abc");
        assert_eq!(field.cursor(), 2);
        assert!(field.backspace());
        assert_eq!(field.text(), "ac");
        assert!(field.delete_at());
        assert_eq!(field.text(), "a");
        assert!(!field.delete_at());
        field.move_home();
        assert!(!field.backspace());
    }

    #[test]
    fn cursor_clamps_everywhere() {
        let mut field = TextField::new("ab");
        assert_eq!(field.cursor(), 2);
        field.place(99);
        assert_eq!(field.cursor(), 2);
        field.move_right();
        assert_eq!(field.cursor(), 2);
        field.move_home();
        field.move_left();
        assert_eq!(field.cursor(), 0);
        field.move_end();
        assert_eq!(field.text(), "ab");
        assert!(!field.is_empty());
    }

    #[test]
    fn window_shows_head_then_slides_with_caret() {
        let field = TextField::new("hello world");
        let (visible, caret) = field.window(5);
        assert_eq!((visible, caret), ("world", 5));

        let mut head = TextField::new("hello world");
        head.move_home();
        assert_eq!(head.window(5), ("hello", 0));

        let mut mid = TextField::new("hello world");
        mid.place(6);
        assert_eq!(mid.window(5), ("llo w", 4));
    }

    #[test]
    fn window_never_splits_a_char() {
        let field = TextField::new("héllo wörld");
        let (visible, _) = field.window(5);
        assert!(visible.is_char_boundary(0));
        assert!(visible.is_char_boundary(visible.len()));
    }

    #[test]
    fn masked_view_concept() {
        // Masking renders "*" per character at the call site, keeping the
        // shared field free of display policy.
        let field = TextField::new("s3cr3t");
        assert_eq!("*".repeat(field.len_chars()), "******");
    }

    #[test]
    fn index_at_clamps_past_end() {
        let field = TextField::new("gm");
        assert_eq!(field.index_at(0), 0);
        assert_eq!(field.index_at(2), 2);
        assert_eq!(field.index_at(99), 2);
    }

    #[test]
    fn multibyte_insert_delete() {
        let mut field = TextField::new("héllo");
        field.move_home();
        field.move_right();
        field.move_right();
        field.insert('X');
        assert_eq!(field.text(), "héXllo");
        assert!(field.backspace());
        assert_eq!(field.text(), "héllo");
    }

    #[test]
    fn word_jumps_stop_at_boundaries() {
        let mut field = TextField::new("foo bar-baz");
        field.move_word_left();
        assert_eq!(field.cursor(), 8);
        field.move_word_left();
        assert_eq!(field.cursor(), 4);
        field.move_home();
        field.move_word_right();
        assert_eq!(field.cursor(), 3);
        field.move_word_right();
        assert_eq!(field.cursor(), 7);
    }

    #[test]
    fn word_deletes_eat_whole_words() {
        let mut field = TextField::new("foo bar");
        assert!(field.delete_word_before());
        assert_eq!(field.text(), "foo ");
        assert!(field.delete_word_before());
        assert_eq!(field.text(), "");
        assert!(!field.delete_word_before());

        let mut field = TextField::new("foo bar");
        field.move_home();
        assert!(field.delete_word_after());
        assert_eq!(field.text(), " bar");
        assert!(field.delete_word_after());
        assert_eq!(field.text(), "");
        assert!(!field.delete_word_after());
    }

    #[test]
    fn wrap_keeps_words_whole() {
        let field = TextField::new("hello world again");
        assert_eq!(field.wrapped(10, true).lines(), ["hello", "world", "again"]);
    }

    #[test]
    fn wrap_hard_breaks_words_longer_than_the_line() {
        let field = TextField::new("abcdefgh");
        assert_eq!(field.wrapped(3, true).lines(), ["abc", "def", "gh"]);
    }

    #[test]
    fn wrap_collapses_the_space_it_breaks_on() {
        let field = TextField::new("aa bb");
        assert_eq!(field.wrapped(2, false).lines(), ["aa", "bb"]);
    }

    #[test]
    fn caret_moves_to_a_new_line_at_exact_width() {
        let field = TextField::new("abcd");
        let wrapped = field.wrapped(4, true);
        assert_eq!(wrapped.lines(), ["abcd", ""]);
        assert_eq!(wrapped.caret(), (1, 0));
    }

    #[test]
    fn click_maps_to_the_wrapped_line_and_column() {
        let field = TextField::new("hello world");
        let wrapped = field.wrapped(5, true);
        assert_eq!(wrapped.index_at(1, 2), 8);
        assert_eq!(wrapped.index_at(0, 99), 5);
    }

    #[test]
    fn scroll_follows_the_caret_through_wrapped_lines() {
        let field = TextField::new("aa bb cc d");
        let wrapped = field.wrapped(2, true);
        assert_eq!(wrapped.lines(), ["aa", "bb", "cc", "d"]);
        assert_eq!(wrapped.caret(), (3, 1));
        assert_eq!(wrapped.scroll(2), 2);
    }
}
