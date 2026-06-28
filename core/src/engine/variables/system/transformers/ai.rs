pub fn is_ai_transformer(transformer: &str) -> bool {
    let t = transformer.trim();
    t == "ai" || t.starts_with("ai(") || t.starts_with("ai (")
}

pub fn extract_ai_prompt(transformer: &str) -> &str {
    let t = transformer.trim();
    if let Some(rest) = t.strip_prefix("ai")
        && let Some(inner) = rest.trim().strip_prefix('(')
        && let Some(inner) = inner.strip_suffix(')')
    {
        let inner = inner.trim();
        return crate::engine::variables::system::strip_quotes(inner).unwrap_or(inner);
    }
    ""
}

/// AI responses sometimes include markdown code fences (e.g., ```rust ... ```).
/// This helper strips the leading and trailing fences if the entire response is enclosed in one.
pub fn strip_markdown_fence(text: &str) -> String {
    let t = text.trim();
    if t.starts_with("```") && t.ends_with("```") && t.len() >= 6 {
        // Find the end of the first line (where the language identifier might be)
        let end_of_first_line = t.find('\n').unwrap_or(3);
        let start_idx = if end_of_first_line < t.len() - 3 {
            end_of_first_line + 1 // skip the newline
        } else {
            3 // just the backticks, no newline (e.g. ```text```)
        };
        // It's possible the closing ``` is on its own line or not, just trim the interior
        return t[start_idx..t.len() - 3].trim().to_string();
    }
    text.to_string()
}

pub fn apply(_name: &str, _args: &[&str], _content: &str) -> Option<String> {
    // ai(...) transformers are handled asynchronously by the daemon, not synchronously here.
    // We return None so if it ever slips through, we don't erroneously transform it.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ai_transformer() {
        assert!(is_ai_transformer("ai"));
        assert!(is_ai_transformer("ai(prompt)"));
        assert!(is_ai_transformer("ai (prompt)"));
        assert!(!is_ai_transformer("upper"));
    }

    #[test]
    fn test_extract_ai_prompt() {
        assert_eq!(extract_ai_prompt("ai(summarize)"), "summarize");
        assert_eq!(extract_ai_prompt("ai('summarize')"), "summarize");
        assert_eq!(extract_ai_prompt("ai(\"summarize\")"), "summarize");
        assert_eq!(extract_ai_prompt("ai"), "");
    }

    #[test]
    fn test_strip_markdown_fence() {
        assert_eq!(strip_markdown_fence("```\nhello\n```"), "hello");
        assert_eq!(strip_markdown_fence("```rust\nhello\n```"), "hello");
        assert_eq!(strip_markdown_fence("```text\nhello\n```"), "hello");
        assert_eq!(strip_markdown_fence("hello"), "hello");
    }
}
