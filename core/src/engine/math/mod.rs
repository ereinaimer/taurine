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

    // Format intelligently, Rust's default f64 formatting strips trailing zeros.
    Some(format!("{}", result))
}
