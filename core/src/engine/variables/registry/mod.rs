use crate::engine::variables::system;

// honey: allow(dead_code) until Tasks 3+ consume the catalog in non-test
// code (tests pin it; system_variable_roots stays legacy until migration).
#[allow(dead_code)]
const SYSTEM_ROOTS: &[&str] = &[
    "chrono", "clip", "uuid", "random", "lorem", "file", "ip", "http", "env", "execute", "mouse",
    "key", "delay", "use", "image", "cursor", "newline",
];

// honey: legacy dot-chain roots for pre-migration callers (interpolate, plan,
// triggers validate still use split_system_tag + validate_system_tag); Tasks 3/8
// delete this with split_system_tag once callers move to parse_system_call.
const LEGACY_ROOTS: &[&str] = &[
    "cursor",
    "clipboard",
    "time",
    "date",
    "datetime",
    "uuid",
    "env",
    "net",
    "execute",
    "random",
    "key",
    "delay",
    "lorem",
    "file",
    "use",
    "http",
    "mouse",
    "image",
];

const TIME_METHODS: &[&str] = &["utc", "calc(±...)", "format(...)"];
const DATE_METHODS: &[&str] = &["utc", "calc(±...)", "format(...)"];
const DATETIME_METHODS: &[&str] = &["utc", "calc(±...)", "format(...)"];

const UUID_MODIFIERS: &[&str] = &["v4", "v7"];
const NET_MODIFIERS: &[&str] = &["publicip", "localip", "online"];
const EXEC_MODIFIERS: &[&str] = &[
    "execute.<lang>(...)",
    "execute.silent.<lang>(...)",
    "execute.<lang>.file(...).args(...)",
];
const RANDOM_MODIFIERS: &[&str] = &[
    "int([min], [max])",
    "choice(a, b, ...)",
    "str([len])",
    "pass([len])",
];
const LOREM_MODIFIERS: &[&str] = &["(n)", "words(n)", "sentences(n)", "paragraphs(n)"];
const FILE_MODIFIERS: &[&str] = &["read(path)", "line(path, n)", "lines(path, start, [end])"];
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

/// Splits a unified `namespace(args)` tag into namespace + raw args.
/// Bare tags are `()` sugar: `clip` parses as `("clip", "")`.
/// Purely syntactic: unknown and dotted namespaces are preserved for
/// `validate_system_call` to reject with a canonical-form hint.
pub fn parse_system_call(inner: &str) -> Option<(&str, &str)> {
    let pipeline = system::transformers::split_pipeline(inner);
    let s = pipeline.first()?.trim();
    if s.is_empty() {
        return None;
    }
    match s.find('(') {
        None => Some((s, "")),
        Some(i) => {
            let ns = s[..i].trim();
            if ns.is_empty() || !s.ends_with(')') {
                return None;
            }
            Some((ns, s[i + 1..s.len() - 1].trim()))
        }
    }
}

