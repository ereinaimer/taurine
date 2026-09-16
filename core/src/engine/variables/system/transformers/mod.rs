pub mod ai;
mod calc;
mod case;
pub mod color;
mod crypto;
mod encoding;
mod extractors;
mod formatting;
mod lines;
mod text;

pub const TRANSFORMERS: &[&str] = &[
    "case", "lines", "count", "truncate", "repeat", "replace", "slice", "filter", "strip",
    "encode", "decode", "clean", "hash", "extract", "wrap", "unwrap", "color", "json", "html",
    "xml", "toml", "yaml", "regex", "calc", "ai",
];

/// Accepted argument counts per transformer as (min, max), mirroring each
/// family's dispatch guards. New families must add their row here or
/// save-time validation fails closed.
pub fn transformer_arity(name: &str) -> Option<(usize, usize)> {
    let arity = match name {
        "case" | "count" | "truncate" | "repeat" | "filter" | "strip" | "encode" | "decode"
        | "clean" | "hash" | "wrap" | "unwrap" | "color" | "json" | "html" | "xml" | "toml"
        | "yaml" | "ai" => (1, 1),
        "slice" => (2, 2),
        "replace" => (2, 3),
        "regex" | "extract" => (1, 2),
        "lines" => (1, usize::MAX),
        "calc" => (0, 1),
        _ => return None,
    };
    Some(arity)
}

/// Splits a pipeline segment into transformer name + argument count.
/// Returns None when the segment is not a well-formed known transformer call.
pub fn transformer_call_parts(segment: &str) -> Option<(&str, usize)> {
    let parsed = parse_transformer(segment.trim())?;
    Some((parsed.name, parsed.args.len()))
}

const EXTRACT_TARGETS: &[&str] = &[
    "url",
    "email",
    "phone",
    "mention",
    "hashtag",
    "ip",
    "mac",
    "path",
    "filename",
    "directory",
    "jwt",
    "semver",
    "mdcode",
    "mdtable",
    "mdlist",
];

/// Checks a transformer's argument *values* against its dispatch guards.
/// Returns None when the call is fine, or an expected-form example when it
/// would fail at runtime. Arity itself is `transformer_arity`'s job; `ai`
/// prompts belong to the prompt rule; content-dependent shapes (json paths,
/// toml/yaml paths, calc expressions) always pass here.
///
/// A known name with unparseable arguments (unbalanced quotes or parens)
/// also reports: every other check treats an unparseable segment as "skip",
/// so this is the only place that catches it.
pub fn transformer_value_error(segment: &str) -> Option<String> {
    let Some(parsed) = parse_transformer(segment.trim()) else {
        return transformer_name_only(segment).map(canonical_example);
    };
    // Total on missing args (the arity check runs first in the audit loop,
    // so a missing arg surfaces as an arity error, never here).
    let arg = |i: usize| {
        parsed
            .args
            .get(i)
            .map(|a| strip_argument_quotes(a))
            .unwrap_or("")
    };
    match parsed.name {
        "case" => match arg(0).to_ascii_lowercase().as_str() {
            "upper" | "lower" | "snake" | "kebab" | "pascal" | "camel" | "title" | "sentence"
            | "slug" => None,
            _ => Some("case(upper)".to_string()),
        },
        "count" => match arg(0) {
            "chars" | "words" => None,
            _ => Some("count(words)".to_string()),
        },
        "truncate" | "repeat" => match arg(0).parse::<usize>() {
            Ok(_) => None,
            _ => Some(format!("{}(4)", parsed.name)),
        },
        "slice" => match (arg(0).parse::<usize>(), arg(1).parse::<usize>()) {
            (Ok(_), Ok(_)) => None,
            _ => Some("slice(1, 3)".to_string()),
        },
        "replace" if parsed.args.len() == 3 && arg(0) != "regex" => {
            Some("replace(regex, \"a+\", \"b\")".to_string())
        }
        "filter" => match arg(0) {
            "digits" | "alphanumeric" => None,
            _ => Some("filter(digits)".to_string()),
        },
        "strip" => match arg(0) {
            "whitespace" | "emoji" => None,
            _ => Some("strip(whitespace)".to_string()),
        },
        "encode" | "decode" => match arg(0) {
            "url" | "base64" => None,
            _ => Some(format!("{}(url)", parsed.name)),
        },
        "clean" => match arg(0) {
            "url" => None,
            _ => Some("clean(url)".to_string()),
        },
        "hash" => match arg(0) {
            "sha256" | "sha512" => None,
            _ => Some("hash(sha256)".to_string()),
        },
        "wrap" | "unwrap" => match arg(0) {
            "doublequote" | "singlequote" | "backtick" => None,
            _ => Some(format!("{}(doublequote)", parsed.name)),
        },
        "color" => match arg(0) {
            "hex" | "rgb" | "rgba" | "hsl" | "hsla" => None,
            _ => Some("color(hex)".to_string()),
        },
        "lines" => {
            let action = arg(0).to_ascii_lowercase();
            let rest_ok = match action.as_str() {
                "first" | "last" | "count" | "compact" | "unique" => parsed.args.len() == 1,
                "prefix" | "suffix" | "join" | "split" => parsed.args.len() == 2,
                "sort" => parsed.args.iter().skip(1).all(|a| {
                    matches!(
                        strip_argument_quotes(a).to_ascii_lowercase().as_str(),
                        "desc" | "insensitive" | "numeric"
                    )
                }),
                _ => false,
            };
            if rest_ok {
                None
            } else {
                Some("lines(first)".to_string())
            }
        }
        "extract" => {
            if EXTRACT_TARGETS.contains(&arg(0)) || extractors::is_compilable_pattern(arg(0)) {
                None
            } else {
                Some("extract(email)".to_string())
            }
        }
        "html" | "xml" => {
            if extractors::is_valid_selector(arg(0)) {
                None
            } else {
                Some("html(div.content)".to_string())
            }
        }
        "regex" => {
            if extractors::is_compilable_pattern(arg(0)) {
                None
            } else {
                Some("regex(\"a+\")".to_string())
            }
        }
        _ => None,
    }
}

