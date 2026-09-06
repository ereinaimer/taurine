use super::system;

const SYSTEM_ROOTS: &[&str] = &[
    "cursor",
    "clip",
    "clipboard",
    "time",
    "date",
    "datetime",
    "uuid",
    "env",
    "net",
    "exec",
    "random",
    "key",
    "delay",
    "lorem",
    "file",
    "use",
    "http",
    "mouse",
    "img",
];

const TIME_METHODS: &[&str] = &["utc", "calc(±...)", "format(...)"];
const DATE_METHODS: &[&str] = &["utc", "calc(±...)", "format(...)"];
const DATETIME_METHODS: &[&str] = &["utc", "calc(±...)", "format(...)"];

const UUID_MODIFIERS: &[&str] = &["v4", "v7"];
const NET_MODIFIERS: &[&str] = &["ip", "lip", "online"];
const EXEC_MODIFIERS: &[&str] = &[
    "exec.<lang>(...)",
    "exec.silent.<lang>(...)",
    "exec.<lang>.file(...).args(...)",
];
const RANDOM_MODIFIERS: &[&str] = &[
    "int(min, max)",
    "choice(a, b, ...)",
    "str(len)",
    "pass(len)",
];
const LOREM_MODIFIERS: &[&str] = &["word(n)", "sentence(n)", "paragraph(n)"];
const FILE_MODIFIERS: &[&str] = &["read(path)", "read_line(path, start, [end])"];
const KEY_MODIFIERS: &[&str] = &[
    "enter",
    "tab",
    "space",
    "esc",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pgup",
    "pageup",
    "pgdown",
    "pagedown",
    "insert",
    "ins",
    "backspace",
    "delete",
    "ctrl",
    "shift",
    "alt",
    "super",
    "mod",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "printscreen",
    "prtsc",
    "pause",
    "break",
    "capslock",
    "numlock",
    "scrolllock",
];

pub fn strip_global_transformers(key: &str) -> &str {
    let pipeline = system::transformers::split_pipeline(key);
    pipeline[0]
}

pub fn split_system_tag(key: &str) -> Option<(&str, Option<&str>)> {
    let base = strip_global_transformers(key);
    if base == "newline" {
        return Some(("newline", None));
    }
    if system::clip::is_clip_key(base) {
        let (root, idx_str) = if let Some(rest) = base.strip_prefix("clipboard") {
            ("clipboard", rest)
        } else if let Some(rest) = base.strip_prefix("clip") {
            ("clip", rest)
        } else {
            return None;
        };
        let modifier = if idx_str.is_empty() {
            None
        } else {
            Some(idx_str)
        };
        return Some((root, modifier));
    }

    if let Some(rest) = base.strip_prefix("key(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("key", Some(inner)));
    }
    if let Some(rest) = base.strip_prefix("delay(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("delay", Some(inner)));
    }
    if let Some(rest) = base.strip_prefix("env(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("env", Some(inner)));
    }
    if let Some(rest) = base.strip_prefix("use(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("use", Some(inner)));
    }
    if let Some(rest) = base.strip_prefix("img(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("img", Some(inner)));
    }

    let (root, modifier) = match base.split_once('.') {
        Some((root, modifier)) => (root, Some(modifier.trim()).filter(|m| !m.is_empty())),
        None => (base, None),
    };

    SYSTEM_ROOTS.contains(&root).then_some((root, modifier))
}

pub fn valid_modifier_hint(root: &str) -> String {
    match root {
        "cursor" => "Valid form: [cursor]".to_string(),
        "clip" => "Valid forms: [clip], [clip(0)], [clip(1)], [clip(2)]"
            .to_string(),
        "clipboard" => "Valid forms: [clipboard], [clipboard(0)], [clipboard(1)], [clipboard(2)]"
            .to_string(),
        "time" => format!("Valid modifiers / methods: {}", TIME_METHODS.join(", ")),
        "date" => format!("Valid modifiers / methods: {}", DATE_METHODS.join(", ")),
        "datetime" => format!("Valid modifiers / methods: {}", DATETIME_METHODS.join(", ")),
        "uuid" => "Valid forms: [uuid], [uuid.v4], [uuid.v7]".to_string(),
        "env" => "Valid form: [env(<var_name>)] or [env(\"<var_name>\")]".to_string(),
        "net" => format!("Valid modifiers: {}", NET_MODIFIERS.join(", ")),
        "exec" => "Valid forms: [exec.bash(...)], [exec.powershell(...)], [exec.python(...)], [exec.node(...)], [exec.cmd(...)]".to_string(),
        "random" => format!("Valid modifiers: {}", RANDOM_MODIFIERS.join(", ")),
        "lorem" => "Valid forms: [lorem], [lorem.word([n])], [lorem.sentence([n])], [lorem.paragraph([n])]".to_string(),
        "file" => format!("Valid modifiers: {}", FILE_MODIFIERS.join(", ")),
        "key" => format!(
            "Valid forms: [key(<token>)]. Tokens: {}. You can combine them with `+`, and any single character token is also allowed.",
            KEY_MODIFIERS.join(", ")
        ),
        "delay" => "Valid form: [delay(<ms>)] or [delay(<u64>ms)]".to_string(),
        "use" => "Valid form: [use(\"trigger_name\")]".to_string(),
        "http" => "Valid forms: [http.get(<url>)], [http.status(<url>)]".to_string(),
        "mouse" => "Valid forms: [mouse.click([btn])], [mouse.dblclick([btn])], [mouse.down([btn])], [mouse.up([btn])], [mouse.rclick], [mouse.mclick], [mouse.m4], [mouse.m5], [mouse.move(x,y)], [mouse.scroll(delta)], [mouse.pos]. Buttons: left, right, middle, m4, m5, m<N>".to_string(),
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

pub use validation::{ValidationError, validate_system_tag};
