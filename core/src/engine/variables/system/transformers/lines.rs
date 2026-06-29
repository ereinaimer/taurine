use super::strip_argument_quotes;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    match transformer {
        "firstline" if args.is_empty() => Some(content.lines().next().unwrap_or("").to_string()),
        "lastline" if args.is_empty() => Some(content.lines().last().unwrap_or("").to_string()),
        "prefixlines" if args.len() == 1 => Some(prefix_lines(content, args[0])),
        "suffixlines" if args.len() == 1 => Some(suffix_lines(content, args[0])),
        "joinlines" if args.len() == 1 => Some(join_lines(content, args[0])),
        "splitlines" if args.len() == 1 => Some(split_lines(content, args[0])),
        "removeemptylines" | "compactlines" if args.is_empty() => Some(remove_empty_lines(content)),
        _ => None,
    }
}

fn prefix_lines(content: &str, prefix: &str) -> String {
    let prefix = strip_argument_quotes(prefix);
    content
        .lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn suffix_lines(content: &str, suffix: &str) -> String {
    let suffix = strip_argument_quotes(suffix);
    content
        .lines()
        .map(|line| format!("{line}{suffix}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn join_lines(content: &str, delimiter: &str) -> String {
    content
        .lines()
        .collect::<Vec<_>>()
        .join(strip_argument_quotes(delimiter))
}

fn split_lines(content: &str, delimiter: &str) -> String {
    let delimiter = strip_argument_quotes(delimiter);
    if delimiter.is_empty() {
        return content.to_string();
    }

    content.split(delimiter).collect::<Vec<_>>().join("\n")
}

fn remove_empty_lines(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_transformers() {
        let content = "alpha\r\nbeta\ngamma";
        assert_eq!(apply("firstline", &[], content), Some("alpha".to_string()));
        assert_eq!(apply("lastline", &[], content), Some("gamma".to_string()));
        assert_eq!(
            apply("prefixlines", &["\"> \""], "a\nb"),
            Some("> a\n> b".to_string())
        );
        assert_eq!(
            apply("suffixlines", &["\";\""], "a\nb"),
            Some("a;\nb;".to_string())
        );
        assert_eq!(
            apply("joinlines", &["\", \""], "a\nb\nc"),
            Some("a, b, c".to_string())
        );
        assert_eq!(
            apply("splitlines", &["\", \""], "a, b, c"),
            Some("a\nb\nc".to_string())
        );
        assert_eq!(
            apply("removeemptylines", &[], "a\n\n \n b"),
            Some("a\n b".to_string())
        );
        assert_eq!(
            apply("compactlines", &[], "a\n\nb"),
            Some("a\nb".to_string())
        );
    }
}
