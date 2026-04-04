#[derive(Debug, Clone)]
pub struct FastBuffer {
    pub(crate) data: [char; 64],
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
            data: ['\0'; 64],
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, c: char) {
        self.data[self.head] = c;
        self.head = (self.head + 1) % 64;
        if self.len < 64 {
            self.len += 1;
        }
    }

    pub fn pop(&mut self) {
        if self.len > 0 {
            self.head = (self.head + 64 - 1) % 64;
            self.len -= 1;
        }
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
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
        let mut curr = (self.head + 64 - 1) % 64;
        let mut n = 0;
        while n < self.len {
            let c = self.data[curr];
            if c.is_whitespace() {
                break;
            }
            if c == trigger_char {
                count += 1;
            }
            curr = (curr + 64 - 1) % 64;
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
        let mut curr = (self.head + 64 - 1) % 64;
        let mut count = 0;

        while count < self.len {
            let c = self.data[curr];
            if c == trigger_char {
                // We've found the trigger char. The keyword is everything after it.
                collected.reverse();
                return Some(collected.into_iter().collect());
            } else if c.is_whitespace() {
                // Invalid sequence, space found before trigger char
                return None;
            } else {
                collected.push(c);
            }
            curr = (curr + 64 - 1) % 64;
            count += 1;
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::FastBuffer;

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
        for _ in 0..62 {
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
}
