pub mod parser;

pub fn evaluate(expr: &str) -> Option<String> {
    // Ensure the expression contains at least one digit or valid mathematical feature.
    if !expr
        .chars()
        .any(|c| c.is_ascii_digit() || "piPIeE".contains(c))
    {
        return None;
    }

    let tokens = parser::tokenize(expr)?;
    let result = parser::parse_expression(&tokens)?;

    // Format to at most 4 decimal places, trimming trailing zeros and the decimal point if unnecessary.
    let formatted = format!("{:.4}", result);
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');

    if trimmed.is_empty() {
        Some("0".to_string())
    } else {
        Some(trimmed.to_string())
    }
}
