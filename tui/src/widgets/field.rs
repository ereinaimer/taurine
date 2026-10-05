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
}
