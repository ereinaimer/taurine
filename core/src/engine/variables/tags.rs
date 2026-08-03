pub const TAG_OPEN: u8 = b'[';
pub const TAG_CLOSE: u8 = b']';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TagBounds {
    pub start: usize,
    pub end: usize,
}

pub fn is_escaped(bytes: &[u8], idx: usize) -> bool {
    let mut backslashes = 0;
    let mut cursor = idx;

    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        backslashes += 1;
        cursor -= 1;
    }

    backslashes % 2 == 1
}

pub fn trim_slice(s: &str) -> &str {
    let trimmed = s.trim();
    let start = s.len() - s.trim_start().len();
    &s[start..start + trimmed.len()]
}

pub fn scan_tag_bounds(template: &str) -> Vec<TagBounds> {
    let bytes = template.as_bytes();
    let mut stack = Vec::new();
    let mut tags = Vec::new();
    let mut ptr = 0;
    let mut quote = None;

    while ptr < bytes.len() {
        if let Some(active_quote) = quote {
            if bytes[ptr] == active_quote && !is_escaped(bytes, ptr) {
                quote = None;
            }
            ptr += 1;
            continue;
        }

        match bytes[ptr] {
            b'\'' | b'"' if !stack.is_empty() && !is_escaped(bytes, ptr) => {
                quote = Some(bytes[ptr])
            }
            TAG_OPEN if !is_escaped(bytes, ptr) => stack.push(ptr),
            TAG_CLOSE if !is_escaped(bytes, ptr) => {
                if let Some(start) = stack.pop() {
                    tags.push(TagBounds { start, end: ptr });
                }
            }
            _ => {}
        }
        ptr += 1;
    }

    tags
}

pub fn split_key_default(inner: &str) -> (&str, Option<&str>) {
    let inner = trim_slice(inner);
    let bytes = inner.as_bytes();
    let mut depth = 0;
    let mut paren_depth = 0;
    let mut ptr = 0;
    let mut quote = None;
    while ptr < bytes.len() {
        if let Some(active_quote) = quote {
            if bytes[ptr] == active_quote && !is_escaped(bytes, ptr) {
                quote = None;
            }
        } else if (bytes[ptr] == b'\'' || bytes[ptr] == b'"') && !is_escaped(bytes, ptr) {
            quote = Some(bytes[ptr]);
        } else if bytes[ptr] == TAG_OPEN && !is_escaped(bytes, ptr) {
            depth += 1;
        } else if bytes[ptr] == TAG_CLOSE && !is_escaped(bytes, ptr) {
            depth -= 1;
        } else if bytes[ptr] == b'(' && !is_escaped(bytes, ptr) {
            paren_depth += 1;
        } else if bytes[ptr] == b')' && !is_escaped(bytes, ptr) {
            paren_depth -= 1;
        } else if bytes[ptr] == b'=' && depth == 0 && paren_depth == 0 {
            return (
                trim_slice(&inner[..ptr]),
                Some(trim_slice(&inner[ptr + 1..])),
            );
        }
        ptr += 1;
    }
    (inner, None)
}

pub fn find_next_tag(text: &str, from: usize) -> Option<TagBounds> {
    let bytes = text.as_bytes();
    let mut ptr = from;
    let mut start = None;
    let mut depth = 0usize;

    while ptr < bytes.len() {
        match bytes[ptr] {
            TAG_OPEN if !is_escaped(bytes, ptr) => {
                if depth == 0 {
                    start = Some(ptr);
                }
                depth += 1;
            }
            TAG_CLOSE if !is_escaped(bytes, ptr) && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return start.map(|tag_start| TagBounds {
                        start: tag_start,
                        end: ptr,
                    });
                }
            }
            _ => {}
        }
        ptr += 1;
    }

    None
}

pub fn tag_inner(text: &str, tag: TagBounds) -> &str {
    trim_slice(&text[tag.start + 1..tag.end])
}
