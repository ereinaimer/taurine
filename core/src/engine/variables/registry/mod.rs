use crate::engine::variables::system;

pub(crate) const SYSTEM_ROOTS: &[&str] = &[
    "chrono", "clip", "uuid", "random", "lorem", "file", "ip", "http", "env", "execute", "mouse",
    "key", "delay", "use", "image", "cursor", "newline",
];

pub fn strip_global_transformers(key: &str) -> &str {
    let pipeline = system::transformers::split_pipeline(key);
    pipeline[0]
}

/// Splits a unified `namespace(args)` tag into namespace + raw args.
/// Bare tags are `()` sugar: `clip` parses as `("clip", "")`.
/// Purely syntactic: unknown and dotted namespaces are preserved for
/// `validate_system_call` to reject with a canonical-form hint.
fn extract_root(name: &str) -> &str {
    name.split('.').next().unwrap_or(name).trim()
}

pub fn parse_system_call(inner: &str) -> Option<(&str, &str)> {
    let pipeline = system::transformers::split_pipeline(inner);
    let s = pipeline.first()?.trim();
    if s.is_empty() {
        return None;
    }
    match s.find('(') {
        None => {
            let lower = s.to_ascii_lowercase();
            let root = extract_root(&lower);
            if SYSTEM_ROOTS.contains(&root) || validation::deleted_root_hint(root).is_some() {
                Some((s, ""))
            } else {
                None
            }
        }
        Some(i) => {
            let ns = s[..i].trim();
            if ns.is_empty() || !s.ends_with(')') {
                return None;
            }
            let lower_ns = ns.to_ascii_lowercase();
            let root = extract_root(&lower_ns);
            if SYSTEM_ROOTS.contains(&root) || validation::deleted_root_hint(root).is_some() {
                Some((ns, s[i + 1..s.len() - 1].trim()))
            } else {
                None
            }
        }
    }
}

pub fn valid_modifier_hint(root: &str) -> String {
    match root {
        "chrono" => "Valid forms: [chrono], [chrono(date)], [chrono(+1d)], [chrono(type=time, tz=utc)]".to_string(),
        "clip" => "Valid forms: [clip], [clip(0)], [clip(1)], [clip(2)]".to_string(),
        "uuid" => "Valid forms: [uuid], [uuid(v4)], [uuid(v7)]".to_string(),
        "ip" => "Valid forms: [ip], [ip(public)], [ip(local)]".to_string(),
        "env" => "Valid forms: [env(HOME)], [env(VAR, default)]".to_string(),
        "file" => "Valid forms: [file(read, path)], [file(line, path, n)], [file(lines, path, start, end)]".to_string(),
        "execute" => "Valid forms: [execute(bash, \"cmd\")], [execute(python, /s.py, file=true)], [execute(bash, \"cmd\", silent=true)]".to_string(),
        "random" => "Valid forms: [random], [random(6)], [random(int, 1, 10)], [random(choice, a, b)], [random(str, 16)]".to_string(),
        "lorem" => "Valid forms: [lorem], [lorem(3)], [lorem(words, 5)], [lorem(type=sentences, count=2)]".to_string(),
        "http" => "Valid forms: [http(get, url)], [http(status, url)]".to_string(),
        "mouse" => "Valid forms: [mouse(click, m1)], [mouse(click, m2, 2)], [mouse(hold, m1)], [mouse(release, m1)], [mouse(move, 100, 200)], [mouse(scroll, -100)], [mouse(pos)]".to_string(),
        "key" => "Valid form: [key(<token>)]".to_string(),
        "delay" => "Valid form: [delay(<duration>)] (e.g. [delay(200ms)])".to_string(),
        "use" => "Valid form: [use(\"trigger_name\")]".to_string(),
        "image" => "Valid form: [image(path/to/image.png)]".to_string(),
        "cursor" => "Valid form: [cursor]".to_string(),
        "newline" => "Valid form: [newline]".to_string(),
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
