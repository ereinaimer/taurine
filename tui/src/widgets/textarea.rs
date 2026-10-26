/// Multiline text area: lines plus a (row, column) caret in character
/// units, with an optional selection anchor, word ops, indent, and a
/// bounded undo ring. Sibling of the single-line `field` module;
/// rendering and scrolling stay at each call site, this owns state
/// and caret math only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TextArea {
    lines: Vec<String>,
    row: usize,
    col: usize,
    anchor: Option<(usize, usize)>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

/// One undo step: full text plus caret. Snippet bodies are small, so
/// whole snapshots stay trivial.
/// honey: O(n) memory per keystroke, fine for snippet sizes; shrink
/// the cap if this ever backs a large document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Snapshot {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

/// Undo depth: per-keystroke history within the edit session.
const UNDO_CAP: usize = 100;

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
        Self {
            lines,
            row,
            col,
            anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
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

    /// Ordered selection endpoints, anchor first when it trails.
    pub(crate) fn selection_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let anchor = self.anchor?;
        let caret = (self.row, self.col);
        if anchor == caret {
            return None;
        }
        Some(if anchor < caret {
            (anchor, caret)
        } else {
            (caret, anchor)
        })
    }

    pub(crate) fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// Select the whole body, caret parked at the end.
    pub(crate) fn select_all(&mut self) {
        self.anchor = Some((0, 0));
        self.row = self.lines.len().saturating_sub(1);
        self.col = self.lines[self.row].chars().count();
    }

    /// Collapse any selection, keeping the caret.
    pub(crate) fn collapse(&mut self) {
        self.anchor = None;
    }

    /// Selected text across the ordered range, lines joined.
    pub(crate) fn selected_text(&self) -> Option<String> {
        let ((start_row, start_col), (end_row, end_col)) = self.selection_range()?;
        let mut out = String::new();
        for row in start_row..=end_row {
            let line = &self.lines[row];
            let from = if row == start_row { start_col } else { 0 };
            let to = if row == end_row {
                end_col
            } else {
                line.chars().count()
            };
            let start = byte_index(line, from);
            let end = byte_index(line, to);
            out.push_str(&line[start..end]);
            if row != end_row {
                out.push('\n');
            }
        }
        Some(out)
    }

    fn push_undo(&mut self) {
        self.redo.clear();
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            row: self.row,
            col: self.col,
        });
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.lines = snapshot.lines;
        self.row = snapshot.row.min(self.lines.len().saturating_sub(1));
        self.anchor = None;
        self.clamp_col();
        self.col = snapshot.col.min(self.lines[self.row].chars().count());
    }

    /// Step back one edit. False when the ring is empty.
    pub(crate) fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot {
            lines: std::mem::take(&mut self.lines),
            row: self.row,
            col: self.col,
        });
        self.restore(snapshot);
        true
    }

    /// Step forward after undo. False when nothing was undone.
    pub(crate) fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot {
            lines: std::mem::take(&mut self.lines),
            row: self.row,
            col: self.col,
        });
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
        self.restore(snapshot);
        true
    }

    /// Delete the selection, parking the caret at its start.
    /// Returns false when there is nothing selected.
    pub(crate) fn delete_selection(&mut self) -> bool {
        let Some(((start_row, start_col), (end_row, end_col))) = self.selection_range() else {
            return false;
        };
        self.push_undo();
        self.delete_range(start_row, start_col, end_row, end_col);
        true
    }

    fn delete_range(&mut self, start_row: usize, start_col: usize, end_row: usize, end_col: usize) {
        let head = self.lines[start_row]
            .chars()
            .take(start_col)
            .collect::<String>();
        let tail: String = self.lines[end_row].chars().skip(end_col).collect();
        self.lines
            .splice(start_row..=end_row, [format!("{head}{tail}")]);
        self.row = start_row;
        self.col = start_col;
        self.anchor = None;
    }

    /// Replace the selection first so every mutating op stays whole.
    fn replace_selection(&mut self) {
        if self.has_selection()
            && let Some(((start_row, start_col), (end_row, end_col))) = self.selection_range()
        {
            self.delete_range(start_row, start_col, end_row, end_col);
        }
    }

    fn clamp_col(&mut self) {
        let len = self.lines[self.row].chars().count();
        self.col = self.col.min(len);
    }

    pub(crate) fn insert_char(&mut self, ch: char) {
        self.push_undo();
        self.replace_selection();
        self.raw_insert_char(ch);
    }

    fn raw_insert_char(&mut self, ch: char) {
        let byte = byte_index(&self.lines[self.row], self.col);
        self.lines[self.row].insert(byte, ch);
        self.col = self.col.saturating_add(1);
    }

    /// Insert pasted text as one undo step, splitting newlines.
    pub(crate) fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.push_undo();
        self.replace_selection();
        let mut chunks = text.split('\n');
        if let Some(first) = chunks.next() {
            for ch in first.chars() {
                self.raw_insert_char(ch);
            }
        }
        for chunk in chunks {
            self.raw_insert_newline();
            for ch in chunk.chars() {
                self.raw_insert_char(ch);
            }
        }
    }

    /// Split the current line at the caret.
    pub(crate) fn insert_newline(&mut self) {
        self.push_undo();
        self.replace_selection();
        self.raw_insert_newline();
    }

    fn raw_insert_newline(&mut self) {
        let byte = byte_index(&self.lines[self.row], self.col);
        let tail = self.lines[self.row][byte..].to_string();
        self.lines[self.row].truncate(byte);
        self.row = self.row.saturating_add(1);
        self.lines.insert(self.row, tail);
        self.col = 0;
    }

    /// Delete behind the caret, joining with the previous line at column 0.
    /// A selection deletes whole instead. False when already at the very start.
    pub(crate) fn backspace(&mut self) -> bool {
        if self.has_selection() {
            return self.delete_selection();
        }
        if self.col > 0 {
            self.push_undo();
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
        self.push_undo();
        let tail = self.lines.remove(self.row);
        self.row = self.row.saturating_sub(1);
        self.col = self.lines[self.row].chars().count();
        self.lines[self.row].push_str(&tail);
        true
    }

    /// Delete at the caret, joining with the next line at line end.
    /// A selection deletes whole instead. False when already at the very end.
    pub(crate) fn delete_at(&mut self) -> bool {
        if self.has_selection() {
            return self.delete_selection();
        }
        let len = self.lines[self.row].chars().count();
        if self.col < len {
            self.push_undo();
            let line = &mut self.lines[self.row];
            let start = byte_index(line, self.col);
            let end = byte_index(line, self.col + 1);
            line.drain(start..end);
            return true;
        }
        if self.row + 1 >= self.lines.len() {
            return false;
        }
        self.push_undo();
        let tail = self.lines.remove(self.row + 1);
        self.lines[self.row].push_str(&tail);
        true
    }

    /// Delete back over the previous word (spaces, then word chars),
    /// joining lines at column 0 like backspace.
    pub(crate) fn delete_word_before(&mut self) -> bool {
        if self.has_selection() {
            return self.delete_selection();
        }
        let (row, col) = self.word_start();
        if row == self.row && col == self.col {
            return false;
        }
        self.push_undo();
        if row == self.row {
            let line = &mut self.lines[self.row];
            line.drain(byte_index(line, col)..byte_index(line, self.col));
            self.col = col;
        } else {
            self.delete_range(row, col, self.row, self.col);
        }
        true
    }

    /// Delete forward over the next word, joining lines at line end.
    pub(crate) fn delete_word_after(&mut self) -> bool {
        if self.has_selection() {
            return self.delete_selection();
        }
        let (row, col) = self.word_end();
        if row == self.row && col == self.col {
            return false;
        }
        self.push_undo();
        if row == self.row {
            let line = &mut self.lines[self.row];
            line.drain(byte_index(line, self.col)..byte_index(line, col));
        } else {
            self.delete_range(self.row, self.col, row, col);
        }
        true
    }

    fn is_word_char(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    /// Start of the previous word: skip separators, then word chars.
    /// Stops at the previous line end when the caret leads the line.
    fn word_start(&self) -> (usize, usize) {
        let (row, col) = (self.row, self.col);
        if col == 0 {
            if row == 0 {
                return (0, 0);
            }
            let prev = row.saturating_sub(1);
            return (prev, self.lines[prev].chars().count());
        }
        let chars: Vec<char> = self.lines[row].chars().collect();
        let mut index = col;
        while index > 0 && !Self::is_word_char(chars[index - 1]) {
            index -= 1;
        }
        while index > 0 && Self::is_word_char(chars[index - 1]) {
            index -= 1;
        }
        if index == col {
            // honey: pure separators with no word behind still eat one
            // step back, never stalls.
            index = col.saturating_sub(1);
        }
        (row, index)
    }

    /// End of the next word: skip separators, then word chars. Stops
    /// at line end, joining on the next invocation like delete_at.
    fn word_end(&self) -> (usize, usize) {
        let (row, col) = (self.row, self.col);
        let len = self.lines[row].chars().count();
        if col >= len {
            return (row, col);
        }
        let chars: Vec<char> = self.lines[row].chars().collect();
        let mut index = col;
        while index < len && !Self::is_word_char(chars[index]) {
            index += 1;
        }
        while index < len && Self::is_word_char(chars[index]) {
            index += 1;
        }
        if index == col {
            index = (col + 1).min(len);
        }
        (row, index)
    }

    /// Indent the selected lines (or the current one) by two spaces.
    pub(crate) fn indent(&mut self) {
        let (from, to) = self.indent_range();
        self.push_undo();
        for line in &mut self.lines[from..=to] {
            line.insert_str(0, "  ");
        }
        self.col = self.col.saturating_add(2);
        if let Some((anchor_row, anchor_col)) = self.anchor {
            self.anchor = Some((anchor_row, anchor_col.saturating_add(2)));
        }
    }

    /// Outdent the selected lines (or the current one) by up to two spaces.
    pub(crate) fn outdent(&mut self) {
        let (from, to) = self.indent_range();
        self.push_undo();
        for line in &mut self.lines[from..=to] {
            let drop: usize = line.chars().take(2).take_while(|ch| *ch == ' ').count();
            let bytes: usize = line.chars().take(drop).map(char::len_utf8).sum();
            line.drain(..bytes);
        }
        self.clamp_col();
        if let Some((anchor_row, anchor_col)) = self.anchor {
            let len = self.lines[anchor_row.min(self.lines.len() - 1)]
                .chars()
                .count();
            self.anchor = Some((anchor_row, anchor_col.min(len)));
        }
    }

    fn indent_range(&self) -> (usize, usize) {
        match self.selection_range() {
            Some(((start_row, _), (end_row, end_col))) => {
                // honey: a caret parked at column 0 excludes the last
                // line, mirroring VS Code's indent scope.
                let end = if end_col == 0 && end_row > start_row {
                    end_row.saturating_sub(1)
                } else {
                    end_row
                };
                (start_row, end)
            }
            None => (self.row, self.row),
        }
    }

    fn track_anchor(&mut self, extend: bool) {
        if extend {
            self.anchor.get_or_insert((self.row, self.col));
        } else {
            self.anchor = None;
        }
    }

    pub(crate) fn move_left(&mut self, extend: bool) {
        self.track_anchor(extend);
        if self.col > 0 {
            self.col = self.col.saturating_sub(1);
        } else if self.row > 0 {
            self.row = self.row.saturating_sub(1);
            self.col = self.lines[self.row].chars().count();
        }
    }

    pub(crate) fn move_right(&mut self, extend: bool) {
        self.track_anchor(extend);
        if self.col < self.lines[self.row].chars().count() {
            self.col = self.col.saturating_add(1);
        } else if self.row + 1 < self.lines.len() {
            self.row = self.row.saturating_add(1);
            self.col = 0;
        }
    }

    pub(crate) fn move_up(&mut self, extend: bool) {
        self.track_anchor(extend);
        if self.row > 0 {
            self.row = self.row.saturating_sub(1);
            self.clamp_col();
        }
    }

    pub(crate) fn move_down(&mut self, extend: bool) {
        self.track_anchor(extend);
        if self.row + 1 < self.lines.len() {
            self.row = self.row.saturating_add(1);
            self.clamp_col();
        }
    }

    pub(crate) fn move_home(&mut self, extend: bool) {
        self.track_anchor(extend);
        self.col = 0;
    }

    pub(crate) fn move_end(&mut self, extend: bool) {
        self.track_anchor(extend);
        self.col = self.lines[self.row].chars().count();
    }

    /// File ends, mirroring Ctrl+Home/End.
    pub(crate) fn move_top(&mut self, extend: bool) {
        self.track_anchor(extend);
        self.row = 0;
        self.clamp_col();
    }

    pub(crate) fn move_bottom(&mut self, extend: bool) {
        self.track_anchor(extend);
        self.row = self.lines.len().saturating_sub(1);
        self.clamp_col();
    }

    pub(crate) fn move_word_left(&mut self, extend: bool) {
        self.track_anchor(extend);
        let (row, col) = self.word_start();
        self.row = row;
        self.col = col;
    }

    pub(crate) fn move_word_right(&mut self, extend: bool) {
        self.track_anchor(extend);
        let (row, col) = self.word_end();
        self.row = row;
        self.col = col;
    }

    /// Caret placement, e.g. from a click. Clears any selection and
    /// clamps past the end.
    pub(crate) fn place(&mut self, row: usize, col: usize) {
        self.row = row.min(self.lines.len().saturating_sub(1));
        self.clamp_col();
        self.col = col.min(self.lines[self.row].chars().count());
        self.collapse();
    }

    /// Anchor a mouse drag: caret and selection origin together, so a
    /// bare press shows no selection until the pointer moves.
    pub(crate) fn begin_select(&mut self, row: usize, col: usize) {
        self.row = row.min(self.lines.len().saturating_sub(1));
        self.clamp_col();
        self.col = col.min(self.lines[self.row].chars().count());
        self.anchor = Some((self.row, self.col));
    }

    /// Stretch a drag selection to the pointer, keeping the origin.
    /// A missing origin (no press seen) falls back to the old caret,
    /// mirroring the keyboard extend path.
    pub(crate) fn extend_select(&mut self, row: usize, col: usize) {
        let caret = (self.row, self.col);
        self.anchor.get_or_insert(caret);
        self.row = row.min(self.lines.len().saturating_sub(1));
        self.clamp_col();
        self.col = col.min(self.lines[self.row].chars().count());
    }

    /// Select the word under (row, col), e.g. on double-click. A probe
    /// on a separator collapses instead of grabbing neighbors.
    pub(crate) fn select_word_at(&mut self, row: usize, col: usize) {
        self.row = row.min(self.lines.len().saturating_sub(1));
        self.clamp_col();
        self.col = col.min(self.lines[self.row].chars().count());
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        let probe = if self.col < chars.len() {
            self.col
        } else {
            self.col.saturating_sub(1)
        };
        if chars.get(probe).is_none_or(|ch| !Self::is_word_char(*ch)) {
            self.collapse();
            return;
        }
        let mut start = probe;
        while start > 0 && Self::is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = probe.saturating_add(1);
        while end < chars.len() && Self::is_word_char(chars[end]) {
            end += 1;
        }
        self.anchor = Some((self.row, start));
        self.col = end;
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
        area.move_left(false);
        area.move_left(false);
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
        end.move_end(false);
        assert!(!end.delete_at());
    }

    #[test]
    fn arrows_walk_across_line_ends() {
        let mut area = TextArea::new("ab\nc");
        area.move_home(false);
        assert_eq!(area.cursor(), (1, 0));
        area.move_left(false);
        assert_eq!(area.cursor(), (0, 2));
        area.move_left(false);
        area.move_home(false);
        area.move_left(false);
        assert_eq!(area.cursor(), (0, 0));
        area.move_end(false);
        area.move_right(false);
        assert_eq!(area.cursor(), (1, 0));
        area.move_right(false);
        area.move_right(false);
        assert_eq!(area.cursor(), (1, 1));
        area.move_left(false);
        area.move_left(false);
        assert_eq!(area.cursor(), (0, 2));
    }

    #[test]
    fn up_down_clamp_to_short_lines() {
        let mut area = TextArea::new("abcdef\nxy");
        area.place(0, 5);
        area.move_down(false);
        assert_eq!(area.cursor(), (1, 2));
        area.move_up(false);
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
        area.move_home(false);
        area.move_right(false);
        area.move_right(false);
        area.insert_char('X');
        assert_eq!(area.text(), "héXllo");
        assert!(area.backspace());
        assert_eq!(area.text(), "héllo");
    }

    #[test]
    fn select_all_covers_body() {
        let mut area = TextArea::new("ab\ncdef");
        assert!(!area.has_selection());
        area.select_all();
        assert!(area.has_selection());
        assert_eq!(area.selected_text().as_deref(), Some("ab\ncdef"));
        assert_eq!(area.cursor(), (1, 4));
    }

    #[test]
    fn shift_extends_and_move_collapses() {
        let mut area = TextArea::new("abcdef");
        area.place(0, 2);
        area.move_right(true);
        area.move_right(true);
        assert_eq!(area.selected_text().as_deref(), Some("cd"));
        area.move_right(false);
        assert!(!area.has_selection());
        assert_eq!(area.cursor(), (0, 5));
    }

    #[test]
    fn typing_replaces_selection() {
        let mut area = TextArea::new("abcdef");
        area.place(0, 1);
        area.move_right(true);
        area.move_right(true);
        area.insert_char('X');
        assert_eq!(area.text(), "aXdef");
        assert!(!area.has_selection());
    }

    #[test]
    fn word_jumps_stop_at_boundaries() {
        let mut area = TextArea::new("foo bar-baz  qux");
        area.move_end(false);
        area.move_word_left(false);
        assert_eq!(area.cursor(), (0, 13));
        area.move_word_left(false);
        assert_eq!(area.cursor(), (0, 8));
        area.move_word_left(false);
        assert_eq!(area.cursor(), (0, 4));
        area.move_home(false);
        area.move_word_right(false);
        assert_eq!(area.cursor(), (0, 3));
    }

    #[test]
    fn ctrl_backspace_deletes_previous_word() {
        let mut area = TextArea::new("foo bar");
        area.move_end(false);
        assert!(area.delete_word_before());
        assert_eq!(area.text(), "foo ");
        assert!(area.delete_word_before());
        assert_eq!(area.text(), "");
        assert!(!area.delete_word_before());
    }

    #[test]
    fn ctrl_delete_deletes_next_word() {
        let mut area = TextArea::new("foo bar");
        area.move_home(false);
        assert!(area.delete_word_after());
        assert_eq!(area.text(), " bar");
        assert!(area.delete_word_after());
        assert_eq!(area.text(), "");
        assert!(!area.delete_word_after());
    }

    #[test]
    fn indent_outdent_round_trip() {
        let mut area = TextArea::new("a\nb");
        area.select_all();
        area.indent();
        assert_eq!(area.text(), "  a\n  b");
        area.outdent();
        assert_eq!(area.text(), "a\nb");
        area.outdent();
        assert_eq!(area.text(), "a\nb");
    }

    #[test]
    fn undo_redo_walks_keystrokes() {
        let mut area = TextArea::new("ab");
        area.move_end(false);
        area.insert_char('c');
        assert_eq!(area.text(), "abc");
        assert!(area.undo());
        assert_eq!(area.text(), "ab");
        assert!(area.redo());
        assert_eq!(area.text(), "abc");
        assert!(area.undo());
        assert_eq!(area.text(), "ab");
        assert!(!area.undo());
    }

    #[test]
    fn insert_text_pastes_as_one_step() {
        let mut area = TextArea::new("ab");
        area.move_end(false);
        area.insert_text("X\nYZ");
        assert_eq!(area.text(), "abX\nYZ");
        assert!(area.undo());
        assert_eq!(area.text(), "ab");
    }

    #[test]
    fn drag_selects_from_anchor_to_pointer() {
        let mut area = TextArea::new("abc\ndef");
        area.begin_select(0, 1);
        assert!(!area.has_selection());
        area.extend_select(1, 2);
        assert_eq!(area.selected_text().as_deref(), Some("bc\nde"));
        assert_eq!(area.cursor(), (1, 2));
    }

    #[test]
    fn extend_without_anchor_starts_from_caret() {
        let mut area = TextArea::new("abcdef");
        area.place(0, 2);
        area.extend_select(0, 5);
        assert_eq!(area.selected_text().as_deref(), Some("cde"));
    }

    #[test]
    fn double_click_selects_word_only() {
        let mut area = TextArea::new("foo bar baz");
        area.select_word_at(0, 5);
        assert_eq!(area.selected_text().as_deref(), Some("bar"));
        area.select_word_at(0, 3);
        assert!(!area.has_selection());
        area.select_word_at(0, 11);
        assert_eq!(area.selected_text().as_deref(), Some("baz"));
    }
}
