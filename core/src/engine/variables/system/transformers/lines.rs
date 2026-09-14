use super::strip_argument_quotes;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if transformer != "lines" {
        return None;
    }
    let action = strip_argument_quotes(args.first()?.trim()).to_lowercase();
    let rest = &args[1..];
    match action.as_str() {
        "first" if rest.is_empty() => Some(content.lines().next().unwrap_or("").to_string()),
        "last" if rest.is_empty() => Some(content.lines().last().unwrap_or("").to_string()),
        "count" if rest.is_empty() => Some(content.lines().count().to_string()),
        "compact" if rest.is_empty() => Some(remove_empty_lines(content)),
        "unique" if rest.is_empty() => Some(uniq_lines(content)),
        "prefix" if rest.len() == 1 => Some(prefix_lines(content, rest[0])),
        "suffix" if rest.len() == 1 => Some(suffix_lines(content, rest[0])),
        "join" if rest.len() == 1 => Some(join_lines(content, rest[0])),
        "split" if rest.len() == 1 => Some(split_lines(content, rest[0])),
        "sort" => sort_lines(content, rest),
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

fn uniq_lines(content: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    content
        .lines()
        .filter(|line| seen.insert(*line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sort_lines(content: &str, args: &[&str]) -> Option<String> {
    let mut lines: Vec<&str> = content.lines().collect();

    let mut desc = false;
    let mut insensitive = false;
    let mut numeric = false;

    for arg in args {
        let clean = strip_argument_quotes(arg).trim().to_lowercase();
        if clean == "desc" {
            desc = true;
        } else if clean == "insensitive" {
            insensitive = true;
        } else if clean == "numeric" {
            numeric = true;
        } else {
            return None;
        }
    }

    if numeric {
        lines.sort_by(|a, b| {
            let a_val = a.trim().parse::<f64>().unwrap_or(f64::NAN);
            let b_val = b.trim().parse::<f64>().unwrap_or(f64::NAN);
            match (a_val.is_nan(), b_val.is_nan()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => a_val
                    .partial_cmp(&b_val)
                    .unwrap_or(std::cmp::Ordering::Equal),
            }
        });
    } else if insensitive {
        lines.sort_by_key(|a| a.to_lowercase());
    } else {
        lines.sort();
    }

    if desc {
        lines.reverse();
    }

    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_transformers() {
        let content = "alpha\r\nbeta\ngamma";
        assert_eq!(
            apply("lines", &["first"], content),
            Some("alpha".to_string())
        );
        assert_eq!(
            apply("lines", &["last"], content),
            Some("gamma".to_string())
        );
        assert_eq!(
            apply("lines", &["prefix", "\"> \""], "a\nb"),
            Some("> a\n> b".to_string())
        );
        assert_eq!(
            apply("lines", &["suffix", "\";\""], "a\nb"),
            Some("a;\nb;".to_string())
        );
        assert_eq!(
            apply("lines", &["join", "\", \""], "a\nb\nc"),
            Some("a, b, c".to_string())
        );
        assert_eq!(
            apply("lines", &["split", "\", \""], "a, b, c"),
            Some("a\nb\nc".to_string())
        );
        assert_eq!(
            apply("lines", &["compact"], "a\n\n \n b"),
            Some("a\n b".to_string())
        );
        assert_eq!(
            apply("lines", &["compact"], "a\n\nb"),
            Some("a\nb".to_string())
        );
        assert_eq!(apply("lines", &["count"], "a\nb\nc"), Some("3".to_string()));
        assert_eq!(
            apply("lines", &["count"], "a\r\nb\r\nc"),
            Some("3".to_string())
        );
        assert_eq!(apply("lines", &["count"], ""), Some("0".to_string()));
        assert_eq!(
            apply("lines", &["unique"], "b\na\nb\nc\na"),
            Some("b\na\nc".to_string())
        );
        assert_eq!(
            apply("lines", &["unique"], "a\n\nb\n\nc"),
            Some("a\n\nb\nc".to_string())
        );
        assert_eq!(
            apply("lines", &["sort"], "b\nc\na"),
            Some("a\nb\nc".to_string())
        );
        assert_eq!(
            apply("lines", &["sort", "\"desc\""], "b\nc\na"),
            Some("c\nb\na".to_string())
        );
        assert_eq!(
            apply("lines", &["sort", "\"insensitive\""], "B\nc\na"),
            Some("a\nB\nc".to_string())
        );
        assert_eq!(
            apply("lines", &["sort", "\"numeric\""], "10\n2\n1.5"),
            Some("1.5\n2\n10".to_string())
        );
        // Rejections: legacy names, missing action, bad arity, unknown flags.
        assert_eq!(apply("firstline", &[], content), None);
        assert_eq!(apply("lines", &[], content), None);
        assert_eq!(apply("lines", &["bogus"], content), None);
        assert_eq!(apply("lines", &["first", "x"], content), None);
        assert_eq!(apply("lines", &["prefix"], content), None);
        assert_eq!(apply("lines", &["sort", "\"reverse\""], content), None);
    }
}
