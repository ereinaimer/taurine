const FAST_BUFFER_CAPACITY: usize = 512;

#[derive(Debug, Clone)]
pub struct FastBuffer {
    pub(crate) data: Vec<char>,
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
            data: vec!['\0'; FAST_BUFFER_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    fn grow(&mut self) {
        let old_capacity = self.data.len();
        let new_capacity = old_capacity * 2;
        let mut new_data = vec!['\0'; new_capacity];
        let start = (self.head + old_capacity - self.len) % old_capacity;
        for (i, item) in new_data.iter_mut().take(self.len).enumerate() {
            *item = self.data[(start + i) % old_capacity];
        }
        self.data = new_data;
        self.head = self.len;
    }

    pub fn push(&mut self, c: char) {
        if self.len >= self.data.len() {
            self.grow();
        }

        let capacity = self.data.len();
        let threshold = (capacity * 8) / 10;
        if self.len >= threshold {
            tracing::warn!(
                "FastBuffer capacity warning: {}/{} ({}%) reached",
                self.len,
                capacity,
                (self.len * 100) / capacity
            );
        }

        self.data[self.head] = c;
        self.head = (self.head + 1) % capacity;
        if self.len < capacity {
            self.len += 1;
        }
    }

    pub fn pop(&mut self) {
        if self.len > 0 {
            let capacity = self.data.len();
            self.head = (self.head + capacity - 1) % capacity;
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

        let capacity = self.data.len();

        // 1. Pop trailing whitespace
        while self.len > 0 {
            let curr = (self.head + capacity - 1) % capacity;
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
        let curr = (self.head + capacity - 1) % capacity;
        let start_char = self.data[curr];
        let is_alphanumeric = start_char.is_alphanumeric();

        // 3. Pop all characters of the same class
        while self.len > 0 {
            let curr = (self.head + capacity - 1) % capacity;
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

    pub fn buffer_string(&self) -> String {
        let capacity = self.data.len();
        let mut result = String::with_capacity(self.len);
        let start = (self.head + capacity - self.len) % capacity;
        for i in 0..self.len {
            result.push(self.data[(start + i) % capacity]);
        }
        result
    }

    pub fn is_inside_open_quote(&self) -> bool {
        if self.len == 0 {
            return false;
        }

        let capacity = self.data.len();
        let start = (self.head + capacity - self.len) % capacity;
        let mut active_quote: Option<char> = None;

        for i in 0..self.len {
            let curr = (start + i) % capacity;
            let c = self.data[curr];

            let is_escaped = if c == '"' || c == '\'' {
                let prev_curr = (curr + capacity - 1) % capacity;
                let available = i;
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
            }
        }

        active_quote.is_some()
    }

    fn count_consecutive_backslashes_before(&self, mut curr: usize, mut available: usize) -> usize {
        let capacity = self.data.len();
        let mut count = 0;
        while available > 0 {
            if self.data[curr] == '\\' {
                count += 1;
                curr = (curr + capacity - 1) % capacity;
                available -= 1;
            } else {
                break;
            }
        }
        count
    }

    /// Counts `trigger_char` only in the maximal suffix of consecutive non-whitespace characters.
    fn trigger_char_count_in_nonwhitespace_suffix(
        &self,
        trigger_char: char,
        allow_spaces: bool,
    ) -> usize {
        if self.len == 0 {
            return 0;
        }
        let capacity = self.data.len();
        let mut count = 0;
        let mut curr = (self.head + capacity - 1) % capacity;
        let mut n = 0;
        let mut active_quote: Option<char> = None;

        while n < self.len {
            let c = self.data[curr];

            let is_escaped = if c == '"' || c == '\'' {
                let prev_curr = (curr + capacity - 1) % capacity;
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
                if allow_spaces && c == ' ' && count == 0 {
                    // Allow spaces
                } else {
                    break;
                }
            } else if c == trigger_char && active_quote.is_none() {
                count += 1;
            }

            curr = (curr + capacity - 1) % capacity;
            n += 1;
        }
        count
    }

    pub fn extract_trigger_word(&self, trigger_char: char, allow_spaces: bool) -> Option<String> {
        if self.len == 0 {
            return None;
        }

        if self.trigger_char_count_in_nonwhitespace_suffix(trigger_char, allow_spaces) > 1 {
            return None;
        }

        let capacity = self.data.len();
        let mut collected = Vec::new();
        let mut curr = (self.head + capacity - 1) % capacity;
        let mut n = 0;
        let mut active_quote: Option<char> = None;

        while n < self.len {
            let c = self.data[curr];

            let is_escaped = if c == '"' || c == '\'' {
                let prev_curr = (curr + capacity - 1) % capacity;
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
                if allow_spaces && c == ' ' {
                    collected.push(c);
                } else {
                    return None;
                }
            } else if c == trigger_char && active_quote.is_none() {
                collected.reverse();
                return Some(collected.into_iter().collect());
            } else {
                collected.push(c);
            }

            curr = (curr + capacity - 1) % capacity;
            n += 1;
        }

        None
    }

    pub fn extract_tail_word(&self) -> Option<String> {
        if self.len == 0 {
            return None;
        }

        let capacity = self.data.len();
        let mut collected = Vec::new();
        let mut curr = (self.head + capacity - 1) % capacity;
        let mut n = 0;

        while n < self.len {
            let c = self.data[curr];

            if c.is_whitespace() {
                break;
            }

            collected.push(c);
            curr = (curr + capacity - 1) % capacity;
            n += 1;
        }

        if collected.is_empty() {
            return None;
        }

        collected.reverse();
        Some(collected.into_iter().collect())
    }

    pub fn extract_suffix_candidates(&self) -> Vec<(String, Option<char>)> {
        let mut candidates = Vec::new();
        if self.len == 0 {
            return candidates;
        }

        let capacity = self.data.len();
        let mut collected = Vec::new();
        let mut curr = (self.head + capacity - 1) % capacity;
        let mut n = 0;

        while n < self.len && n < 30 {
            let c = self.data[curr];

            if c.is_whitespace() {
                break;
            }

            collected.push(c);

            let prev_idx = (curr + capacity - 1) % capacity;
            let prev_char = if n + 1 < self.len {
                Some(self.data[prev_idx])
            } else {
                None
            };

            let mut word = collected.clone();
            word.reverse();
            candidates.push((word.into_iter().collect(), prev_char));

            curr = (curr + capacity - 1) % capacity;
            n += 1;
        }

        candidates
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
        assert_eq!(b.extract_trigger_word('>', false), Some("gm".to_string()));
    }

    #[test]
    fn extract_trigger_word_two_triggers_returns_none_prevents_partial_delete() {
        let mut b = FastBuffer::new();
        type_str(&mut b, ">brb>gm");
        assert_eq!(
            b.extract_trigger_word('>', false),
            None,
            "simulates user typing >brb>gm without finishing first expansion — must not match a keyword with wrong delete span"
        );
    }

    #[test]
    fn extract_trigger_word_whitespace_before_trigger_token_aborts() {
        let mut b = FastBuffer::new();
        // Walk backward: 'm', then space — cannot reach `>` without crossing whitespace.
        type_str(&mut b, ">g m");
        assert_eq!(b.extract_trigger_word('>', false), None);
    }

    #[test]
    fn extract_trigger_word_no_trigger_in_suffix() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "plain");
        assert_eq!(b.extract_trigger_word('>', false), None);
    }

    #[test]
    fn two_triggers_in_suffix_still_rejected_after_ring_wrap() {
        let mut b = FastBuffer::new();
        for _ in 0..(FAST_BUFFER_CAPACITY - 2) {
            b.push('a');
        }
        type_str(&mut b, ">x>y");
        assert_eq!(b.extract_trigger_word('>', false), None);
    }

    #[test]
    fn older_trigger_separated_by_whitespace_does_not_block_new_trigger() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "note: x > y and then >gm");
        assert_eq!(b.extract_trigger_word('>', false), Some("gm".to_string()));
    }

    #[test]
    fn extract_trigger_word_quote_aware_whitespace() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">gfb-"my branch""#);
        assert_eq!(
            b.extract_trigger_word('>', false),
            Some(r#"gfb-"my branch""#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_ignores_trigger_chars_inside_quotes() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">echo-">>>""#);
        assert_eq!(
            b.extract_trigger_word('>', false),
            Some(r#"echo-">>>""#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_handles_backslash_escaped_quotes_inside_quotes() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">cmd-"\"echo\"""#);
        assert_eq!(
            b.extract_trigger_word('>', false),
            Some(r#"cmd-"\"echo\"""#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_handles_escaped_backslashes_before_quotes() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">cmd-"ab\\""#);
        assert_eq!(
            b.extract_trigger_word('>', false),
            Some(r#"cmd-"ab\\""#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_single_quote_aware_whitespace() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">search-'Neil Armstrong'"#);
        assert_eq!(
            b.extract_trigger_word('>', false),
            Some(r#"search-'Neil Armstrong'"#.to_string())
        );
    }

    #[test]
    fn extract_trigger_word_abort_on_unopened_quote_state() {
        let mut b = FastBuffer::new();
        type_str(&mut b, r#">foo-"bar"#);
        assert_eq!(b.extract_trigger_word('>', false), None);
    }

    #[test]
    fn extract_trigger_word_allow_spaces() {
        let mut b = FastBuffer::new();
        type_str(&mut b, ">hi:erein aimer: how was your day");
        assert_eq!(
            b.extract_trigger_word('>', true),
            Some("hi:erein aimer: how was your day".to_string())
        );

        let mut b2 = FastBuffer::new();
        type_str(&mut b2, "hello >world >hi:erein aimer");
        assert_eq!(
            b2.extract_trigger_word('>', true),
            Some("hi:erein aimer".to_string())
        );

        // Should still fail if multiple trigger characters without space
        let mut b3 = FastBuffer::new();
        type_str(&mut b3, ">brb>gm");
        assert_eq!(b3.extract_trigger_word('>', true), None);
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

    #[test]
    fn test_buffer_grows_automatically() {
        let mut b = FastBuffer::new();
        // Initial capacity is 512. Push 513 characters.
        for i in 0..513 {
            b.push(char::from_digit((i % 10) as u32, 10).unwrap());
        }
        assert_eq!(b.data.len(), 1024);
        assert_eq!(b.len, 513);

        // Check that all characters are preserved in correct order
        let s = b.buffer_string();
        assert_eq!(s.len(), 513);
        let mut expected = String::new();
        for i in 0..513 {
            expected.push(char::from_digit((i % 10) as u32, 10).unwrap());
        }
        assert_eq!(s, expected);
    }

    #[test]
    fn test_buffer_grow_realigns_correctly_when_wrapped() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "hello");
        b.pop_n(5); // Now head is 5, len is 0.

        // Now fill up to capacity (512)
        for _ in 0..512 {
            b.push('x');
        }
        // At this point, the buffer is full (len = 512, capacity = 512).
        // Pushing one more triggers grow() which must unroll and realign from the offset head.
        b.push('y');

        assert_eq!(b.data.len(), 1024);
        assert_eq!(b.len, 513);

        let s = b.buffer_string();
        assert_eq!(s.len(), 513);
        assert!(s.starts_with("xxxxxxxxxx"));
        assert!(s.ends_with("xxxxxxxxy"));
    }

    #[test]
    fn test_extract_suffix_candidates() {
        let mut b = FastBuffer::new();
        for c in "hello,btw".chars() {
            b.push(c);
        }
        let candidates = b.extract_suffix_candidates();
        assert!(
            candidates
                .iter()
                .any(|(s, prev)| s == "btw" && *prev == Some(','))
        );
    }
}
