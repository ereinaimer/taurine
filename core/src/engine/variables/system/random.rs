use rand::Rng;

const ALPHANUMERIC: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const NUMERIC: &[u8] = b"0123456789";
const HEX: &[u8] = b"0123456789abcdef";
const PASSWORD: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()-_=+[]{}|;:,.<>?";
const MAX_RANDOM_STRING_LEN: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RandomInvocation {
    pub variant: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RandomParseError {
    MissingVariant,
    MissingParentheses,
    UnbalancedParentheses,
    InvalidTrailingSyntax,
}

pub(crate) fn parse_invocation(key: &str) -> Result<RandomInvocation, RandomParseError> {
    let rest = key
        .strip_prefix("random.")
        .ok_or(RandomParseError::MissingVariant)?;

    let variant_end = rest.find('(').unwrap_or(rest.len());
    let variant = rest[..variant_end].trim();
    if variant.is_empty() {
        return Err(RandomParseError::MissingVariant);
    }

    let (args, trailing) = if variant_end == rest.len() {
        if rest.contains(')') {
            return Err(RandomParseError::UnbalancedParentheses);
        }
        (Vec::new(), "")
    } else {
        let (args, trailing) = scan_parenthesized(&rest[variant_end..])?;
        (split_args(&args), trailing)
    };

    if !trailing.trim().is_empty() {
        return Err(RandomParseError::InvalidTrailingSyntax);
    }

    Ok(RandomInvocation {
        variant: variant.to_string(),
        args,
    })
}

pub fn resolve(key: &str) -> Option<String> {
    let invocation = parse_invocation(key).ok()?;
    let mut rng = rand::rng();

    match invocation.variant.as_str() {
        "int" => {
            let (min, max) = parse_int_range(&invocation.args, 0, 99)?;
            Some(rng.random_range(min..=max).to_string())
        }
        "float" => {
            let (min, max) = parse_float_range(&invocation.args, 0.0, 1.0)?;
            Some(rng.random_range(min..=max).to_string())
        }
        "bool" => no_args(&invocation.args).then(|| rng.random_bool(0.5).to_string()),
        "choice" => {
            if invocation.args.is_empty() {
                return None;
            }
            let index = rng.random_range(0..invocation.args.len());
            Some(invocation.args[index].clone())
        }
        "string" => {
            let len = parse_len(&invocation.args, 16)?;
            Some(random_chars(&mut rng, ALPHANUMERIC, len))
        }
        "alpha" => {
            let len = parse_len(&invocation.args, 16)?;
            Some(random_chars(&mut rng, ALPHA, len))
        }
        "numeric" => {
            let len = parse_len(&invocation.args, 16)?;
            Some(random_chars(&mut rng, NUMERIC, len))
        }
        "hex" => {
            let len = parse_len(&invocation.args, 32)?;
            Some(random_chars(&mut rng, HEX, len))
        }
        "password" => {
            let len = parse_len(&invocation.args, 20)?;
            Some(random_chars(&mut rng, PASSWORD, len))
        }
        "color" => no_args(&invocation.args).then(|| {
            let value: u32 = rng.random_range(0..=0xFF_FFFF);
            format!("#{value:06X}")
        }),
        "ip" => no_args(&invocation.args).then(|| {
            format!(
                "{}.{}.{}.{}",
                rng.random_range(0..=255u8),
                rng.random_range(0..=255u8),
                rng.random_range(0..=255u8),
                rng.random_range(0..=255u8)
            )
        }),
        "mac" => no_args(&invocation.args).then(|| {
            (0..6)
                .map(|_| format!("{:02X}", rng.random_range(0..=255u8)))
                .collect::<Vec<_>>()
                .join(":")
        }),
        _ => None,
    }
}

fn scan_parenthesized(input: &str) -> Result<(String, &str), RandomParseError> {
    if !input.starts_with('(') {
        return Err(RandomParseError::MissingParentheses);
    }

    let mut depth = 0usize;
    let mut start = None;

    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => {
                if depth == 0 {
                    start = Some(idx + ch.len_utf8());
                }
                depth += 1;
            }
            ')' => {
                if depth == 0 {
                    return Err(RandomParseError::UnbalancedParentheses);
                }
                depth -= 1;
                if depth == 0 {
                    let start = start.ok_or(RandomParseError::MissingParentheses)?;
                    return Ok((input[start..idx].trim().to_string(), &input[idx + 1..]));
                }
            }
            _ => {}
        }
    }

    Err(RandomParseError::UnbalancedParentheses)
}

fn split_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;

    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                push_arg(&mut args, &input[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    push_arg(&mut args, &input[start..]);
    args
}

fn push_arg(args: &mut Vec<String>, raw: &str) {
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        args.push(trimmed.to_string());
    }
}

fn parse_int_range(args: &[String], default_min: i64, default_max: i64) -> Option<(i64, i64)> {
    let (min, max) = match args {
        [] => (default_min, default_max),
        [min, max] => (min.parse::<i64>().ok()?, max.parse::<i64>().ok()?),
        _ => return None,
    };

    (min <= max).then_some((min, max))
}

fn parse_float_range(args: &[String], default_min: f64, default_max: f64) -> Option<(f64, f64)> {
    let (min, max) = match args {
        [] => (default_min, default_max),
        [min, max] => (min.parse::<f64>().ok()?, max.parse::<f64>().ok()?),
        _ => return None,
    };

    (min.is_finite() && max.is_finite() && min <= max).then_some((min, max))
}

