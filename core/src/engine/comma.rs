use regex::Regex;
use std::sync::OnceLock;

pub fn preprocess(input: &str) -> (String, Option<Vec<usize>>) {
    static COMMA_NUM_RE: OnceLock<Regex> = OnceLock::new();
    let re = COMMA_NUM_RE.get_or_init(|| Regex::new(r"[0-9]+(?:,[0-9]+)+").unwrap());

    let intervals = if let Some(m) = re.find(input) {
        let matched_str = m.as_str();
        let integer_part = matched_str.split('.').next().unwrap_or(matched_str);
        let mut ivs = Vec::new();
        let mut current_len = 0;
        for c in integer_part.chars().rev() {
            if c == ',' {
                if current_len > 0 {
                    ivs.push(current_len);
                    current_len = 0;
                }
            } else if c.is_ascii_digit() {
                current_len += 1;
            }
        }
        if ivs.is_empty() { None } else { Some(ivs) }
    } else {
        None
    };

    let cleaned = input.replace(',', "");
    (cleaned, intervals)
}

pub fn format_result(result: &str, intervals: &[usize]) -> String {
    if intervals.is_empty() {
        return result.to_string();
    }

    static SPLIT_RE: OnceLock<Regex> = OnceLock::new();
    let split_re =
        SPLIT_RE.get_or_init(|| Regex::new(r"^([+-]?[0-9]+(?:\.[0-9]+)?)(.*)$").unwrap());

    let Some(caps) = split_re.captures(result) else {
        return result.to_string();
    };

    let num_str = caps.get(1).map(|m| m.as_str()).unwrap_or("");
    let suffix = caps.get(2).map(|m| m.as_str()).unwrap_or("");

    if num_str.is_empty() {
        return result.to_string();
    }

    let mut chars = num_str.chars().peekable();
    let mut sign = String::new();
    if let Some(&c) = chars.peek()
        && (c == '+' || c == '-')
    {
        sign.push(c);
        chars.next();
    }

    let remaining: String = chars.collect();
    let parts: Vec<&str> = remaining.split('.').collect();
    let integer_part = parts[0];
    let decimal_part = if parts.len() > 1 {
        format!(".{}", parts[1])
    } else {
        String::new()
    };

    let mut formatted_integer = String::new();
    let mut digits = integer_part.chars().rev().peekable();
    let mut interval_idx = 0;

    while digits.peek().is_some() {
        let interval_size = if interval_idx < intervals.len() {
            intervals[interval_idx]
        } else {
            *intervals.last().unwrap_or(&3)
        };

        let mut block = String::new();
        for _ in 0..interval_size {
            if let Some(digit) = digits.next() {
                block.push(digit);
            } else {
                break;
            }
        }

        if !formatted_integer.is_empty() && !block.is_empty() {
            formatted_integer.push(',');
        }
        formatted_integer.push_str(&block);
        interval_idx += 1;
    }

    let formatted_integer_reversed: String = formatted_integer.chars().rev().collect();
    format!(
        "{}{}{}{}",
        sign, formatted_integer_reversed, decimal_part, suffix
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preprocess_western() {
        let (cleaned, intervals) = preprocess("100,000*2");
        assert_eq!(cleaned, "100000*2");
        assert_eq!(intervals, Some(vec![3]));
    }

    #[test]
    fn test_preprocess_indian() {
        let (cleaned, intervals) = preprocess("2,00,000/2");
        assert_eq!(cleaned, "200000/2");
        assert_eq!(intervals, Some(vec![3, 2]));
    }

    #[test]
    fn test_preprocess_no_commas() {
        let (cleaned, intervals) = preprocess("1000/2");
        assert_eq!(cleaned, "1000/2");
        assert_eq!(intervals, None);
    }

    #[test]
    fn test_format_western() {
        let formatted = format_result("200000", &[3]);
        assert_eq!(formatted, "200,000");
    }

    #[test]
    fn test_format_indian() {
        let formatted = format_result("100000", &[3, 2]);
        assert_eq!(formatted, "1,00,000");
    }

    #[test]
    fn test_format_with_suffix() {
        let formatted = format_result("180032f", &[3]);
        assert_eq!(formatted, "180,032f");
    }

    #[test]
    fn test_format_float_and_suffix() {
        let formatted = format_result("9125.68inr", &[3, 2]);
        assert_eq!(formatted, "9,125.68inr");
    }
}
