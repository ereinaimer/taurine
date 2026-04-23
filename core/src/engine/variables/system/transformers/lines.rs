pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if !args.is_empty() {
        return None;
    }

    match transformer {
        "firstline" => Some(content.lines().next().unwrap_or("").to_string()),
        "lastline" => Some(content.lines().last().unwrap_or("").to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_transformers() {
        let content = "alpha\r\nbeta\ngamma";
        assert_eq!(apply("firstline", &[], content), Some("alpha".to_string()));
        assert_eq!(apply("lastline", &[], content), Some("gamma".to_string()));
    }
}
