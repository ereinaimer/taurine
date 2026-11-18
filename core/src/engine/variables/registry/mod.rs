use crate::engine::variables::system;

pub(crate) const SYSTEM_ROOTS: &[&str] = &[
    "date", "time", "clip", "uuid", "random", "lorem", "file", "ip", "env", "run", "mouse", "wait",
    "use", "image", "cursor", "newline", "calc",
];

pub fn strip_global_transformers(key: &str) -> &str {
    let pipeline = system::transformers::split_pipeline(key);
    pipeline[0]
}

/// Splits a unified tag into namespace/root + raw args.
/// Space-separated syntax (e.g. `date tomorrow YYYY-MM-DD`, `run "echo hi"`)
/// or legacy paren syntax are both recognized.
pub fn parse_system_call(inner: &str) -> Option<(&str, &str)> {
    let pipeline = system::transformers::split_pipeline(inner);
    let s = pipeline.first()?.trim();
    if s.is_empty() {
        return None;
    }
    let (root, rest) = match s.split_once(|c: char| c.is_whitespace() || c == '(') {
        Some((r, rem)) => (r.trim(), rem.trim()),
        None => (s, ""),
    };
    let lower_root = root.to_ascii_lowercase();
    if SYSTEM_ROOTS.contains(&lower_root.as_str())
        || validation::deleted_root_hint(&lower_root).is_some()
    {
        let clean_rest = if rest.ends_with(')') && s.contains('(') {
            &rest[..rest.len() - 1]
        } else if s.contains('(') {
            return None;
        } else {
            rest
        };
        Some((root, clean_rest.trim()))
    } else {
        None
    }
}

pub fn valid_modifier_hint(root: &str) -> String {
    match root {
        "date" => "Valid forms: [date], [date +1d], [date tomorrow], [date yesterday], [date YYYY-MM-DD]".to_string(),
        "time" => "Valid forms: [time], [time utc], [time HH:mm], [time utc HH:mm:ss]".to_string(),
        "clip" => "Valid forms: [clip], [clip 1], [clip 2]".to_string(),
        "uuid" => "Valid forms: [uuid], [uuid v7]".to_string(),
        "ip" => "Valid form: [ip] (zero arguments, returns local network IPs)".to_string(),
        "env" => "Valid forms: [env HOME], [env PORT 8080]".to_string(),
        "file" => "Valid forms: [file path], [file path 5], [file path 1..10]".to_string(),
        "run" => "Valid forms: [run \"cmd\"], [run silent \"cmd\"]".to_string(),
        "wait" => "Valid forms: [wait 200], [wait 200ms], [wait 1.5s]".to_string(),
        "calc" => "Valid forms: [calc 10 * 5], [calc (10 + 2) / 3]".to_string(),
        "random" => "Valid forms: [random], [random 6], [random 1 100], [random choice \"a\" \"b\"], [random str 16]".to_string(),
        "lorem" => "Valid forms: [lorem], [lorem 3], [lorem words 5], [lorem sentences 2]".to_string(),
        "mouse" => "Valid forms: [mouse click m1], [mouse click m2 2], [mouse move 100 200], [mouse scroll -100], [mouse pos]".to_string(),
        "use" => "Valid form: [use \"trigger_name\"]".to_string(),
        "image" => "Valid form: [image \"path/to/image.png\"]".to_string(),
        "cursor" => "Valid form: [cursor]".to_string(),
        "newline" => "Valid forms: [newline], [newline 2]".to_string(),
        _ => "No modifier help available.".to_string(),
    }
}

pub fn system_variable_roots() -> &'static [&'static str] {
    SYSTEM_ROOTS
}

pub fn system_transformers() -> &'static [&'static str] {
    system::transformers::TRANSFORMERS
}

pub fn is_valid_system_root(root: &str) -> bool {
    let cleaned = root.trim().to_lowercase();
    SYSTEM_ROOTS.contains(&cleaned.as_str())
}

pub fn is_valid_transformer(name: &str) -> bool {
    let cleaned = name.trim().to_lowercase();
    let base = cleaned.split('(').next().unwrap_or(&cleaned).trim();
    system::transformers::TRANSFORMERS.contains(&base)
}

#[cfg(test)]
mod tests;
mod validation;

pub use validation::{ValidationError, deleted_root_hint, param_spec, validate_system_call};
