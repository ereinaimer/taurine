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

    /// Walks backwards from the head. Stops and aborts if it hits whitespace.
    /// If it hits `trigger_char`, extracts the sequence between `trigger_char` and the head.
    pub fn extract_trigger_word(&self, trigger_char: char) -> Option<String> {
        if self.len == 0 {
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
