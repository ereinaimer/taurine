use std::borrow::Cow;

pub const FAST_BUFFER_CAPACITY: usize = 1024 * 1024; // 1 MiB hard capacity cap (1,048,576 chars)
const INITIAL_BUFFER_CAPACITY: usize = 512;

use smallvec::SmallVec;

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
            data: vec!['\0'; INITIAL_BUFFER_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    fn grow(&mut self) {
        let old_capacity = self.data.len();
        let new_capacity = (old_capacity * 2).min(FAST_BUFFER_CAPACITY);
        if new_capacity <= old_capacity {
            return;
        }
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
            if self.data.len() >= FAST_BUFFER_CAPACITY {
                let capacity = self.data.len();
                self.data[self.head] = c;
                self.head = (self.head + 1) % capacity;
                return;
            }
            self.grow();
        }

        let capacity = self.data.len();
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
            self.data[self.head] = '\0';
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
        self.data.fill('\0');
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

    pub fn as_str(&self) -> Cow<'_, str> {
        if self.len == 0 {
            return Cow::Borrowed("");
        }
        let capacity = self.data.len();
        let start = (self.head + capacity - self.len) % capacity;
        let end = start + self.len;

        if end <= capacity {
            // Contiguous in underlying vec - iterate slice directly
            let slice = &self.data[start..end];
            let mut s = String::with_capacity(self.len);
            for &ch in slice {
                s.push(ch);
            }
            Cow::Owned(s)
        } else {
            // Wrapped - must allocate
            let mut s = String::with_capacity(self.len);
            let first_part = capacity - start;
            for &ch in &self.data[start..] {
                s.push(ch);
            }
            for &ch in &self.data[..self.len - first_part] {
                s.push(ch);
            }
            Cow::Owned(s)
        }
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
    fn trigger_char_count_in_nonwhitespace_suffix(&self, trigger_char: char) -> usize {
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
                if c == ' ' && count == 0 {
                    // Allow leading spaces
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

    pub fn extract_trigger_word(&self, trigger_char: char) -> Option<String> {
        if self.len == 0 {
            return None;
        }

        if self.trigger_char_count_in_nonwhitespace_suffix(trigger_char) > 1 {
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
                if c == ' ' {
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

    /// Returns true if the buffer tail contains any dictionary trigger keywords
    /// ("mean", "defin", "synonym", "antonym", "opposit").
    pub fn has_dictionary_intent(&self) -> bool {
        if self.len < 4 {
            return false;
        }

        let check_len = self.len.min(80);
        let capacity = self.data.len();
        let mut tail = [0u8; 80];

        for (i, byte) in tail[..check_len].iter_mut().enumerate() {
            let idx = (self.head + capacity - check_len + i) % capacity;
            let c = self.data[idx];
            *byte = if c.is_ascii() {
                (c as u8).to_ascii_lowercase()
            } else {
                b' '
            };
        }

        let tail_slice = &tail[..check_len];
        tail_slice.windows(4).any(|w| w == b"mean")
            || tail_slice.windows(5).any(|w| w == b"defin")
            || tail_slice.windows(7).any(|w| w == b"synonym")
            || tail_slice.windows(7).any(|w| w == b"antonym")
            || tail_slice.windows(7).any(|w| w == b"opposit")
    }

    pub fn extract_suffix_candidates(&self) -> SmallVec<[(String, Option<char>); 4]> {
        let mut candidates = SmallVec::new();
        if self.len == 0 {
            return candidates;
        }

        let capacity = self.data.len();
        let mut collected: Vec<char> = Vec::with_capacity(30.min(self.len));
        let mut curr = (self.head + capacity - 1) % capacity;
        let mut n = 0;

        while n < self.len && n < 30 {
            let c = self.data[curr];
            if c.is_whitespace() {
                let mut space_collected = false;
                if c == ' ' && n > 0 {
                    let mut check_curr = (curr + capacity - 1) % capacity;
                    let mut check_n = n + 1;
                    let mut ok = true;
                    let mut chars_to_collect = Vec::new();

                    for _ in 0..3 {
                        if check_n < self.len
                            && check_n < 30
                            && self.data[check_curr].is_ascii_uppercase()
                        {
                            chars_to_collect.push(self.data[check_curr]);
                            check_curr = (check_curr + capacity - 1) % capacity;
                            check_n += 1;
                        } else {
                            ok = false;
                            break;
                        }
                    }

                    if ok {
                        let mut has_minus = false;
                        if check_n < self.len && check_n < 30 && self.data[check_curr] == '-' {
                            has_minus = true;
                            check_curr = (check_curr + capacity - 1) % capacity;
                            check_n += 1;
                        }

                        collected.push(' ');
                        for uc in chars_to_collect {
                            collected.push(uc);
                        }
                        if has_minus {
                            collected.push('-');
                        }

                        curr = check_curr;
                        n = check_n;
                        space_collected = true;
                    }
                }
                if !space_collected {
                    // Include space in collected and continue to build multi-word candidates
                    collected.push(' ');
                    curr = (curr + capacity - 1) % capacity;
                    n += 1;
                    continue;
                }
                continue;
            }
            collected.push(c);
            curr = (curr + capacity - 1) % capacity;
            n += 1;
        }

        // Build candidates in one pass (no O(n²) cloning)
        for len in 1..=collected.len() {
            let word: String = collected[..len].iter().rev().collect();
            let prev_char = collected.get(len).copied();
            candidates.push((word, prev_char));
        }

        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::{FAST_BUFFER_CAPACITY, FastBuffer};
    use std::borrow::Cow;

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
    fn extract_trigger_word_allows_spaces_in_suffix() {
        let mut b = FastBuffer::new();
        // Walk backward: 'm', then space — spaces are always allowed now,
        // so the trigger word spans the space.
        type_str(&mut b, ">g m");
        assert_eq!(b.extract_trigger_word('>'), Some("g m".to_string()));
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
    fn extract_trigger_word_allow_spaces() {
        let mut b = FastBuffer::new();
        type_str(&mut b, ">hi:erein aimer: how was your day");
        assert_eq!(
            b.extract_trigger_word('>'),
            Some("hi:erein aimer: how was your day".to_string())
        );

        let mut b2 = FastBuffer::new();
        type_str(&mut b2, "hello >world >hi:erein aimer");
        assert_eq!(
            b2.extract_trigger_word('>'),
            Some("hi:erein aimer".to_string())
        );

        // Should still fail if multiple trigger characters without space
        let mut b3 = FastBuffer::new();
        type_str(&mut b3, ">brb>gm");
        assert_eq!(b3.extract_trigger_word('>'), None);
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
        // Full list: b,tw,btw each with their preceding char
        assert!(candidates.iter().any(|(s, _)| s == "w"));
        assert!(candidates.iter().any(|(s, _)| s == "tw"));
        assert!(candidates.iter().any(|(s, _)| s == "btw"));
        // Longest candidate (scan continues past non-whitespace separators)
        assert_eq!(
            candidates.last().map(|(s, _)| s.as_str()),
            Some("hello,btw")
        );
    }

    #[test]
    fn test_extract_suffix_candidates_empty_buffer() {
        let b = FastBuffer::new();
        let candidates = b.extract_suffix_candidates();
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_extract_suffix_candidates_whitespace_boundary() {
        let mut b = FastBuffer::new();
        for c in "hello world".chars() {
            b.push(c);
        }
        let candidates = b.extract_suffix_candidates();
        // Both "world" and "hello world" should be extracted (spaces included for multi-word candidates)
        assert!(candidates.iter().any(|(s, _)| s == "world"));
        assert!(candidates.iter().any(|(s, _)| s == "hello world"));
    }

    #[test]
    fn test_extract_suffix_candidates_multi_word() {
        let mut b = FastBuffer::new();
        for c in "my email".chars() {
            b.push(c);
        }
        let candidates = b.extract_suffix_candidates();
        assert!(candidates.iter().any(|(s, _)| s == "my email"));
        assert!(candidates.iter().any(|(s, _)| s == "email"));
    }

    #[test]
    fn test_extract_suffix_candidates_triple_word() {
        let mut b = FastBuffer::new();
        for c in "a b c".chars() {
            b.push(c);
        }
        let candidates = b.extract_suffix_candidates();
        assert!(candidates.iter().any(|(s, _)| s == "a b c"));
        assert!(candidates.iter().any(|(s, _)| s == "b c"));
        assert!(candidates.iter().any(|(s, _)| s == "c"));
    }

    #[test]
    fn test_extract_suffix_candidates_does_not_cross_newline() {
        let mut b = FastBuffer::new();
        for c in "hello\nworld".chars() {
            b.push(c);
        }
        let candidates = b.extract_suffix_candidates();
        // Only "world" — newline is not a space, so scanning stops
        assert!(candidates.iter().any(|(s, _)| s == "world"));
        assert!(!candidates.iter().any(|(s, _)| s == "hello\nworld"));
    }

    #[test]
    fn test_extract_suffix_candidates_does_not_cross_tab() {
        let mut b = FastBuffer::new();
        for c in "hello\tworld".chars() {
            b.push(c);
        }
        let candidates = b.extract_suffix_candidates();
        assert!(candidates.iter().any(|(s, _)| s == "world"));
        assert!(!candidates.iter().any(|(s, _)| s == "hello\tworld"));
    }

    #[test]
    fn test_extract_suffix_candidates_caps_at_30() {
        let mut b = FastBuffer::new();
        for c in "abcdefghijklmnopqrstuvwxyz0123456789".chars() {
            b.push(c);
        }
        let candidates = b.extract_suffix_candidates();
        // Max 30 candidates
        assert!(candidates.len() <= 30);
        // The longest should be 30 chars
        assert_eq!(candidates.last().map(|(s, _)| s.len()), Some(30));
    }

    #[test]
    fn test_as_str_contiguous() {
        let mut b = FastBuffer::new();
        for c in "hello".chars() {
            b.push(c);
        }
        let cow = b.as_str();
        assert!(matches!(cow, Cow::Owned(_)));
        assert_eq!(cow.as_ref(), "hello");
    }

    #[test]
    fn test_as_str_wrapped() {
        let mut b = FastBuffer::new();
        // Fill to capacity then wrap
        for _ in 0..512 {
            b.push('x');
        }
        b.push('y');
        b.push('z');
        let cow = b.as_str();
        assert!(matches!(cow, Cow::Owned(_)));
        assert_eq!(cow.as_ref().len(), 514);
        assert!(cow.as_ref().ends_with("yz"));
    }

    #[test]
    fn test_as_str_empty() {
        let b = FastBuffer::new();
        let cow = b.as_str();
        assert!(matches!(cow, Cow::Borrowed(_)));
        assert_eq!(cow.as_ref(), "");
    }

    #[test]
    fn test_extract_trigger_word_allows_spaces() {
        let mut b = FastBuffer::new();
        type_str(&mut b, ">my email address");
        assert_eq!(
            b.extract_trigger_word('>'),
            Some("my email address".to_string())
        );
    }

    #[test]
    fn test_extract_trigger_word_allows_spaces_only_after_trigger_char() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "prefix text >my email address");
        assert_eq!(
            b.extract_trigger_word('>'),
            Some("my email address".to_string())
        );
    }

    #[test]
    fn test_extract_trigger_word_allow_spaces_still_rejects_newlines() {
        let mut b = FastBuffer::new();
        type_str(&mut b, ">my\nemail");
        assert_eq!(b.extract_trigger_word('>'), None);
    }

    #[test]
    fn test_extract_suffix_candidates_with_iso_code() {
        let mut b = FastBuffer::new();
        for c in "INR 14,500".chars() {
            b.push(c);
        }
        let candidates = b.extract_suffix_candidates();
        assert!(candidates.iter().any(|(s, _)| s == "INR 14,500"));

        let mut b2 = FastBuffer::new();
        for c in "-USD 3,200".chars() {
            b2.push(c);
        }
        let candidates2 = b2.extract_suffix_candidates();
        assert!(candidates2.iter().any(|(s, _)| s == "-USD 3,200"));
    }

    #[test]
    fn test_buffer_clear_zeroes_memory() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "secret_password_123");
        assert!(b.data.contains(&'s'));
        b.clear();
        assert_eq!(b.len, 0);
        assert_eq!(b.head, 0);
        assert!(b.data.iter().all(|&c| c == '\0'));
    }

    #[test]
    fn test_buffer_hard_capacity_cap() {
        let mut b = FastBuffer::new();
        b.data = vec!['\0'; FAST_BUFFER_CAPACITY];
        b.len = FAST_BUFFER_CAPACITY;
        b.head = 0;

        b.push('Z');
        assert_eq!(b.len, FAST_BUFFER_CAPACITY);
        assert_eq!(b.data.len(), FAST_BUFFER_CAPACITY);
        assert_eq!(b.data[0], 'Z');
    }

    #[test]
    fn test_has_dictionary_intent() {
        let mut b = FastBuffer::new();
        type_str(&mut b, "hello world this is normal text");
        assert!(!b.has_dictionary_intent());

        let mut b2 = FastBuffer::new();
        type_str(&mut b2, "what does serendipity mean");
        assert!(b2.has_dictionary_intent());

        let mut b3 = FastBuffer::new();
        type_str(&mut b3, "define ephemeral");
        assert!(b3.has_dictionary_intent());

        let mut b4 = FastBuffer::new();
        type_str(&mut b4, "synonyms for fast");
        assert!(b4.has_dictionary_intent());

        let mut b5 = FastBuffer::new();
        type_str(&mut b5, "antonym of cold");
        assert!(b5.has_dictionary_intent());

        let mut b6 = FastBuffer::new();
        type_str(&mut b6, "opposite of hot");
        assert!(b6.has_dictionary_intent());
    }
}
