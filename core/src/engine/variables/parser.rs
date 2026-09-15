use super::types::ArgMap;
use indexmap::IndexMap;

pub struct Param<'a> {
    pub name: &'a str,
    pub required: bool,
    pub default: &'a str,
}

pub struct ParamSpec<'a> {
    pub params: &'a [Param<'a>],
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct BoundArgs {
    pub positional: Vec<String>,
    pub named: IndexMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindError {
    #[error("unknown parameter '{key}' for '{namespace}' (expected: {hint})")]
    UnknownKey {
        key: String,
        namespace: String,
        hint: String,
    },
    #[error("duplicate value for parameter '{param}' in '{namespace}' (expected: {hint})")]
    Duplicate {
        param: String,
        namespace: String,
        hint: String,
    },
    #[error("missing required parameter '{param}' for '{namespace}' (expected: {hint})")]
    MissingRequired {
        param: String,
        namespace: String,
        hint: String,
    },
    #[error("positional value after named parameter in '{namespace}' (expected: {hint})")]
    PositionalAfterNamed { namespace: String, hint: String },
    #[error("wrong number of arguments for '{namespace}' (expected: {hint})")]
    Arity { namespace: String, hint: String },
}

fn usage_hint(namespace: &str, spec: &ParamSpec) -> String {
    let parts: Vec<String> = spec
        .params
        .iter()
        .map(|p| {
            if p.required {
                p.name.to_string()
            } else {
                format!("[{}={}]", p.name, p.default)
            }
        })
        .collect();
    format!("{}({})", namespace, parts.join(", "))
}

fn split_call_args(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut paren: usize = 0;
    let mut bracket: usize = 0;
    for c in raw.chars() {
        if let Some(q) = quote {
            current.push(c);
            if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
            current.push(c);
        } else if c == '(' {
            paren += 1;
            current.push(c);
        } else if c == ')' {
            paren = paren.saturating_sub(1);
            current.push(c);
        } else if c == '[' {
            bracket += 1;
            current.push(c);
        } else if c == ']' {
            bracket = bracket.saturating_sub(1);
            current.push(c);
        } else if c == ',' && paren == 0 && bracket == 0 {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    parts.push(current);
    parts
}

fn split_named(token: &str) -> Option<(&str, &str)> {
    let mut quote: Option<char> = None;
    for (i, c) in token.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '=' {
            return Some((&token[..i], &token[i + 1..]));
        }
    }
    None
}

pub fn bind_call(namespace: &str, raw: &str, spec: &ParamSpec) -> Result<BoundArgs, BindError> {
    let hint = usage_hint(namespace, spec);
    let mut values: Vec<Option<String>> = vec![None; spec.params.len()];
    let mut bound = BoundArgs::default();
    let mut seen_named = false;
    for part in split_call_args(raw) {
        let piece = part.trim();
        if strip_quotes(piece).is_empty() {
            continue;
        }
        // honey: split before unquoting so a quoted `=` stays a positional value.
        if let Some((raw_key, raw_value)) = split_named(piece) {
            let key = strip_quotes(raw_key.trim()).to_string();
            let value = strip_quotes(raw_value.trim()).to_string();
            let Some(index) = spec
                .params
                .iter()
                .position(|p| p.name.eq_ignore_ascii_case(&key))
            else {
                return Err(BindError::UnknownKey {
                    key,
                    namespace: namespace.to_string(),
                    hint,
                });
            };
            if values[index].is_some() {
                return Err(BindError::Duplicate {
                    param: key,
                    namespace: namespace.to_string(),
                    hint,
                });
            }
            values[index] = Some(value);
            seen_named = true;
        } else {
            if seen_named {
                return Err(BindError::PositionalAfterNamed {
                    namespace: namespace.to_string(),
                    hint,
                });
            }
            let value = strip_quotes(piece).to_string();
            bound.positional.push(value.clone());
            if let Some(index) = values.iter().position(Option::is_none) {
                values[index] = Some(value);
            }
        }
    }
    for (param, slot) in spec.params.iter().zip(values.iter()) {
        match slot {
            Some(value) => {
                bound.named.insert(param.name.to_string(), value.clone());
            }
            None if param.required => {
                return Err(BindError::MissingRequired {
                    param: param.name.to_string(),
                    namespace: namespace.to_string(),
                    hint,
                });
            }
            None => {
                bound
                    .named
                    .insert(param.name.to_string(), param.default.to_string());
            }
        }
    }
    Ok(bound)
}

/// True when any parameter beyond the positional prefix carries a non-default
/// value, i.e. it was supplied as `key=value` rather than positionally.
/// A default passed explicitly as `key=default` is indistinguishable from an
/// absent one — and resolves identically — so it does not count.
pub fn has_named_args(bound: &BoundArgs, spec: &ParamSpec) -> bool {
    let prefix = bound.positional.len().min(spec.params.len());
    spec.params.iter().skip(prefix).any(|p| {
        bound
            .named
            .get(p.name)
            .map(String::as_str)
            .unwrap_or(p.default)
            != p.default
    })
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 {
        let first = match s.chars().next() {
            Some(first) => first,
            None => return s,
        };
        let last = match s.chars().last() {
            Some(last) => last,
            None => return s,
        };
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return &s[first.len_utf8()..s.len() - last.len_utf8()];
        }
    }
    s
}

pub fn tokenize(raw: &str, delimiter: char) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }

    let mut tokens = Vec::new();
    let mut current_token = String::new();
    let mut active_quote: Option<char> = None;

    for c in raw.chars() {
        if (c == '"' || c == '\'') && (active_quote.is_none() || active_quote == Some(c)) {
            if active_quote.is_some() {
                active_quote = None;
            } else {
                active_quote = Some(c);
            }
            current_token.push(c);
        } else if c == delimiter && active_quote.is_none() {
            tokens.push(current_token.clone());
            current_token.clear();
        } else {
            current_token.push(c);
        }
    }

    tokens.push(current_token);
    tokens
}

