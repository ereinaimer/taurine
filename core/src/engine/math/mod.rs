pub mod parser;

pub fn evaluate(expr: &str) -> Option<String> {
    let (cleaned, intervals) = crate::engine::comma::preprocess(expr);

    // Ensure the expression contains at least one digit or valid mathematical feature.
    if !cleaned
        .chars()
        .any(|c| c.is_ascii_digit() || "piPIeE".contains(c))
    {
        return None;
    }

    let tokens = parser::tokenize(&cleaned)?;
    if parser::is_single_operand(&tokens) {
        return None;
    }
    let result = parser::parse_expression(&tokens)?;

    // Format to at most 4 decimal places, trimming trailing zeros and the decimal point if unnecessary.
    let formatted = format!("{:.4}", result);
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');

    let final_res = if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    };

    if let Some(ref ivs) = intervals {
        Some(crate::engine::comma::format_result(&final_res, ivs))
    } else {
        Some(final_res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_with_commas() {
        assert_eq!(evaluate("100,000*2"), Some("200,000".to_string()));
        assert_eq!(evaluate("2,00,000/2"), Some("1,00,000".to_string()));
        assert_eq!(evaluate("1000/2"), Some("500".to_string()));
    }

    #[test]
    fn test_implicit_mult_requires_adjacency() {
        assert_eq!(evaluate("2(2)"), Some("4".to_string()));
        assert_eq!(evaluate("(2)(3)"), Some("6".to_string()));
        assert_eq!(evaluate("2^3(4)"), Some("32".to_string()));
        assert_eq!(evaluate("2pi"), Some("6.2832".to_string()));
    }

    #[test]
    fn test_whitespace_breaks_implicit_mult() {
        assert_eq!(evaluate("2 2"), None);
        assert_eq!(evaluate("4\n2"), None);
        assert_eq!(evaluate("2 2+2"), None);
        assert_eq!(evaluate("2 (2)"), None);
        assert_eq!(evaluate("2 pi"), None);
        assert_eq!(evaluate("(2) (3)"), None);
        assert_eq!(evaluate("2 sqrt(4)"), None);
    }

    #[test]
    fn test_explicit_operators_with_whitespace() {
        assert_eq!(evaluate("2 * 2"), Some("4".to_string()));
        assert_eq!(evaluate("2 + 2"), Some("4".to_string()));
        assert_eq!(evaluate(" 2 * 2 "), Some("4".to_string()));
        assert_eq!(evaluate("1.5e3 + 100"), Some("1600".to_string()));
    }
}
