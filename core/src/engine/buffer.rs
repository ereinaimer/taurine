const FAST_BUFFER_CAPACITY: usize = 512;

#[derive(Debug, Clone)]
pub struct FastBuffer {
    pub(crate) data: [char; FAST_BUFFER_CAPACITY],
    pub(crate) head: usize,
    pub(crate) len: usize,
}

impl Default for FastBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl FastBuffer {
    pub fn new() -> Self {
        Self {
            data: ['\0'; FAST_BUFFER_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, c: char) {
        self.data[self.head] = c;
        self.head = (self.head + 1) % FAST_BUFFER_CAPACITY;
        if self.len < FAST_BUFFER_CAPACITY {
            self.len += 1;
        }
    }

    pub fn pop(&mut self) {
        if self.len > 0 {
            self.head = (self.head + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
            self.len -= 1;
        }
    }

    pub fn pop_n(&mut self, count: usize) {
        for _ in 0..count {
            self.pop();
        }
    }

    pub fn pop_word(&mut self) {
        if self.len == 0 {
            return;
        }

        // 1. Pop trailing whitespace
        while self.len > 0 {
            let curr = (self.head + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
            if self.data[curr].is_whitespace() {
                self.pop();
            } else {
                break;
            }
        }

        if self.len == 0 {
            return;
        }

        // 2. Determine class of the last character
        let curr = (self.head + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
        let start_char = self.data[curr];
        let is_alphanumeric = start_char.is_alphanumeric();

        // 3. Pop all characters of the same class
        while self.len > 0 {
            let curr = (self.head + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
            let c = self.data[curr];

            if c.is_whitespace() {
                break;
            }

            if c.is_alphanumeric() == is_alphanumeric {
                self.pop();
            } else {
                break;
            }
        }
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    fn count_consecutive_backslashes_before(&self, mut curr: usize, mut available: usize) -> usize {
        let mut count = 0;
        while available > 0 {
            if self.data[curr] == '\\' {
                count += 1;
                curr = (curr + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
                available -= 1;
            } else {
                break;
            }
        }
        count
    }

    /// Counts `trigger_char` only in the maximal suffix of consecutive non-whitespace characters.
    ///
    /// Using the whole ring buffer would false-reject valid triggers when an older trigger
    /// character still sits in history (e.g. prose like `x > y` then a new `>gm` after a space).
    /// Chained triggers without whitespace (e.g. `>brb>gm`) still see two counts and are rejected.
    fn trigger_char_count_in_nonwhitespace_suffix(&self, trigger_char: char) -> usize {
        if self.len == 0 {
            return 0;
        }
        let mut count = 0;
        let mut curr = (self.head + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
        let mut n = 0;
        let mut active_quote: Option<char> = None;

        while n < self.len {
            let c = self.data[curr];

            let is_escaped = if c == '"' || c == '\'' {
                let prev_curr = (curr + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
                let available = self.len - n - 1;
                !self
                    .count_consecutive_backslashes_before(prev_curr, available)
                    .is_multiple_of(2)
            } else {
                false
            };

            if (c == '"' || c == '\'') && !is_escaped {
                match active_quote {
                    Some(q) if q == c => active_quote = None,
                    None => active_quote = Some(c),
                    _ => {}
                }
            } else if c.is_whitespace() && active_quote.is_none() {
                break;
            } else if c == trigger_char && active_quote.is_none() {
                count += 1;
            }

            curr = (curr + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
            n += 1;
        }
        count
    }

    /// Walks backwards from the head. Stops and aborts if it hits whitespace.
    /// If it hits `trigger_char`, extracts the sequence between `trigger_char` and the head.
    ///
    /// If the non-whitespace suffix contains more than one trigger (e.g. `>brb>gm`), expansion is
    /// ambiguous and we return `None` so the user does not get a partial delete + wrong paste.
    pub fn extract_trigger_word(&self, trigger_char: char) -> Option<String> {
        if self.len == 0 {
            return None;
        }

        if self.trigger_char_count_in_nonwhitespace_suffix(trigger_char) > 1 {
            return None;
        }

        let mut collected = Vec::new();
        let mut curr = (self.head + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
        let mut n = 0;
        let mut active_quote: Option<char> = None;

        while n < self.len {
            let c = self.data[curr];

            let is_escaped = if c == '"' || c == '\'' {
                let prev_curr = (curr + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
                let available = self.len - n - 1;
                !self
                    .count_consecutive_backslashes_before(prev_curr, available)
                    .is_multiple_of(2)
            } else {
                false
            };

            if (c == '"' || c == '\'') && !is_escaped {
                match active_quote {
                    Some(q) if q == c => {
                        active_quote = None;
                        collected.push(c);
                    }
                    None => {
                        active_quote = Some(c);
                        collected.push(c);
                    }
                    Some(_) => {
                        collected.push(c);
                    }
                }
            } else if c.is_whitespace() && active_quote.is_none() {
                // Invalid sequence, space found before trigger char
                return None;
            } else if c == trigger_char && active_quote.is_none() {
                // We've found the trigger char. The keyword is everything after it.
                collected.reverse();
                return Some(collected.into_iter().collect());
            } else {
                collected.push(c);
            }

            curr = (curr + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
            n += 1;
        }

        None
    }

    /// Extracts the trailing word from the buffer for triggerless expansion.
    ///
    /// Walks backward from the head, collecting non-whitespace characters until it
    /// hits a whitespace character or the start of the buffer. Returns the collected
    /// word, or `None` if the buffer is empty or the tail contains only whitespace.
    ///
    /// **Boundary rule**: The word must be immediately preceded by whitespace or be at
    /// the absolute start of the buffer. Punctuation is NOT treated as a boundary —
    /// it becomes part of the word. For example, `(gs` extracts as `(gs`, which will
    /// not match a trigger named `gs`.
    pub fn extract_tail_word(&self) -> Option<String> {
        if self.len == 0 {
            return None;
        }

        let mut collected = Vec::new();
        let mut curr = (self.head + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
        let mut n = 0;

        while n < self.len {
            let c = self.data[curr];

            if c.is_whitespace() {
                break;
            }

            collected.push(c);
            curr = (curr + FAST_BUFFER_CAPACITY - 1) % FAST_BUFFER_CAPACITY;
            n += 1;
        }

        if collected.is_empty() {
            return None;
        }

        collected.reverse();
        Some(collected.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{FAST_BUFFER_CAPACITY, FastBuffer};

    fn type_str(buf: &mut FastBuffer, s: &str) {
        for c in s.chars() {
            buf.push(c);
        }
    }

    #[test]
    fn extract_trigger_word_single_trigger_suffix() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "prefix>gm");
        assert_eq!(b.extract_trigger_word('>'), Some("gm".to_string()));
    }

    #[test]
    fn extract_trigger_word_two_triggers_returns_none_prevents_partial_delete() {
        let mut b = FastBuffer::new();
        type_str(&mut b, ">brb>gm");
        assert_eq!(
            b.extract_trigger_word('>'),
            None,
            "simulates user typing >brb>gm without finishing first expansion — must not match a keyword with wrong delete span"
        );
    }

    #[test]
    fn extract_trigger_word_whitespace_before_trigger_token_aborts() {
        let mut b = FastBuffer::new();
        // Walk backward: 'm', then space — cannot reach `>` without crossing whitespace.
        type_str(&mut b, ">g m");
        assert_eq!(b.extract_trigger_word('>'), None);
    }

    #[test]
    fn extract_trigger_word_no_trigger_in_suffix() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "plain");
        assert_eq!(b.extract_trigger_word('>'), None);
    }

    #[test]
    fn two_triggers_in_suffix_still_rejected_after_ring_wrap() {
        let mut b = FastBuffer::new();
        for _ in 0..(FAST_BUFFER_CAPACITY - 2) {
            b.push('a');
        }
        type_str(&mut b, ">x>y");
        assert_eq!(b.extract_trigger_word('>'), None);
    }

    #[test]
    fn older_trigger_separated_by_whitespace_does_not_block_new_trigger() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "note: x > y and then >gm");
        assert_eq!(b.extract_trigger_word('>'), Some("gm".to_string()));
    }

    #[test]
    fn extract_trigger_word_quote_aware_whitespace() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">gfb-"my branch""#);
        assert_eq!(
            b.extract_trigger_word('>'),
            Some(r#"gfb-"my branch""#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_ignores_trigger_chars_inside_quotes() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">echo-">>>""#);
        assert_eq!(
            b.extract_trigger_word('>'),
            Some(r#"echo-">>>""#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_handles_backslash_escaped_quotes_inside_quotes() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">cmd-"\"echo\"""#);
        assert_eq!(
            b.extract_trigger_word('>'),
            Some(r#"cmd-"\"echo\"""#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_handles_escaped_backslashes_before_quotes() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">cmd-"ab\\""#);
        assert_eq!(
            b.extract_trigger_word('>'),
            Some(r#"cmd-"ab\\""#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_single_quote_aware_whitespace() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">search-'Neil Armstrong'"#);
        assert_eq!(
            b.extract_trigger_word('>'),
            Some(r#"search-'Neil Armstrong'"#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_abort_on_unopened_quote_state() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">foo-"bar"#);
        assert_eq!(b.extract_trigger_word('>'), None);
    }

    #[test]
    fn extract_tail_word_empty_buffer_returns_none() {
        let b = FastBuffer::new();
        assert_eq!(b.extract_tail_word(), None);
    }

    #[test]
    fn extract_tail_word_single_word_at_buffer_start() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "gs");
        assert_eq!(b.extract_tail_word(), Some("gs".to_string()));
    }

    #[test]
    fn extract_tail_word_after_whitespace() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "hello gs");
        assert_eq!(b.extract_tail_word(), Some("gs".to_string()));
    }

    #[test]
    fn extract_tail_word_punctuation_is_not_a_boundary() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "(gs");
        assert_eq!(
            b.extract_tail_word(),
            Some("(gs".to_string()),
            "punctuation should be part of the word, not a boundary"
        );
    }

    #[test]
    fn extract_tail_word_trailing_whitespace_returns_none() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "hello ");
        assert_eq!(b.extract_tail_word(), None);
    }

    #[test]
    fn extract_tail_word_after_tab() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "some\tgs");
        assert_eq!(b.extract_tail_word(), Some("gs".to_string()));
    }

    #[test]
    fn extract_tail_word_unicode_word() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "text ツ");
        assert_eq!(b.extract_tail_word(), Some("ツ".to_string()));
    }
}
