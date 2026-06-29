use super::strip_argument_quotes;
use crate::engine::math;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if transformer != "calc" {
        return None;
    }

    if args.len() > 1 {
        return None;
    }

    let content_trimmed = content.trim();

    if args.is_empty() {
        return math::evaluate(content_trimmed);
    }

    let arg = strip_argument_quotes(args[0]).trim();
    if arg.is_empty() {
        return math::evaluate(content_trimmed);
    }

    let expr = if arg.starts_with(['+', '-', '*', '/', '%', '^']) {
        format!("{content_trimmed} {arg}")
    } else if has_variable_x(arg) {
        replace_variable_x(arg, content_trimmed)
    } else {
        arg.to_string()
    };

    math::evaluate(&expr)
}

fn has_variable_x(arg: &str) -> bool {
    let chars: Vec<char> = arg.chars().collect();
    let len = chars.len();
    for idx in 0..len {
        let ch = chars[idx];
        let prev_is_ident = idx > 0 && (chars[idx - 1].is_alphanumeric() || chars[idx - 1] == '_');
        let next_is_ident =
            idx + 1 < len && (chars[idx + 1].is_alphanumeric() || chars[idx + 1] == '_');
        if (ch == 'x' || ch == 'X') && !prev_is_ident && !next_is_ident {
            return true;
        }
    }
    false
}

fn replace_variable_x(arg: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(arg.len() + replacement.len());
    let chars: Vec<char> = arg.chars().collect();
    let len = chars.len();
    let mut idx = 0;

    while idx < len {
        let ch = chars[idx];
        let prev_is_ident = idx > 0 && (chars[idx - 1].is_alphanumeric() || chars[idx - 1] == '_');
        let next_is_ident =
            idx + 1 < len && (chars[idx + 1].is_alphanumeric() || chars[idx + 1] == '_');
        if (ch == 'x' || ch == 'X') && !prev_is_ident && !next_is_ident {
            result.push_str(replacement);
            idx += 1;
            continue;
        }
        result.push(ch);
        idx += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calc_operator_prefix() {
        assert_eq!(
            apply("calc", &["\"* 1.15\""], "100"),
            Some("115".to_string())
        );
        assert_eq!(apply("calc", &["\"+ 1\""], "5"), Some("6".to_string()));
        assert_eq!(apply("calc", &["\"- 10\""], "50"), Some("40".to_string()));
        assert_eq!(apply("calc", &["\"/ 2\""], "10"), Some("5".to_string()));
        assert_eq!(apply("calc", &["\"% 4\""], "10"), Some("2".to_string()));
        assert_eq!(apply("calc", &["\"^ 3\""], "2"), Some("8".to_string()));
        assert_eq!(
            apply("calc", &["\"- 10.5\""], "20.5"),
            Some("10".to_string())
        );
    }

    #[test]
    fn test_calc_parameterless() {
        assert_eq!(apply("calc", &[], "10 + 20"), Some("30".to_string()));
        assert_eq!(apply("calc", &[], "100 / 4"), Some("25".to_string()));
        assert_eq!(apply("calc", &[], "2 ^ 3"), Some("8".to_string()));
        assert_eq!(apply("calc", &[], "  5 * 5  "), Some("25".to_string()));
    }

    #[test]
    fn test_calc_variable_x() {
        assert_eq!(
            apply("calc", &["\"x * 2 + 5\""], "10"),
            Some("25".to_string())
        );
        assert_eq!(
            apply("calc", &["\"X + 100\""], "50"),
            Some("150".to_string())
        );
        assert_eq!(apply("calc", &["\"x + x\""], "12"), Some("24".to_string()));
        assert_eq!(apply("calc", &["\"sqrt(x)\""], "16"), Some("4".to_string()));
        assert_eq!(
            apply("calc", &["\"abs(x)\""], "-42"),
            Some("42".to_string())
        );
        assert_eq!(
            apply("calc", &["\"floor(x)\""], "3.8"),
            Some("3".to_string())
        );
        assert_eq!(
            apply("calc", &["\"ceil(x)\""], "3.1"),
            Some("4".to_string())
        );
        assert_eq!(
            apply("calc", &["\"round(x)\""], "3.5"),
            Some("4".to_string())
        );
    }

    #[test]
    fn test_calc_quoted_and_trimmed_arguments() {
        assert_eq!(apply("calc", &["'* 2'"], "15"), Some("30".to_string()));
        assert_eq!(apply("calc", &["   * 3   "], "4"), Some("12".to_string()));
    }

    #[test]
    fn test_calc_invalid_and_edge_cases() {
        assert_eq!(apply("calc", &["\"* 1.15\""], "invalid_number"), None);
        assert_eq!(apply("calc", &["\"invalid_expr\""], "10"), None);
        assert_eq!(apply("calc", &["\"+ 1\"", "\"- 2\""], "10"), None);
        assert_eq!(apply("math", &["\"+ 1\""], "5"), None);
    }
}
