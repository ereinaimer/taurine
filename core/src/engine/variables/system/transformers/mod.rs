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
];

#[derive(Debug)]
struct ParsedTransformer<'a> {
    name: &'a str,
    args: Vec<&'a str>,
}

pub fn split_suffix(key: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut last_dot = None;

    for (idx, ch) in key.char_indices() {
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
            '.' if depth == 0 => last_dot = Some(idx),
            _ => {}
        }
    }

    if depth != 0 || quote.is_some() {
        return None;
    }

    let dot_idx = last_dot?;
    let sub = &key[..dot_idx];
    let suffix = &key[dot_idx + 1..];

    (!sub.is_empty() && parse_transformer(suffix).is_some()).then_some((sub, suffix))
}

pub fn resolve<F>(key: &str, mut resolve_sub: F) -> Option<String>
where
    F: FnMut(&str) -> Option<String>,
{
    let (sub, suffix) = split_suffix(key)?;
    let content = resolve_sub(sub).or_else(|| super::strip_quotes(sub).map(str::to_string))?;
    apply(suffix, &content)
}

pub fn apply(transformer: &str, content: &str) -> Option<String> {
    let parsed = parse_transformer(transformer)?;

    case::apply(parsed.name, content)
        .or_else(|| text::apply(parsed.name, &parsed.args, content))
        .or_else(|| encoding::apply(parsed.name, &parsed.args, content))
        .or_else(|| crypto::apply(parsed.name, &parsed.args, content))
        .or_else(|| lines::apply(parsed.name, &parsed.args, content))
        .or_else(|| formatting::apply(parsed.name, &parsed.args, content))
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
    fn test_split_suffix_supports_parameterized_transformers() {
        assert_eq!(
            split_suffix("clipboard.truncate(5)"),
            Some(("clipboard", "truncate(5)"))
        );
        assert_eq!(
            split_suffix("clipboard.replace(\",\", \";\").upper"),
            Some(("clipboard.replace(\",\", \";\")", "upper"))
        );
        assert_eq!(
            split_suffix("'a.b'.replace(\".\", \"-\")"),
            Some(("'a.b'", "replace(\".\", \"-\")"))
        );
        assert_eq!(
            split_suffix("clipboard.regexreplace(\"([a-z]),([A-Z])\", \"$1 $2\").upper"),
            Some((
                "clipboard.regexreplace(\"([a-z]),([A-Z])\", \"$1 $2\")",
                "upper"
            ))
        );
        assert_eq!(split_suffix("clipboard.unknown(5)"), None);
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
