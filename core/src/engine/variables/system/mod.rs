//! System variables module.
//!
//! Centralizes logic for reserved keywords and system-wide markers like `{cursor}`,
//! and future variables like `{time.now}`.

pub mod cursor;

use crate::engine::variables::types::FinalExpansion;

/// Checks if a keyword is reserved by the system.
///
/// Reserved keys are either hardcoded (like `cursor`) or follow a namespace
/// pattern (like `time.*`, `crypto.*`).
pub fn is_reserved(key: &str) -> bool {
    key == "cursor" || key.contains('.')
}

/// Checks if a keyword is a post-processing directive.
///
/// Directives are not replaced during interpolation but are instead handled
/// in the `finalize` phase (e.g., `{cursor}`).
pub fn is_directive(key: &str) -> bool {
    key == "cursor"
}

/// Resolves a content-producing system variable.
///
/// For example, `time.now` would be resolved to the current timestamp.
/// Returns `None` for directives or unknown keys.
pub fn resolve(_key: &str) -> Option<String> {
    // TODO: Implement time.* and other system modules here.
    None
}

/// Performs final post-processing on the interpolated string.
///
/// This includes handling directives like `{cursor}` offset calculation.
pub fn finalize(interpolated: &str, trigger: Option<&str>) -> FinalExpansion {
    let mut text = interpolated.to_string();
    let left_arrow_count = cursor::process(&mut text, trigger);

    FinalExpansion {
        text,
        left_arrow_count,
        is_calculation: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_reserved() {
        assert!(is_reserved("cursor"));
        assert!(is_reserved("time.now"));
        assert!(is_reserved("crypto.price"));
        assert!(!is_reserved("username"));
        assert!(!is_reserved("repo"));
    }

    #[test]
    fn test_is_directive() {
        assert!(is_directive("cursor"));
        assert!(!is_directive("time.now"));
    }

    #[test]
    fn test_finalize_cursor() {
        let res = finalize("hello {cursor} world", None);
        assert_eq!(res.text, "hello  world");
        assert_eq!(res.left_arrow_count, 6);
    }
}
