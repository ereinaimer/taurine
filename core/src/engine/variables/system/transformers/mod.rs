pub mod ai;
mod calc;
mod case;
mod crypto;
mod encoding;
mod formatting;
mod lines;
mod text;

pub const TRANSFORMERS: &[&str] = &[
    "upper",
    "uppercase",
    "lower",
    "lowercase",
    "snake",
    "snakecase",
    "kebab",
    "kebabcase",
    "pascal",
    "pascalcase",
    "camel",
    "camelcase",
    "title",
    "titlecase",
    "sentencecase",
    "shoutysnake",
    "shoutysnakecase",
    "shoutykebab",
    "shoutykebabcase",
    "train",
    "traincase",
    "mockingcase",
    "spongebobcase",
    "leet",
    "leetspeak",
    "reverse",
    "length",
    "trim",
    "truncate",
    "repeat",
    "replace",
    "remove",
    "regexreplace",
    "substring",
    "extracturls",
    "extractemails",
    "onlydigits",
    "onlyalphanumeric",
    "stripall",
    "urlencode",
    "urldecode",
    "base64encode",
    "base64decode",
    "hexencode",
    "hexdecode",
    "md5",
    "sha1",
    "sha256",
    "sha512",
    "crc32",
    "rot13",
    "firstline",
    "lastline",
    "reverselines",
    "prefixlines",
    "suffixlines",
    "joinlines",
    "splitlines",
    "removeemptylines",
    "compactlines",
    "shufflelines",
    "quote",
    "doublequote",
    "singlequote",
    "unquote",
    "calc",
    "ai",
];

#[derive(Debug)]
struct ParsedTransformer<'a> {
    name: &'a str,
    args: Vec<&'a str>,
}

/// Splits an expression on top-level `|` characters (ignoring `|` inside quotes or parentheses).
/// Returns a list of trimmed segments: `[base_expression, transformer1, transformer2, ...]`.
pub fn split_pipeline(input: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0usize;

    for (idx, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                segments.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    segments.push(input[start..].trim());
    segments
}

pub fn is_valid_transformer(input: &str) -> bool {
    parse_transformer(input).is_some()
}

pub use ai::{extract_ai_prompt, is_ai_transformer};

pub fn apply(transformer: &str, content: &str) -> Option<String> {
    let parsed = parse_transformer(transformer)?;

    case::apply(parsed.name, content)
        .or_else(|| text::apply(parsed.name, &parsed.args, content))
        .or_else(|| encoding::apply(parsed.name, &parsed.args, content))
        .or_else(|| crypto::apply(parsed.name, &parsed.args, content))
        .or_else(|| lines::apply(parsed.name, &parsed.args, content))
        .or_else(|| formatting::apply(parsed.name, &parsed.args, content))
        .or_else(|| calc::apply(parsed.name, &parsed.args, content))
        .or_else(|| ai::apply(parsed.name, &parsed.args, content))
}

pub(crate) fn strip_argument_quotes(arg: &str) -> &str {
    let trimmed = arg.trim();
    super::strip_quotes(trimmed).unwrap_or(trimmed)
}

fn parse_transformer(input: &str) -> Option<ParsedTransformer<'_>> {
    let input = input.trim();

    if let Some(open_idx) = find_call_open(input) {
        let name = input[..open_idx].trim();
        let args = input
            .strip_suffix(')')
            .and_then(|prefix| prefix.get(open_idx + 1..))
            .and_then(split_arguments)?;

        TRANSFORMERS
            .contains(&name)
            .then_some(ParsedTransformer { name, args })
    } else {
        TRANSFORMERS.contains(&input).then_some(ParsedTransformer {
            name: input,
            args: Vec::new(),
        })
    }
}

fn find_call_open(input: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                current if current == active_quote => quote = None,
                _ => {}
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => return Some(idx),
            ')' => return None,
            _ => {}
        }
    }

    None
}

fn split_arguments(args: &str) -> Option<Vec<&str>> {
    if args.trim().is_empty() {
        return Some(Vec::new());
    }

    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0usize;

    for (idx, ch) in args.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                current if current == active_quote => quote = None,
                _ => {}
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            ',' if depth == 0 => {
                parts.push(args[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if depth != 0 || quote.is_some() {
        return None;
    }

    parts.push(args[start..].trim());
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_pipeline_supports_parameterized_transformers() {
        assert_eq!(
            split_pipeline("clipboard | truncate(5)"),
            vec!["clipboard", "truncate(5)"]
        );
        assert_eq!(
            split_pipeline("clipboard | replace(\",\", \";\") | upper"),
            vec!["clipboard", "replace(\",\", \";\")", "upper"]
        );
        assert_eq!(
            split_pipeline("'a|b' | replace(\"|\", \"-\")"),
            vec!["'a|b'", "replace(\"|\", \"-\")"]
        );
    }

    #[test]
    fn test_apply_parameterized_transformers() {
        assert_eq!(apply("truncate(3)", "abcdef"), Some("abc".to_string()));
        assert_eq!(
            apply("replace(\",\", \";\")", "a,b,c"),
            Some("a;b;c".to_string())
        );
        assert_eq!(
            apply("regexreplace(\"([a-z]),([A-Z])\", \"$1 $2\")", "a,B"),
            Some("a B".to_string())
        );
        assert_eq!(apply("substring(1, 3)", "aßc"), Some("ßc".to_string()));
        assert_eq!(apply("length", "aßc"), Some("3".to_string()));
    }
}