// honey: migration glue (see LEGACY_ROOTS); Tasks 3/8 own deletion + callers.
pub fn split_system_tag(key: &str) -> Option<(&str, Option<&str>)> {
    let base = strip_global_transformers(key);
    if base == "newline" {
        return Some(("newline", None));
    }
    if system::clipboard::is_clip_key(base) {
        let rest = base
            .strip_prefix("clipboard")
            .or_else(|| base.strip_prefix("clip"))
            .unwrap_or("");
        let modifier = if rest.is_empty() { None } else { Some(rest) };
        return Some(("clipboard", modifier));
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
    if let Some(rest) = base.strip_prefix("image(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("image", Some(inner)));
    }
    if let Some(rest) = base.strip_prefix("lorem(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return Some(("lorem", Some(inner)));
    }

    let (root, modifier) = match base.split_once('.') {
        Some((root, modifier)) => (root, Some(modifier.trim()).filter(|m| !m.is_empty())),
        None => (base, None),
    };

    LEGACY_ROOTS.contains(&root).then_some((root, modifier))
}

pub fn valid_modifier_hint(root: &str) -> String {
    match root {
        "cursor" => "Valid form: [cursor]".to_string(),
        "clipboard" => "Valid forms: [clip], [clipboard], [clip(1)], [clip(2)]"
            .to_string(),
        "time" => format!("Valid modifiers / methods: {}", TIME_METHODS.join(", ")),
        "date" => format!("Valid modifiers / methods: {}", DATE_METHODS.join(", ")),
        "datetime" => format!("Valid modifiers / methods: {}", DATETIME_METHODS.join(", ")),
        "uuid" => "Valid forms: [uuid], [uuid.v4], [uuid.v7]".to_string(),
        "env" => "Valid form: [env(<var_name>)] or [env(\"<var_name>\")]".to_string(),
        "net" => format!("Valid modifiers: {}", NET_MODIFIERS.join(", ")),
        "execute" => "Valid forms: [execute.bash(...)], [execute.powershell(...)], [execute.python(...)], [execute.node(...)], [execute.cmd(...)]".to_string(),
        "random" => "Valid forms: [random], [random.int([min], [max])], [random.choice(...)], [random.str([len])], [random.pass([len])]".to_string(),
        "lorem" => "Valid forms: [lorem], [lorem([n])], [lorem.words([n])], [lorem.sentences([n])], [lorem.paragraphs([n])]".to_string(),
        "file" => format!("Valid modifiers: {}", FILE_MODIFIERS.join(", ")),
        "key" => format!(
            "Valid forms: [key(<token>)]. Tokens: {}. You can combine them with +, and any single character token is also allowed.",
            KEY_MODIFIERS.join(", ")
        ),
        "delay" => "Valid form: [delay(<ms>)] or [delay(<u64>ms)] or [delay(<f64>s)]".to_string(),
        "use" => "Valid form: [use(\"trigger_name\")]".to_string(),
        "http" => "Valid forms: [http.get(<url>)], [http.status(<url>)]".to_string(),
        "mouse" => "Valid directives:\n  [mouse.click(btn)]    Click button (default: left)\n  [mouse.dblclick(btn)] Double-click button (default: left)\n  [mouse.hold(btn)]     Press and hold button\n  [mouse.release(btn)]  Release button\n  [mouse.rclick]        Right-click shortcut\n  [mouse.mclick]        Middle-click shortcut\n  [mouse.m4]            Back button shortcut (mouse4)\n  [mouse.m5]            Forward button shortcut (mouse5)\n  [mouse.move(x, y)]    Move cursor to absolute coordinates (x, y)\n  [mouse.scroll(delta)] Scroll wheel vertically (positive: up, negative: down)\n  [mouse.pos]           Insert current cursor position as x, y\n\nSupported buttons:\n  left, right, middle, m4 (back), m5 (forward), m<N>".to_string(),
        "image" => "Valid form: [image(path/to/image.png)]".to_string(),
        "newline" => "Valid form: [newline]".to_string(),
        _ => "No modifier help available.".to_string(),
    }
}

// honey: returns LEGACY_ROOTS until Tasks 3/8 migrate split_system_tag
// callers (a triggers test pins the [clipboard] suggestion); flip to
// SYSTEM_ROOTS with that migration.
pub fn system_variable_roots() -> &'static [&'static str] {
    LEGACY_ROOTS
}

pub fn system_transformers() -> &'static [&'static str] {
    system::transformers::TRANSFORMERS
}

pub fn is_valid_system_root(root: &str) -> bool {
    let cleaned = root.trim().to_lowercase();
    LEGACY_ROOTS.contains(&cleaned.as_str())
}

pub fn is_valid_transformer(name: &str) -> bool {
    let cleaned = name.trim().to_lowercase();
    let base = cleaned.split('(').next().unwrap_or(&cleaned).trim();
    system::transformers::TRANSFORMERS.contains(&base)
}

#[cfg(test)]
mod tests;
mod validation;

pub use validation::{
    ValidationError, deleted_root_hint, param_spec, validate_system_call, validate_system_tag,
};
