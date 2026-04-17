use taurine_core::engine::variables::{
    ValidationError, split_system_tag, valid_modifier_hint, validate_system_tag,
};

const TAG_OPEN: u8 = b'[';
const TAG_CLOSE: u8 = b']';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TagBounds {
    start: usize,
    end: usize,
}

pub fn audit_payload_tags(payload: &str) -> taurine_core::error::Result<()> {
    let mut ptr = 0;

    while let Some(tag) = find_next_tag(payload, ptr) {
        let inner = trim_slice(&payload[tag.start + 1..tag.end]);
        let (key, default_value) = split_key_default(inner);

        if let Some((root, modifier)) = split_system_tag(key) {
            if default_value.is_some() {
                return Err(taurine_core::error::Error::Config(format!(
                    "Invalid system tag [{}]: system tags cannot use default assignments. {}",
                    inner,
                    valid_modifier_hint(root)
                )));
            }

            if let Err(error) = validate_system_tag(root, modifier) {
                return Err(taurine_core::error::Error::Config(format_validation_error(
                    inner, root, modifier, &error,
                )));
            }
        }

        ptr = tag.end + 1;
    }

    Ok(())
}

fn format_validation_error(
    raw_tag: &str,
    root: &str,
    modifier: Option<&str>,
    error: &ValidationError,
) -> String {
    match error {
        ValidationError::MissingModifier { .. } => format!(
            "Invalid system tag [{}]: `{}` requires a modifier. {}",
            raw_tag,
            root,
            valid_modifier_hint(root)
        ),
        ValidationError::UnexpectedModifier { .. } => format!(
            "Invalid system tag [{}]: `{}` does not accept modifier `{}`. {}",
            raw_tag,
            root,
            modifier.unwrap_or_default(),
            valid_modifier_hint(root)
        ),
        ValidationError::InvalidModifier { modifier, .. } => format!(
            "Invalid system tag [{}]: modifier `{}` is not valid for `{}`. {}",
            raw_tag,
            modifier,
            root,
            valid_modifier_hint(root)
        ),
        ValidationError::UnknownRoot(root) => {
            format!("Invalid system tag [{}]: unknown root `{}`.", raw_tag, root)
        }
    }
}

fn is_escaped(bytes: &[u8], idx: usize) -> bool {
    let mut backslashes = 0;
    let mut cursor = idx;

    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        backslashes += 1;
        cursor -= 1;
    }

    backslashes % 2 == 1
}

fn trim_slice(s: &str) -> &str {
    let trimmed = s.trim();
    let start = s.len() - s.trim_start().len();
    &s[start..start + trimmed.len()]
}

fn find_next_tag(text: &str, from: usize) -> Option<TagBounds> {
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
            TAG_CLOSE if !is_escaped(bytes, ptr) => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        return start.map(|tag_start| TagBounds {
                            start: tag_start,
                            end: ptr,
                        });
                    }
                }
            }
            _ => {}
        }
        ptr += 1;
    }

    None
}

fn split_key_default(inner: &str) -> (&str, Option<&str>) {
    let inner = trim_slice(inner);
    let bytes = inner.as_bytes();
    let mut depth = 0usize;
    let mut ptr = 0;

    while ptr < bytes.len() {
        if bytes[ptr] == TAG_OPEN && !is_escaped(bytes, ptr) {
            depth += 1;
        } else if bytes[ptr] == TAG_CLOSE && !is_escaped(bytes, ptr) {
            depth -= 1;
        } else if bytes[ptr] == b'=' && depth == 0 {
            return (
                trim_slice(&inner[..ptr]),
                Some(trim_slice(&inner[ptr + 1..])),
            );
        }
        ptr += 1;
    }

    (inner, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_system_tags_and_literals() {
        assert!(audit_payload_tags("[time.now.upper] [env.USERPROFILE]").is_ok());
        assert!(audit_payload_tags("json = [1, 2, 3]").is_ok());
        assert!(audit_payload_tags("[name.upper]").is_ok());
    }

    #[test]
    fn rejects_invalid_system_modifier() {
        let error = audit_payload_tags("[time.india]").unwrap_err();
        assert!(error.to_string().contains("time.india"));
        assert!(error.to_string().contains("Valid modifiers"));
    }

    #[test]
    fn rejects_system_default_assignment() {
        let error = audit_payload_tags("[cursor=here]").unwrap_err();
        assert!(error.to_string().contains("cannot use default assignments"));
    }

    #[test]
    fn rejects_missing_env_key() {
        let error = audit_payload_tags("[env]").unwrap_err();
        assert!(error.to_string().contains("requires a modifier"));
    }
}