/// Bare transformer name from a segment, even when its arguments do not
/// parse (unbalanced quotes or parens). None for unknown names.
fn transformer_name_only(segment: &str) -> Option<&str> {
    let segment = segment.trim();
    let end = segment.find('(').unwrap_or(segment.len());
    let name = segment[..end].trim();
    TRANSFORMERS.contains(&name).then_some(name)
}

/// Canonical example form for a known transformer name.
fn canonical_example(name: &str) -> String {
    match name {
        "case" => "case(upper)",
        "lines" => "lines(first)",
        "replace" => "replace(\"a\", \"b\")",
        "regex" | "extract" => "regex(\"a+\")",
        "calc" => "calc(\"* 2\")",
        _ => return format!("{name}(...)"),
    }
    .to_string()
}

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

    case::apply(parsed.name, &parsed.args, content)
        .or_else(|| text::apply(parsed.name, &parsed.args, content))
        .or_else(|| encoding::apply(parsed.name, &parsed.args, content))
        .or_else(|| crypto::apply(parsed.name, &parsed.args, content))
        .or_else(|| lines::apply(parsed.name, &parsed.args, content))
        .or_else(|| formatting::apply(parsed.name, &parsed.args, content))
        .or_else(|| color::apply(parsed.name, &parsed.args, content))
        .or_else(|| calc::apply(parsed.name, &parsed.args, content))
        .or_else(|| ai::apply(parsed.name, &parsed.args, content))
        .or_else(|| extractors::apply(parsed.name, &parsed.args, content))
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
            split_pipeline("clip | truncate(5)"),
            vec!["clip", "truncate(5)"]
        );
        assert_eq!(
            split_pipeline("clip | replace(\",\", \";\") | case(upper)"),
            vec!["clip", "replace(\",\", \";\")", "case(upper)"]
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
            apply("replace(regex, \"([a-z]),([A-Z])\", \"$1 $2\")", "a,B"),
            Some("a B".to_string())
        );
        assert_eq!(apply("slice(1, 3)", "aßc"), Some("ßc".to_string()));
        assert_eq!(apply("count(chars)", "aßc"), Some("3".to_string()));
    }

    #[test]
    fn test_case_transformer_integration() {
        assert_eq!(apply("case(upper)", "hello"), Some("HELLO".to_string()));
        assert_eq!(apply("case(lower)", "HELLO"), Some("hello".to_string()));
        assert_eq!(
            apply("case(snake)", "HelloWorld"),
            Some("hello_world".to_string())
        );
        assert_eq!(
            apply("case(slug)", "Hello World 2026!"),
            Some("hello-world-2026".to_string())
        );
    }

    #[test]
    fn test_color_transformer_integration() {
        assert_eq!(
            apply("color(hex)", "rgb(255, 0, 0)"),
            Some("#FF0000".to_string())
        );
        assert_eq!(
            apply("color(rgb)", "#ff0000"),
            Some("rgb(255, 0, 0)".to_string())
        );
    }

    #[test]
    fn test_encoding_transformer_integration() {
        assert_eq!(
            apply("encode(url)", "hello world!"),
            Some("hello%20world%21".to_string())
        );
        assert_eq!(
            apply("encode(base64)", "hello"),
            Some("aGVsbG8=".to_string())
        );
    }

    #[test]
    fn test_hash_transformer_integration() {
        assert_eq!(
            apply("hash(sha256)", "hello"),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_string())
        );
    }

    #[test]
    fn test_json_pretty_minify_dispatch() {
        assert!(is_valid_transformer("json(pretty)"));
        assert!(is_valid_transformer("json(minify)"));
        assert!(!is_valid_transformer("json.pretty"));
        assert!(!is_valid_transformer("json.minify"));
        assert!(!is_valid_transformer("pretty"));
        assert!(!is_valid_transformer("minify"));
        let pretty = apply("json(pretty)", r#"{"a":1}"#).unwrap();
        assert!(pretty.contains('\n'));
        assert_eq!(
            apply("json(minify)", &pretty),
            Some(r#"{"a":1}"#.to_string())
        );
    }

    #[test]
    fn test_wrap_transformer_integration() {
        assert_eq!(
            apply("wrap(doublequote)", "hello"),
            Some("\"hello\"".to_string())
        );
        assert_eq!(
            apply("wrap(singlequote)", "hello"),
            Some("'hello'".to_string())
        );
        assert_eq!(
            apply("unwrap(doublequote)", "\"hello\""),
            Some("hello".to_string())
        );
        assert_eq!(
            apply("unwrap(singlequote)", "'hello'"),
            Some("hello".to_string())
        );
        assert_eq!(
            apply("unwrap(backtick)", "`hello`"),
            Some("hello".to_string())
        );
    }
}
