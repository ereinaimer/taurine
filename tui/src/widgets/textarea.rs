/// Multiline text area: lines plus a (row, column) caret in character
/// units. Sibling of the single-line `field` module; rendering and
/// scrolling stay at each call site, this owns state and caret math only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TextArea {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

impl TextArea {
    pub(crate) fn new(text: &str) -> Self {
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        // honey: a trailing newline is a cursor row of its own, so an
        // edit round-trips the exact text including the final break.
        if text.ends_with('\n') {
            lines.push(String::new());
        }
        // honey: trailing newline is a cursor position, not a phantom row.
        let row = lines.len().saturating_sub(1);
        let col = lines[row].chars().count();
        Self { lines, row, col }
    }

    pub(crate) fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }

    pub(crate) const fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    fn clamp_col(&mut self) {
        let len = self.lines[self.row].chars().count();
        self.col = self.col.min(len);
    }

    pub(crate) fn insert_char(&mut self, ch: char) {
        let byte = byte_index(&self.lines[self.row], self.col);
        self.lines[self.row].insert(byte, ch);
        self.col = self.col.saturating_add(1);
    }

    /// Split the current line at the caret.
    pub(crate) fn insert_newline(&mut self) {
        let byte = byte_index(&self.lines[self.row], self.col);
        let tail = self.lines[self.row][byte..].to_string();
        self.lines[self.row].truncate(byte);
        self.row = self.row.saturating_add(1);
        self.lines.insert(self.row, tail);
        self.col = 0;
    }

    /// Delete behind the caret, joining with the previous line at column 0.
    /// False when already at the very start.
    pub(crate) fn backspace(&mut self) -> bool {
        if self.col > 0 {
            let line = &mut self.lines[self.row];
            let end = byte_index(line, self.col);
            let start = byte_index(line, self.col - 1);
            line.drain(start..end);
            self.col = self.col.saturating_sub(1);
            return true;
        }
        if self.row == 0 {
            return false;
        }
        let tail = self.lines.remove(self.row);
        self.row = self.row.saturating_sub(1);
        self.col = self.lines[self.row].chars().count();
        self.lines[self.row].push_str(&tail);
        true
    }

    /// Delete at the caret, joining with the next line at line end.
    /// False when already at the very end.
    pub(crate) fn delete_at(&mut self) -> bool {
        let len = self.lines[self.row].chars().count();
        if self.col < len {
            let line = &mut self.lines[self.row];
            let start = byte_index(line, self.col);
            let end = byte_index(line, self.col + 1);
            line.drain(start..end);
            return true;
        }
        if self.row + 1 >= self.lines.len() {
            return false;
        }
        let tail = self.lines.remove(self.row + 1);
        self.lines[self.row].push_str(&tail);
        true
    }

    pub(crate) fn move_left(&mut self) {
        if self.col > 0 {
            self.col = self.col.saturating_sub(1);
        } else if self.row > 0 {
            self.row = self.row.saturating_sub(1);
            self.col = self.lines[self.row].chars().count();
        }
    }

    pub(crate) fn move_right(&mut self) {
        if self.col < self.lines[self.row].chars().count() {
            self.col = self.col.saturating_add(1);
        } else if self.row + 1 < self.lines.len() {
            self.row = self.row.saturating_add(1);
            self.col = 0;
        }
    }

    pub(crate) fn move_up(&mut self) {
        if self.row > 0 {
            self.row = self.row.saturating_sub(1);
            self.clamp_col();
        }
    }

    pub(crate) fn move_down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row = self.row.saturating_add(1);
            self.clamp_col();
        }
    }

    pub(crate) fn move_home(&mut self) {
        self.col = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.col = self.lines[self.row].chars().count();
    }

    /// Caret placement, e.g. from a click. Clamps past the end.
    pub(crate) fn place(&mut self, row: usize, col: usize) {
        self.row = row.min(self.lines.len().saturating_sub(1));
        self.clamp_col();
        self.col = col.min(self.lines[self.row].chars().count());
    }
}

fn byte_index(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(value.len())
}

#[cfg(test)]
mod tests {
    use super::TextArea;

    #[test]
    fn new_parks_caret_at_text_end() {
        let area = TextArea::new("ab\ncdef");
        assert_eq!(area.cursor(), (1, 4));
        assert_eq!(area.text(), "ab\ncdef");
    }

    #[test]
    fn empty_and_trailing_newline_shapes() {
        assert_eq!(TextArea::new("").cursor(), (0, 0));
        let area = TextArea::new("ab\n");
        assert_eq!(area.cursor(), (1, 0));
        assert_eq!(area.text(), "ab\n");
    }

    #[test]
    fn newline_splits_and_backspace_joins() {
        let mut area = TextArea::new("abcd");
        area.move_left();
        area.move_left();
        area.insert_newline();
        assert_eq!(area.lines(), &["ab".to_string(), "cd".to_string()]);
        assert_eq!(area.cursor(), (1, 0));
        assert!(area.backspace());
        assert_eq!(area.text(), "abcd");
        assert_eq!(area.cursor(), (0, 2));
    }

    #[test]
    fn delete_at_joins_with_next_line() {
        let mut area = TextArea::new("ab\ncd");
        area.place(0, 2);
        assert!(area.delete_at());
        assert_eq!(area.text(), "abcd");
        assert_eq!(area.cursor(), (0, 2));
        let mut end = TextArea::new("ab");
        end.move_end();
        assert!(!end.delete_at());
    }

    #[test]
    fn arrows_walk_across_line_ends() {
        let mut area = TextArea::new("ab\nc");
        area.move_home();
        assert_eq!(area.cursor(), (1, 0));
        area.move_left();
        assert_eq!(area.cursor(), (0, 2));
        area.move_left();
        area.move_home();
        area.move_left();
        assert_eq!(area.cursor(), (0, 0));
        area.move_end();
        area.move_right();
        assert_eq!(area.cursor(), (1, 0));
        area.move_right();
        area.move_right();
        assert_eq!(area.cursor(), (1, 1));
        area.move_left();
        area.move_left();
        assert_eq!(area.cursor(), (0, 2));
    }

    #[test]
    fn up_down_clamp_to_short_lines() {
        let mut area = TextArea::new("abcdef\nxy");
        area.place(0, 5);
        area.move_down();
        assert_eq!(area.cursor(), (1, 2));
        area.move_up();
        assert_eq!(area.cursor(), (0, 2));
    }

    #[test]
    fn place_clamps_past_end() {
        let mut area = TextArea::new("ab\ncdef");
        area.place(99, 99);
        assert_eq!(area.cursor(), (1, 4));
        area.place(0, 1);
        assert_eq!(area.cursor(), (0, 1));
    }

    #[test]
    fn multibyte_insert_delete() {
        let mut area = TextArea::new("héllo");
        area.move_home();
        area.move_right();
        area.move_right();
        area.insert_char('X');
        assert_eq!(area.text(), "héXllo");
        assert!(area.backspace());
        assert_eq!(area.text(), "héllo");
    }
}