pub fn parse_tokens(tokens: &[String]) -> ArgMap {
    let mut map = ArgMap::default();

    for token in tokens {
        let token = token.trim();
        if token.is_empty() {
            map.positional.push(String::new());
            continue;
        }

        let token = strip_quotes(token);

        if let Some((key, value)) = token.split_once('=') {
            map.named.insert(
                strip_quotes(key).to_string(),
                strip_quotes(value).to_string(),
            );
        } else {
            map.positional.push(token.to_string());
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        assert_eq!(
            tokenize(r#"foo:bar:"baz:qux""#, ':'),
            vec!["foo", "bar", "\"baz:qux\""]
        );
        assert_eq!(tokenize("ereinaimer", ':'), vec!["ereinaimer"]);
        assert_eq!(tokenize("", ':'), Vec::<String>::new());
    }

    #[test]
    fn test_parse_tokens_positional() {
        let tokens = vec!["ereinaimer".to_string(), "taurine".to_string()];
        let map = parse_tokens(&tokens);
        assert_eq!(map.positional, vec!["ereinaimer", "taurine"]);
        assert!(map.named.is_empty());
    }

    #[test]
    fn test_parse_tokens_named() {
        let tokens = vec![
            "username=ereinaimer".to_string(),
            "repo=taurine".to_string(),
        ];
        let map = parse_tokens(&tokens);
        assert_eq!(map.named.get("username").unwrap(), "ereinaimer");
        assert_eq!(map.named.get("repo").unwrap(), "taurine");
        assert!(map.positional.is_empty());
    }

    #[test]
    fn test_parse_tokens_quoted_values() {
        let tokens = vec!["name=\"John Doe\"".to_string(), "repo=taurine".to_string()];
        let map = parse_tokens(&tokens);
        assert_eq!(map.named.get("name").unwrap(), "John Doe");
        assert_eq!(map.named.get("repo").unwrap(), "taurine");
        assert!(map.positional.is_empty());
    }

    #[test]
    fn test_parse_tokens_mixed() {
        let tokens = vec![
            "first".to_string(),
            "\"second arg\"".to_string(),
            "key=\"val\"".to_string(),
            "another=123".to_string(),
        ];
        let map = parse_tokens(&tokens);
        assert_eq!(map.positional, vec!["first", "second arg"]);
        assert_eq!(map.named.get("key").unwrap(), "val");
        assert_eq!(map.named.get("another").unwrap(), "123");
    }

    #[test]
    fn test_parse_tokens_single_quoted() {
        let tokens = vec![
            "name='Neil Armstrong'".to_string(),
            "repo=taurine".to_string(),
        ];
        let map = parse_tokens(&tokens);
        assert_eq!(map.named.get("name").unwrap(), "Neil Armstrong");
        assert_eq!(map.named.get("repo").unwrap(), "taurine");
        assert!(map.positional.is_empty());
    }

    mod compatibility_parser_tests {
        use super::*;

        #[test]
        fn tokenize_keeps_colons_inside_single_and_double_quotes() {
            assert_eq!(
                tokenize(r#"alpha:'beta:gamma':"delta:epsilon":zeta"#, ':'),
                vec!["alpha", "'beta:gamma'", "\"delta:epsilon\"", "zeta"]
            );
        }

        #[test]
        fn parse_tokens_preserves_empty_arguments() {
            let tokens = tokenize("alpha::beta:", ':');
            let map = parse_tokens(&tokens);

            assert_eq!(map.positional, vec!["alpha", "", "beta", ""]);
            assert!(map.named.is_empty());
        }

        #[test]
        fn parse_tokens_uses_first_equals_for_named_values() {
            let tokens = vec![
                "query=foo=bar=baz".to_string(),
                "formula=\"x=1:y=2\"".to_string(),
            ];
            let map = parse_tokens(&tokens);

            assert_eq!(map.named.get("query").unwrap(), "foo=bar=baz");
            assert_eq!(map.named.get("formula").unwrap(), "x=1:y=2");
        }

        #[test]
        fn parse_tokens_strips_outer_quotes_from_whole_named_pair() {
            let tokens = vec![
                "\"name=Neil Armstrong\"".to_string(),
                "'repo=taurine'".to_string(),
            ];
            let map = parse_tokens(&tokens);

            assert_eq!(map.named.get("name").unwrap(), "Neil Armstrong");
            assert_eq!(map.named.get("repo").unwrap(), "taurine");
        }

        #[test]
        fn parse_tokens_preserves_spaces_inside_quotes_for_hybrid_arguments() {
            let tokens = vec!["\"bye \"".to_string(), "4".to_string()];
            let map = parse_tokens(&tokens);
            assert_eq!(map.positional[0], "bye ");
            assert_eq!(map.positional[1], "4");
        }
    }

    mod bind_call_tests {
        use super::*;

        fn two_param_spec() -> ParamSpec<'static> {
            ParamSpec {
                params: &[
                    Param {
                        name: "a",
                        required: true,
                        default: "",
                    },
                    Param {
                        name: "b",
                        required: false,
                        default: "2",
                    },
                ],
            }
        }

        #[test]
        fn bind_positional_and_named() {
            let spec = ParamSpec {
                params: &[
                    Param {
                        name: "type",
                        required: false,
                        default: "int",
                    },
                    Param {
                        name: "min",
                        required: false,
                        default: "0",
                    },
                ],
            };
            let b = bind_call("random", "choice", &spec).unwrap();
            assert_eq!(b.positional, vec!["choice".to_string()]);
            assert_eq!(b.named.get("type").unwrap(), "choice");
            assert_eq!(b.named.get("min").unwrap(), "0");
        }

        #[test]
        fn bind_empty_args_use_defaults() {
            let spec = ParamSpec {
                params: &[Param {
                    name: "b",
                    required: false,
                    default: "2",
                }],
            };
            let b = bind_call("f", "", &spec).unwrap();
            assert!(b.positional.is_empty());
            assert_eq!(b.named.get("b").unwrap(), "2");
        }

        #[test]
        fn bind_positional_fill() {
            let spec = two_param_spec();
            let b = bind_call("f", "0, 1", &spec).unwrap();
            assert_eq!(b.positional, vec!["0".to_string(), "1".to_string()]);
            assert_eq!(b.named.get("a").unwrap(), "0");
            assert_eq!(b.named.get("b").unwrap(), "1");
        }

        #[test]
        fn bind_named_skip() {
            let spec = two_param_spec();
            let b = bind_call("f", "0, b=5", &spec).unwrap();
            assert_eq!(b.positional, vec!["0".to_string()]);
            assert_eq!(b.named.get("a").unwrap(), "0");
            assert_eq!(b.named.get("b").unwrap(), "5");
        }

        #[test]
        fn bind_variadic_tail_stays_positional() {
            let spec = two_param_spec();
            let b = bind_call("f", "0, 1, extra", &spec).unwrap();
            assert_eq!(
                b.positional,
                vec!["0".to_string(), "1".to_string(), "extra".to_string()]
            );
            assert_eq!(b.named.get("a").unwrap(), "0");
            assert_eq!(b.named.get("b").unwrap(), "1");
        }

        #[test]
        fn bind_keys_case_insensitive_values_verbatim() {
            let spec = ParamSpec {
                params: &[Param {
                    name: "file",
                    required: true,
                    default: "",
                }],
            };
            let b = bind_call("f", "FILE=True", &spec).unwrap();
            assert_eq!(b.named.get("file").unwrap(), "True");
            let err = bind_call("f", "file=a, FILE=b", &spec).unwrap_err();
            assert!(matches!(err, BindError::Duplicate { .. }));
        }

        #[test]
        fn bind_unknown_key() {
            let spec = two_param_spec();
            let err = bind_call("f", "0, c=1", &spec).unwrap_err();
            assert!(matches!(err, BindError::UnknownKey { .. }));
            assert!(err.to_string().contains("expected: f(a, [b=2])"));
        }

        #[test]
        fn bind_duplicate() {
            let spec = two_param_spec();
            let err = bind_call("f", "0, a=1", &spec).unwrap_err();
            assert!(matches!(err, BindError::Duplicate { .. }));
            assert!(err.to_string().contains("expected: f(a, [b=2])"));
        }

        #[test]
        fn bind_missing_required() {
            let spec = two_param_spec();
            let err = bind_call("f", "b=5", &spec).unwrap_err();
            assert!(matches!(err, BindError::MissingRequired { .. }));
            assert!(err.to_string().contains("expected: f(a, [b=2])"));
        }

        #[test]
        fn bind_positional_after_named() {
            let spec = two_param_spec();
            let err = bind_call("f", "a=0, 1", &spec).unwrap_err();
            assert!(matches!(err, BindError::PositionalAfterNamed { .. }));
            assert!(err.to_string().contains("expected: f(a, [b=2])"));
        }

        #[test]
        fn bind_arity_hint_rendering() {
            let err = BindError::Arity {
                namespace: "f".to_string(),
                hint: "f(a, [b=2])".to_string(),
            };
            assert_eq!(
                err.to_string(),
                "wrong number of arguments for 'f' (expected: f(a, [b=2]))"
            );
        }

        #[test]
        fn bind_quoted_comma_and_paren_values() {
            let spec = two_param_spec();
            let b = bind_call("f", "\"x,y\", b=2", &spec).unwrap();
            assert_eq!(b.positional, vec!["x,y".to_string()]);
            assert_eq!(b.named.get("a").unwrap(), "x,y");
            let b = bind_call("f", "sum(1,2), [a,b]", &spec).unwrap();
            assert_eq!(
                b.positional,
                vec!["sum(1,2)".to_string(), "[a,b]".to_string()]
            );
            assert_eq!(b.named.get("a").unwrap(), "sum(1,2)");
            assert_eq!(b.named.get("b").unwrap(), "[a,b]");
        }

        #[test]
        fn bind_quoted_equals_stays_positional() {
            use crate::engine::variables::parser::has_named_args;
            let spec = two_param_spec();
            let b = bind_call("f", "\"a=b\", 1", &spec).unwrap();
            assert_eq!(b.positional, vec!["a=b".to_string(), "1".to_string()]);
            assert_eq!(b.named.get("a").unwrap(), "a=b");
            assert!(!has_named_args(&b, &spec));
            let b = bind_call("f", "0, b=\"x=y\"", &spec).unwrap();
            assert_eq!(b.named.get("b").unwrap(), "x=y");
            assert!(has_named_args(&b, &spec));
        }
    }
}
