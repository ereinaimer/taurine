use rand::{Rng, RngExt};

const ALPHANUMERIC: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const PASSWORD: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()-_=+[]{}|;:,.<>?";
pub(crate) const MAX_RANDOM_STRING_LEN: usize = 4096;

const KNOWN_TYPES: &[&str] = &["int", "choice", "str", "pass"];

/// Resolves the unified `random(...)` system variable.
///
/// `raw` is the argument list inside `random(...)` (`""` when bare),
/// bound as `(type=int, min=0, max=100)` with a variadic positional tail.
/// Single numeric arg detects `int` with that max (`random(6)` → 1..6).
pub fn resolve(raw: &str) -> Option<String> {
    let spec = crate::engine::variables::registry::param_spec("random")?;
    let bound = crate::engine::variables::parser::bind_call("random", raw, &spec).ok()?;
    let has_named = crate::engine::variables::parser::has_named_args(&bound, &spec);
    let kind = if bound.positional.is_empty() {
        bound
            .named
            .get("type")
            .map(String::as_str)
            .unwrap_or("int")
            .to_string()
    } else if KNOWN_TYPES.contains(&bound.positional[0].as_str()) {
        bound.positional[0].clone()
    } else if bound.positional[0].parse::<i64>().is_ok() {
        "int".to_string()
    } else {
        return None;
    };
    let mut rng = rand::rng();

    match kind.as_str() {
        "int" => {
            if has_named {
                let min = bound.named.get("min")?.parse::<i64>().ok()?;
                let max = bound.named.get("max")?.parse::<i64>().ok()?;
                (min <= max).then(|| rng.random_range(min..=max).to_string())
            } else {
                // A leading kind word owns position 0; bare numerics are the range.
                let nums: Vec<String> =
                    if bound.positional.first().map(String::as_str) == Some("int") {
                        bound.positional.iter().skip(1).cloned().collect()
                    } else {
                        bound.positional.clone()
                    };
                let (min, max) = parse_int_range(&nums, 0, 100)?;
                Some(rng.random_range(min..=max).to_string())
            }
        }
        "choice" => {
            let opts: Vec<String> =
                if bound.positional.first().map(String::as_str) == Some("choice") {
                    bound.positional.iter().skip(1).cloned().collect()
                } else {
                    bound.positional.clone()
                };
            if opts.is_empty() {
                return None;
            }
            if has_named {
                let min = bound.named.get("min").map(String::as_str).unwrap_or("0");
                let max = bound.named.get("max").map(String::as_str).unwrap_or("100");
                if min != "0" || max != "100" {
                    return None;
                }
            }
            let index = rng.random_range(0..opts.len());
            Some(opts[index].clone())
        }
        "str" | "pass" => {
            let default = if kind == "str" { 16 } else { 20 };
            let tail: Vec<String> =
                if bound.positional.first().map(String::as_str) == Some(kind.as_str()) {
                    bound.positional.iter().skip(1).cloned().collect()
                } else {
                    bound.positional.clone()
                };
            if tail.len() > 1 {
                return None;
            }
            if let Some(len_str) = tail.first() {
                if has_named {
                    return None;
                }
                let len = parse_len(std::slice::from_ref(len_str), default)?;
                Some(random_chars(&mut rng, charset(&kind), len))
            } else if has_named {
                let min = bound.named.get("min").map(String::as_str).unwrap_or("0");
                let max = bound.named.get("max").map(String::as_str).unwrap_or("100");
                match (min, max) {
                    ("0", "100") => Some(random_chars(&mut rng, charset(&kind), default)),
                    (m, "100") if m != "0" => {
                        let len = parse_len(&[m.to_string()], default)?;
                        Some(random_chars(&mut rng, charset(&kind), len))
                    }
                    ("0", m) if m != "100" => {
                        let len = parse_len(&[m.to_string()], default)?;
                        Some(random_chars(&mut rng, charset(&kind), len))
                    }
                    _ => None,
                }
            } else {
                Some(random_chars(&mut rng, charset(&kind), default))
            }
        }
        _ => None,
    }
}

fn charset(kind: &str) -> &'static [u8] {
    if kind == "pass" {
        PASSWORD
    } else {
        ALPHANUMERIC
    }
}

fn parse_int_range(args: &[String], default_min: i64, default_max: i64) -> Option<(i64, i64)> {
    let (min, max) = match args {
        [] => (default_min, default_max),
        [max] => (1, max.parse::<i64>().ok()?),
        [min, max] => (min.parse::<i64>().ok()?, max.parse::<i64>().ok()?),
        _ => return None,
    };

    (min <= max).then_some((min, max))
}

fn parse_len(args: &[String], default: usize) -> Option<usize> {
    let len = match args {
        [] => default,
        [len] => len.parse::<usize>().ok()?,
        _ => return None,
    };

    (len <= MAX_RANDOM_STRING_LEN).then_some(len)
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
    fn random_unified() {
        assert!(matches!(
            resolve(""),
            Some(value) if (0..=100).contains(&value.parse::<i64>().unwrap())
        )); // bare = int 0..100
        assert_eq!(resolve("int, 5, 5"), Some("5".to_string()));
        assert!(matches!(
            resolve("6"),
            Some(value) if (1..=6).contains(&value.parse::<i64>().unwrap())
        )); // numeric single-arg → int max=6
        assert!(matches!(
            resolve("type=int, min=1, max=6"),
            Some(value) if (1..=6).contains(&value.parse::<i64>().unwrap())
        ));
        assert_eq!(resolve("choice, only"), Some("only".to_string()));
        assert!(matches!(
            resolve("choice, alpha, beta").as_deref(),
            Some("alpha") | Some("beta")
        ));
        let str_val = resolve("str").unwrap();
        assert_eq!(str_val.len(), 16);
        assert_charset(&str_val, ALPHANUMERIC);
        assert_eq!(resolve("str, 8").unwrap().len(), 8);
        assert_eq!(resolve("pass").unwrap().len(), 20);
        assert!(matches!(
            resolve("0, 100"),
            Some(value) if (0..=100).contains(&value.parse::<i64>().unwrap())
        )); // bare pair = full range
        assert!(matches!(
            resolve("choice, \"a=b\", c").as_deref(),
            Some("a=b") | Some("c")
        )); // quoted = stays positional
        assert_eq!(resolve("bogus"), None);
        assert_eq!(resolve("int, 10, 5"), None);
        assert_eq!(resolve("choice"), None);
    }
}