fn parse_len(args: &[String], default: usize) -> Option<usize> {
    let len = match args {
        [] => default,
        [len] => len.parse::<usize>().ok()?,
        _ => return None,
    };

    (len <= MAX_RANDOM_STRING_LEN).then_some(len)
}

fn no_args(args: &[String]) -> bool {
    args.is_empty()
}

fn random_chars(rng: &mut impl Rng, charset: &[u8], len: usize) -> String {
    (0..len)
        .map(|_| charset[rng.random_range(0..charset.len())] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_charset(value: &str, charset: &[u8]) {
        assert!(
            value.bytes().all(|byte| charset.contains(&byte)),
            "{value:?} contains characters outside the expected set"
        );
    }

    #[test]
    fn parses_missing_and_parenthesized_args() {
        assert_eq!(
            parse_invocation("random.int").unwrap(),
            RandomInvocation {
                variant: "int".to_string(),
                args: Vec::new(),
            }
        );
        assert_eq!(
            parse_invocation("random.int(1, 2)").unwrap(),
            RandomInvocation {
                variant: "int".to_string(),
                args: vec!["1".to_string(), "2".to_string()],
            }
        );
    }

    #[test]
    fn parses_choice_commas_outside_nested_parentheses() {
        let parsed = parse_invocation("random.choice(alpha(one, two), beta, gamma(3, 4))").unwrap();

        assert_eq!(
            parsed.args,
            vec![
                "alpha(one, two)".to_string(),
                "beta".to_string(),
                "gamma(3, 4)".to_string(),
            ]
        );
    }

    #[test]
    fn rejects_unbalanced_or_trailing_syntax() {
        assert_eq!(
            parse_invocation("random.int(1, 2"),
            Err(RandomParseError::UnbalancedParentheses)
        );
        assert_eq!(
            parse_invocation("random.int(1, 2).extra"),
            Err(RandomParseError::InvalidTrailingSyntax)
        );
    }

    #[test]
    fn resolves_int_ranges_and_rejects_invalid_ranges() {
        assert_eq!(resolve("random.int(5, 5)"), Some("5".to_string()));
        assert!(matches!(
            resolve("random.int"),
            Some(value) if (0..=99).contains(&value.parse::<i64>().unwrap())
        ));
        assert_eq!(resolve("random.int(10, 5)"), None);
        assert_eq!(resolve("random.int(nope, 5)"), None);
        assert_eq!(resolve("random.int(1)"), None);
    }

    #[test]
    fn resolves_float_ranges_and_rejects_invalid_ranges() {
        assert_eq!(resolve("random.float(2.5, 2.5)"), Some("2.5".to_string()));
        assert!(matches!(
            resolve("random.float"),
            Some(value) if (0.0..=1.0).contains(&value.parse::<f64>().unwrap())
        ));
        assert_eq!(resolve("random.float(2.0, 1.0)"), None);
        assert_eq!(resolve("random.float(nan, 1.0)"), None);
    }

    #[test]
    fn resolves_bool() {
        assert!(matches!(
            resolve("random.bool").as_deref(),
            Some("true") | Some("false")
        ));
        assert_eq!(resolve("random.bool(true)"), None);
    }

    #[test]
    fn resolves_choice_from_trimmed_options() {
        assert_eq!(resolve("random.choice(only)"), Some("only".to_string()));
        assert!(matches!(
            resolve("random.choice(alpha, beta)").as_deref(),
            Some("alpha") | Some("beta")
        ));
        assert_eq!(resolve("random.choice()"), None);
    }

    #[test]
    fn resolves_random_strings_with_expected_defaults_and_charsets() {
        let string = resolve("random.string").unwrap();
        assert_eq!(string.len(), 16);
        assert_charset(&string, ALPHANUMERIC);

        let alpha = resolve("random.alpha(12)").unwrap();
        assert_eq!(alpha.len(), 12);
        assert_charset(&alpha, ALPHA);

        let numeric = resolve("random.numeric(12)").unwrap();
        assert_eq!(numeric.len(), 12);
        assert_charset(&numeric, NUMERIC);

        let hex = resolve("random.hex").unwrap();
        assert_eq!(hex.len(), 32);
        assert_charset(&hex, HEX);

        let password = resolve("random.password(24)").unwrap();
        assert_eq!(password.len(), 24);
        assert_charset(&password, PASSWORD);
    }

    #[test]
    fn rejects_invalid_lengths() {
        assert_eq!(resolve("random.string(nope)"), None);
        assert_eq!(resolve("random.string(-1)"), None);
        assert_eq!(resolve("random.string(4097)"), None);
        assert_eq!(resolve("random.string(1, 2)"), None);
    }

    #[test]
    fn resolves_color_ip_and_mac() {
        let color = resolve("random.color").unwrap();
        assert_eq!(color.len(), 7);
        assert!(color.starts_with('#'));
        assert!(color[1..].chars().all(|ch| ch.is_ascii_hexdigit()));

        let ip = resolve("random.ip").unwrap();
        let octets = ip
            .split('.')
            .map(|part| part.parse::<u8>())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(octets.len(), 4);

        let mac = resolve("random.mac").unwrap();
        let parts = mac.split(':').collect::<Vec<_>>();
        assert_eq!(parts.len(), 6);
        assert!(
            parts
                .iter()
                .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
        );
    }
}
