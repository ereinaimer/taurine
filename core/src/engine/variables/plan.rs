use crate::engine::variables::interpolate::{contains_ai_markers, interpolate};
use crate::engine::variables::registry::{split_system_tag, validate_system_tag};
use crate::engine::variables::system::{
    self, img, parse_delay_directive, parse_key_directive, parse_mouse_directive, transformers,
};
use crate::engine::variables::tags::{
    TAG_CLOSE, TAG_OPEN, find_next_tag, split_key_default, tag_inner,
};
use crate::engine::variables::types::{ArgMap, ExpansionOrigin, ExpansionStep, FinalExpansion};

/// A single pre-compiled operation in an [`ExecutionPlan`].
#[derive(Debug, Clone, PartialEq)]
pub enum PlanOp {
    /// Static literal text.
    Literal(String),
    /// Positional argument by 0-based index.
    PositionalArg {
        index: usize,
        default_value: Option<String>,
        transformers: Vec<String>,
    },
    /// Named argument by string key.
    NamedArg {
        name: String,
        default_value: Option<String>,
        transformers: Vec<String>,
    },
    /// System variable (e.g. time, date, clip, env, uuid, lorem, net, http).
    SystemVar {
        key: String,
        transformers: Vec<String>,
    },
    /// Quoted literal text with transformers (e.g. `['hello' | upper]`).
    QuotedLiteral {
        value: String,
        transformers: Vec<String>,
    },
    /// Dynamic snippet invocation `[use(snippet_name)]`.
    Use {
        snippet_name: String,
        transformers: Vec<String>,
    },
    /// Cursor positioning directive `[cursor]`.
    Cursor,
    /// Keypress simulation directive `[key(alias)]`.
    KeyPress(String),
    /// Delay pause directive in milliseconds `[delay(ms)]`.
    Delay(u64),
    /// Mouse action directive `[mouse.*]`.
    Mouse(ExpansionStep),
    /// Image insertion directive `[img(path)]`.
    Image(ExpansionStep),
    /// Inline script execution directive `[exec.*]`.
    InlineRun {
        raw_cmd: String,
        transformers: Vec<String>,
    },
    /// Dynamic or nested expression fallback.
    Dynamic(String),
}

/// An immutable, pre-compiled template execution plan.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionPlan {
    raw_template: String,
    ops: Vec<PlanOp>,
    global_transformers: Vec<String>,
    has_directive_steps: bool,
    has_cursor: bool,
}

impl ExecutionPlan {
    /// Compiles a template string into an immutable [`ExecutionPlan`].
    pub fn compile(template: &str) -> Self {
        // Fast path for static strings without brackets, pipelines, or escapes.
        if !template.contains('[') && !template.contains('|') && !template.contains('\\') {
            return Self {
                raw_template: template.to_string(),
                ops: if template.is_empty() {
                    Vec::new()
                } else {
                    vec![PlanOp::Literal(template.to_string())]
                },
                global_transformers: Vec::new(),
                has_directive_steps: false,
                has_cursor: false,
            };
        }

        let segments = transformers::split_pipeline(template);
        if segments.len() > 1
            && segments[1..]
                .iter()
                .all(|tr| transformers::is_valid_transformer(tr))
        {
            let global_transformers = segments[1..].iter().map(|s| s.to_string()).collect();
            let base_expr = system::strip_quotes(segments[0]).unwrap_or(segments[0]);
            let (ops, has_directive_steps, has_cursor) = compile_ops(base_expr);
            return Self {
                raw_template: template.to_string(),
                ops,
                global_transformers,
                has_directive_steps,
                has_cursor,
            };
        }

        let (ops, has_directive_steps, has_cursor) = compile_ops(template);
        Self {
            raw_template: template.to_string(),
            ops,
            global_transformers: Vec::new(),
            has_directive_steps,
            has_cursor,
        }
    }

    /// Evaluates the pre-compiled plan with arguments and returns the resulting [`FinalExpansion`].
    pub fn evaluate(
        &self,
        args: &ArgMap,
        trigger: Option<&str>,
        origin: ExpansionOrigin,
    ) -> FinalExpansion {
        if !self.global_transformers.is_empty() {
            let mut base_text = String::new();
            for op in &self.ops {
                base_text.push_str(&evaluate_text_op(op, args));
            }
            let (mut final_str, _) = apply_transformers(base_text, &self.global_transformers);
            final_str = final_str.replace("\\|", "|");
            if contains_ai_markers(&final_str) {
                return FinalExpansion {
                    steps: Vec::new(),
                    is_calculation: false,
                    ai_transformer_template: Some(final_str),
                };
            }
            return FinalExpansion {
                steps: if final_str.is_empty() {
                    Vec::new()
                } else {
                    vec![ExpansionStep::Text(final_str)]
                },
                is_calculation: false,
                ai_transformer_template: None,
            };
        }

        if origin == ExpansionOrigin::Ai {
            let mut text = String::new();
            for op in &self.ops {
                match op {
                    PlanOp::Cursor => text.push_str("[cursor]"),
                    PlanOp::KeyPress(alias) => text.push_str(&format!("[key({alias})]")),
                    PlanOp::Delay(ms) => text.push_str(&format!("[delay({ms}ms)]")),
                    PlanOp::Mouse(step) => text.push_str(&format_mouse_directive(step)),
                    PlanOp::Image(ExpansionStep::Image(_, path)) => {
                        text.push_str(&format!("[img({path})]"))
                    }
                    PlanOp::InlineRun {
                        raw_cmd,
                        transformers: trs,
                    } => {
                        if trs.is_empty() {
                            text.push_str(&format!("[{raw_cmd}]"));
                        } else {
                            text.push_str(&format!("[{} | {}]", raw_cmd, trs.join(" | ")));
                        }
                    }
                    other => text.push_str(&evaluate_text_op(other, args)),
                }
            }
            return FinalExpansion {
                steps: if text.is_empty() {
                    Vec::new()
                } else {
                    vec![ExpansionStep::Text(text)]
                },
                is_calculation: false,
                ai_transformer_template: None,
            };
        }

        if self.has_directive_steps {
            let _ = system::validate_output(&self.raw_template, trigger);
            let mut steps: Vec<ExpansionStep> = Vec::new();
            let mut current_text = String::new();

            for op in &self.ops {
                match op {
                    PlanOp::Literal(s) => current_text.push_str(s),
                    PlanOp::Cursor => current_text.push_str("[cursor]"),
                    PlanOp::KeyPress(alias) => {
                        flush_text(&mut steps, &mut current_text);
                        steps.push(ExpansionStep::KeyPress(alias.to_lowercase()));
                    }
                    PlanOp::Delay(ms) => {
                        flush_text(&mut steps, &mut current_text);
                        steps.push(ExpansionStep::Delay(*ms));
                    }
                    PlanOp::Mouse(step) | PlanOp::Image(step) => {
                        flush_text(&mut steps, &mut current_text);
                        steps.push(step.clone());
                    }
                    PlanOp::InlineRun {
                        raw_cmd,
                        transformers: trs,
                    } => {
                        flush_text(&mut steps, &mut current_text);
                        if !crate::settings::get_cached_scripts_enabled() {
                            tracing::warn!(
                                "Blocked execution of [exec.*] block because scripts are disabled globally."
                            );
                            steps.push(ExpansionStep::Text(
                                "[Error: Script execution is disabled globally]".to_string(),
                            ));
                        } else {
                            match system::exec::to_script_metadata(raw_cmd) {
                                Ok(metadata) => {
                                    steps.push(ExpansionStep::InlineRun(metadata, trs.clone()))
                                }
                                Err(error) => {
                                    steps.push(ExpansionStep::Text(format_run_error(error)))
                                }
                            }
                        }
                    }
                    other => {
                        let text = evaluate_text_op(other, args);
                        if text.contains('[')
                            && (text.contains("[key(")
                                || text.contains("[delay(")
                                || text.contains("[mouse.")
                                || text.contains("[exec.")
                                || text.contains("[img("))
                        {
                            let sub_steps = system::finalize(&text, None).steps;
                            for s in sub_steps {
                                match s {
                                    ExpansionStep::Text(t) => current_text.push_str(&t),
                                    non_text => {
                                        flush_text(&mut steps, &mut current_text);
                                        steps.push(non_text);
                                    }
                                }
                            }
                        } else {
                            current_text.push_str(&text);
                        }
                    }
                }
            }
            flush_text(&mut steps, &mut current_text);

            if steps.iter().any(|s| match s {
                ExpansionStep::Text(t) => contains_ai_markers(t),
                _ => false,
            }) {
                let full: String = steps
                    .iter()
                    .map(|s| match s {
                        ExpansionStep::Text(t) => t.as_str(),
                        _ => "",
                    })
                    .collect();
                return FinalExpansion {
                    steps: Vec::new(),
                    is_calculation: false,
                    ai_transformer_template: Some(full),
                };
            }

            return FinalExpansion {
                steps,
                is_calculation: false,
                ai_transformer_template: None,
            };
        }

        // Standard text expansion with optional [cursor] positioning
        let mut full_text = String::new();
        let mut first_cursor_char_idx: Option<usize> = None;

        for op in &self.ops {
            match op {
                PlanOp::Cursor => {
                    if first_cursor_char_idx.is_none() {
                        first_cursor_char_idx = Some(full_text.chars().count());
                    }
                }
                PlanOp::Literal(s) => full_text.push_str(s),
                other => {
                    let text = evaluate_text_op(other, args);
                    if text.contains("[cursor]") && first_cursor_char_idx.is_none() {
                        if let Some(pos) = text.find("[cursor]") {
                            let before = &text[..pos];
                            first_cursor_char_idx =
                                Some(full_text.chars().count() + before.chars().count());
                        }
                        let clean = text.replace("[cursor]", "");
                        full_text.push_str(&clean);
                    } else {
                        full_text.push_str(&text);
                    }
                }
            }
        }

        let _ = system::validate_output(&full_text, trigger);

        if contains_ai_markers(&full_text) {
            return FinalExpansion {
                steps: Vec::new(),
                is_calculation: false,
                ai_transformer_template: Some(full_text),
            };
        }

        if let Some(cursor_char_idx) = first_cursor_char_idx {
            let clean_text = full_text.replace("[cursor]", "");
            let total_chars = clean_text.chars().count();
            let left_arrow_count = total_chars.saturating_sub(cursor_char_idx);
            let mut steps = Vec::new();
            if !clean_text.is_empty() {
                steps.push(ExpansionStep::Text(clean_text));
            }
            for _ in 0..left_arrow_count {
                steps.push(ExpansionStep::KeyPress("left".to_string()));
            }
            FinalExpansion {
                steps,
                is_calculation: false,
                ai_transformer_template: None,
            }
        } else {
            let steps = if full_text.is_empty() {
                Vec::new()
            } else {
                vec![ExpansionStep::Text(full_text)]
            };
            FinalExpansion {
                steps,
                is_calculation: false,
                ai_transformer_template: None,
            }
        }
    }

    /// Returns a slice of the compiled operations.
    pub fn ops(&self) -> &[PlanOp] {
        &self.ops
    }

    /// Returns the original raw template string.
    pub fn raw_template(&self) -> &str {
        &self.raw_template
    }
}

fn compile_ops(expr: &str) -> (Vec<PlanOp>, bool, bool) {
    let mut ops = Vec::new();
    let mut ptr = 0;
    let mut has_directive_steps = false;
    let mut has_cursor = false;

    while let Some(tag) = find_next_tag(expr, ptr) {
        let literal_segment = &expr[ptr..tag.start];
        let unescaped = unescape_literal_segment(literal_segment);
        if !unescaped.is_empty() {
            push_literal(&mut ops, unescaped);
        }

        let inner = tag_inner(expr, tag);
        if inner == "cursor" {
            has_cursor = true;
            ops.push(PlanOp::Cursor);
        } else if let Some(alias) = parse_key_directive(inner) {
            has_directive_steps = true;
            ops.push(PlanOp::KeyPress(alias.to_lowercase()));
        } else if let Some(ms) = parse_delay_directive(inner) {
            has_directive_steps = true;
            ops.push(PlanOp::Delay(ms));
        } else if let Some(step) = parse_mouse_directive(inner) {
            has_directive_steps = true;
            ops.push(PlanOp::Mouse(step));
        } else if let Some(step) = img::parse_img_directive(inner) {
            has_directive_steps = true;
            ops.push(PlanOp::Image(step));
        } else if inner.starts_with("exec.") {
            has_directive_steps = true;
            let pipeline = transformers::split_pipeline(inner);
            let base = pipeline[0];
            let trs = pipeline[1..].iter().map(|s| s.to_string()).collect();
            ops.push(PlanOp::InlineRun {
                raw_cmd: base.to_string(),
                transformers: trs,
            });
        } else if inner.contains('[') {
            ops.push(PlanOp::Dynamic(format!("[{inner}]")));
        } else if inner.starts_with("use(") && inner.ends_with(')') {
            let pipeline = transformers::split_pipeline(inner);
            let base = pipeline[0];
            let trs = pipeline[1..].iter().map(|s| s.to_string()).collect();
            let snip = parse_use_key(base).unwrap_or_else(|| base.to_string());
            ops.push(PlanOp::Use {
                snippet_name: snip,
                transformers: trs,
            });
        } else {
            let pipeline = transformers::split_pipeline(inner);
            let base_expr = pipeline[0];
            let transformers: Vec<String> = pipeline[1..].iter().map(|s| s.to_string()).collect();
            let (key, default_value) = split_key_default(base_expr);
            let key_unquoted = system::strip_quotes(key).unwrap_or(key);
            let default_value = default_value.map(|d| d.to_string());

            if let Some(unquoted) = system::strip_quotes(key)
                && !transformers.is_empty()
            {
                ops.push(PlanOp::QuotedLiteral {
                    value: unquoted.to_string(),
                    transformers,
                });
            } else if system::is_reserved(key_unquoted)
                || split_system_tag(key_unquoted)
                    .is_some_and(|(r, m)| validate_system_tag(r, m).is_ok())
            {
                ops.push(PlanOp::SystemVar {
                    key: key_unquoted.to_string(),
                    transformers,
                });
            } else if let Ok(index) = key_unquoted.parse::<usize>() {
                ops.push(PlanOp::PositionalArg {
                    index,
                    default_value,
                    transformers,
                });
            } else if !key_unquoted.contains('[')
                && !key_unquoted.contains(']')
                && !system::is_reserved(key_unquoted)
            {
                ops.push(PlanOp::NamedArg {
                    name: key_unquoted.to_string(),
                    default_value,
                    transformers,
                });
            } else {
                push_literal(&mut ops, format!("[{inner}]"));
            }
        }

        ptr = tag.end + 1;
    }

    let trailing = &expr[ptr..];
    let unescaped_trailing = unescape_literal_segment(trailing);
    if !unescaped_trailing.is_empty() {
        push_literal(&mut ops, unescaped_trailing);
    }

    (ops, has_directive_steps, has_cursor)
}

fn push_literal(ops: &mut Vec<PlanOp>, text: String) {
    if let Some(PlanOp::Literal(existing)) = ops.last_mut() {
        existing.push_str(&text);
    } else {
        ops.push(PlanOp::Literal(text));
    }
}

fn unescape_literal_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut ptr = 0;
    let mut output = String::with_capacity(segment.len());

    while ptr < bytes.len() {
        if bytes[ptr] == b'\\' && ptr + 1 < bytes.len() {
            let next = bytes[ptr + 1];
            if next == TAG_OPEN
                || next == TAG_CLOSE
                || next == b'\\'
                || next == b'\''
                || next == b'"'
            {
                if segment[ptr..].starts_with(r#"\[cursor\]"#) {
                    output.push_str("[cursor]");
                    ptr += r#"\[cursor\]"#.len();
                    continue;
                }
                output.push(next as char);
                ptr += 2;
                continue;
            }
        }

        let Some(c) = segment[ptr..].chars().next() else {
            break;
        };
        output.push(c);
        ptr += c.len_utf8();
    }
    output
}

fn evaluate_text_op(op: &PlanOp, args: &ArgMap) -> String {
    match op {
        PlanOp::Literal(s) => s.clone(),
        PlanOp::PositionalArg {
            index,
            default_value,
            transformers: trs,
        } => {
            let raw_val = if let Some(val) = args.positional.get(*index) {
                val.clone()
            } else if let Some(val) = args.named.get(&index.to_string()) {
                val.clone()
            } else if let Some(def) = default_value
                && has_valid_default_value(Some(def))
            {
                let unquoted = system::strip_quotes(def).unwrap_or(def);
                if unquoted.contains('[') {
                    interpolate(unquoted, args)
                } else {
                    unquoted.to_string()
                }
            } else {
                return format_raw_positional_tag(*index, default_value.as_deref(), trs);
            };
            let (transformed, valid) = apply_transformers(raw_val, trs);
            if valid {
                transformed
            } else {
                format_raw_positional_tag(*index, default_value.as_deref(), trs)
            }
        }
        PlanOp::NamedArg {
            name,
            default_value,
            transformers: trs,
        } => {
            let raw_val = if let Some(val) = args.named.get(name) {
                Some(val.clone())
            } else if let Some(def) = default_value
                && has_valid_default_value(Some(def))
            {
                let unquoted = system::strip_quotes(def).unwrap_or(def);
                let resolved = if unquoted.contains('[') {
                    interpolate(unquoted, args)
                } else {
                    unquoted.to_string()
                };
                Some(resolved)
            } else {
                None
            };

            if let Some(val) = raw_val {
                let (transformed, valid) = apply_transformers(val, trs);
                if valid {
                    transformed
                } else {
                    format_raw_named_tag(name, default_value.as_deref(), trs)
                }
            } else {
                format_raw_named_tag(name, default_value.as_deref(), trs)
            }
        }
        PlanOp::SystemVar {
            key,
            transformers: trs,
        } => {
            let resolved = if system::is_deferred(key) {
                Some(format!("\x03\x1Fsys:{key}\x04"))
            } else {
                system::resolve(key)
            };

            if let Some(val) = resolved {
                let (transformed, valid) = apply_transformers(val, trs);
                if valid {
                    transformed
                } else {
                    format_raw_system_tag(key, trs)
                }
            } else {
                format_raw_system_tag(key, trs)
            }
        }
        PlanOp::QuotedLiteral {
            value,
            transformers: trs,
        } => {
            let (transformed, _) = apply_transformers(value.clone(), trs);
            transformed
        }
        PlanOp::Use {
            snippet_name,
            transformers: trs,
        } => {
            let base = resolve_use_snippet(snippet_name, args, 0);
            let (transformed, _) = apply_transformers(base, trs);
            transformed
        }
        PlanOp::Dynamic(expr) => interpolate(expr, args),
        PlanOp::Cursor => "[cursor]".to_string(),
        PlanOp::KeyPress(alias) => format!("[key({alias})]"),
        PlanOp::Delay(ms) => format!("[delay({ms}ms)]"),
        PlanOp::Mouse(step) => format_mouse_directive(step),
        PlanOp::Image(ExpansionStep::Image(_, path)) => format!("[img({path})]"),
        PlanOp::Image(_) => String::new(),
        PlanOp::InlineRun {
            raw_cmd,
            transformers: trs,
        } => {
            if trs.is_empty() {
                format!("[{raw_cmd}]")
            } else {
                format!("[{} | {}]", raw_cmd, trs.join(" | "))
            }
        }
    }
}

fn apply_transformers(mut text: String, transformers: &[String]) -> (String, bool) {
    for tr in transformers {
        if transformers::is_ai_transformer(tr) {
            let prompt = transformers::extract_ai_prompt(tr).to_string();
            text = format!("\x03{text}\x1F{prompt}\x04");
        } else if text.starts_with("\x03\x1Fsys:") && text.ends_with('\x04') {
            let inner_sys = &text[..text.len() - 1];
            text = format!("{} | {}\x04", inner_sys, tr);
        } else if let Some(transformed) = transformers::apply(tr, &text) {
            text = transformed;
        } else {
            return (text, false);
        }
    }
    (text, true)
}

fn has_valid_default_value(default_value: Option<&str>) -> bool {
    if let Some(dv) = default_value {
        let unquoted = system::strip_quotes(dv).unwrap_or(dv);
        !unquoted.trim().is_empty()
    } else {
        false
    }
}

fn format_raw_positional_tag(
    index: usize,
    default_value: Option<&str>,
    transformers: &[String],
) -> String {
    let mut inner = if let Some(def) = default_value {
        format!("{index}={def}")
    } else {
        index.to_string()
    };
    if !transformers.is_empty() {
        inner = format!("{inner} | {}", transformers.join(" | "));
    }
    format!("[{inner}]")
}

fn format_raw_named_tag(
    name: &str,
    default_value: Option<&str>,
    transformers: &[String],
) -> String {
    let mut inner = if let Some(def) = default_value {
        format!("{name}={def}")
    } else {
        name.to_string()
    };
    if !transformers.is_empty() {
        inner = format!("{inner} | {}", transformers.join(" | "));
    }
    format!("[{inner}]")
}

fn format_raw_system_tag(key: &str, transformers: &[String]) -> String {
    if transformers.is_empty() {
        format!("[{key}]")
    } else {
        format!("[{key} | {}]", transformers.join(" | "))
    }
}

fn parse_use_key(key: &str) -> Option<String> {
    let inner = key.strip_prefix("use(")?.strip_suffix(')')?;
    let unquoted = system::strip_quotes(inner.trim())
        .map(|s| s.to_string())
        .unwrap_or_else(|| inner.trim().to_string());
    Some(unquoted)
}

fn resolve_use_snippet(trigger_name: &str, args: &ArgMap, depth: usize) -> String {
    if depth >= 5 {
        return "[Error: Max recursion depth reached]".to_string();
    }

    let conn = match crate::db::get_conn() {
        Ok(c) => c,
        Err(e) => return format!("[Error: Database pool error: {}]", e),
    };

    let action = match crate::db::crud::triggers::get_action_by_trigger(&conn, trigger_name) {
        Ok(Some(act)) => act,
        Ok(None) => return format!("[Error: Snippet '{}' does not exist]", trigger_name),
        Err(e) => return format!("[Error: Database query error: {}]", e),
    };

    if !action.is_text() {
        return format!("[Error: Cannot invoke non-text snippet '{}']", trigger_name);
    }

    interpolate(&action.output, args)
}

fn format_mouse_directive(step: &ExpansionStep) -> String {
    match step {
        ExpansionStep::MouseClick => "[mouse.click]".to_string(),
        ExpansionStep::MouseRClick => "[mouse.rclick]".to_string(),
        ExpansionStep::MouseMClick => "[mouse.mclick]".to_string(),
        ExpansionStep::MouseHold => "[mouse.hold]".to_string(),
        ExpansionStep::MouseRelease => "[mouse.release]".to_string(),
        ExpansionStep::MouseMove(x, y) => format!("[mouse.move({x},{y})]"),
        ExpansionStep::MouseScroll(d) => format!("[mouse.scroll({d})]"),
        _ => String::new(),
    }
}

fn flush_text(steps: &mut Vec<ExpansionStep>, buf: &mut String) {
    if !buf.is_empty() {
        steps.push(ExpansionStep::Text(std::mem::take(buf)));
    }
}

fn format_run_error(error: String) -> String {
    if error.starts_with("[Error:") {
        error
    } else {
        format!("[Error: {error}]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::variables::types::{ArgMap, ExpansionOrigin, ExpansionStep};
    use crate::engine::variables::{finalize, interpolate};

    #[test]
    fn test_compile_static_literal() {
        let plan = ExecutionPlan::compile("Hello world!");
        let expansion = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
        assert_eq!(
            expansion.steps,
            vec![ExpansionStep::Text("Hello world!".to_string())]
        );
        assert!(!expansion.is_calculation);
        assert!(expansion.ai_transformer_template.is_none());
    }

    #[test]
    fn test_compile_and_evaluate_positional_args() {
        let mut args = ArgMap::default();
        args.positional.push("ereinaimer".to_string());
        args.positional.push("taurine".to_string());

        let plan = ExecutionPlan::compile("https://github.com/[0=org]/[1=repo]");
        let expansion = plan.evaluate(&args, None, ExpansionOrigin::User);
        assert_eq!(
            expansion.steps,
            vec![ExpansionStep::Text(
                "https://github.com/ereinaimer/taurine".to_string()
            )]
        );
    }

    #[test]
    fn test_compile_and_evaluate_named_args() {
        let mut args = ArgMap::default();
        args.named.insert("name".to_string(), "john".to_string());

        let plan = ExecutionPlan::compile("Hello [name=default | upper]!");
        let expansion = plan.evaluate(&args, None, ExpansionOrigin::User);
        assert_eq!(
            expansion.steps,
            vec![ExpansionStep::Text("Hello JOHN!".to_string())]
        );
    }

    #[test]
    fn test_compile_and_evaluate_cursor_positioning() {
        let plan = ExecutionPlan::compile("hello [cursor] world");
        let expansion = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);
        assert_eq!(
            expansion.steps,
            vec![
                ExpansionStep::Text("hello  world".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
                ExpansionStep::KeyPress("left".to_string()),
            ]
        );
    }

    #[test]
    fn test_compile_and_evaluate_directives_sequence() {
        let mut args = ArgMap::default();
        args.positional.push("cli".to_string());
        args.positional.push("add custom pipelines".to_string());

        let tpl = "git commit -m \"feat([0=core]): [1=update codebase | sentence]\"[key(enter)][delay(500ms)]git push origin main[key(enter)]";
        let plan = ExecutionPlan::compile(tpl);
        let expansion = plan.evaluate(&args, None, ExpansionOrigin::User);

        assert_eq!(
            expansion.steps,
            vec![
                ExpansionStep::Text(
                    "git commit -m \"feat(cli): Add custom pipelines\"".to_string()
                ),
                ExpansionStep::KeyPress("enter".to_string()),
                ExpansionStep::Delay(500),
                ExpansionStep::Text("git push origin main".to_string()),
                ExpansionStep::KeyPress("enter".to_string()),
            ]
        );
    }

    #[test]
    fn test_compile_cursor_conflict_with_key_directives() {
        let tpl = "echo [cursor][key(enter)]";
        let plan = ExecutionPlan::compile(tpl);
        let expansion = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);

        assert_eq!(
            expansion.steps,
            vec![
                ExpansionStep::Text("echo [cursor]".to_string()),
                ExpansionStep::KeyPress("enter".to_string()),
            ]
        );
    }

    #[test]
    fn test_compile_ai_transformer() {
        crate::engine::variables::system::clip::set_mock_clip(Some("Article text".to_string()));
        let tpl = "Summary: [clip | ai(summarize this in 3 bullets) | trim]";
        let plan = ExecutionPlan::compile(tpl);
        let expansion = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);

        assert!(expansion.steps.is_empty());
        assert!(expansion.ai_transformer_template.is_some());
        assert!(
            expansion
                .ai_transformer_template
                .as_ref()
                .unwrap()
                .contains("\x03Article text\x1Fsummarize this in 3 bullets\x04")
        );
        crate::engine::variables::system::clip::set_mock_clip(None);
    }

    #[test]
    fn test_compile_global_pipeline() {
        let tpl = "\"hello world \" | title | repeat(2)";
        let plan = ExecutionPlan::compile(tpl);
        let expansion = plan.evaluate(&ArgMap::default(), None, ExpansionOrigin::User);

        assert_eq!(
            expansion.steps,
            vec![ExpansionStep::Text("Hello World Hello World ".to_string())]
        );
    }

    #[test]
    fn test_parity_with_interpolate_and_finalize() {
        let test_cases = vec![
            ("Plain text", ArgMap::default()),
            ("Hello [0=world]!", {
                let mut m = ArgMap::default();
                m.positional.push("Alice".to_string());
                m
            }),
            ("User: [user=guest | upper]", {
                let mut m = ArgMap::default();
                m.named.insert("user".to_string(), "bob".to_string());
                m
            }),
            ("Escaped: \\[cursor\\] and \\\\ path", ArgMap::default()),
            ("| [0=ID] | [1=Name | title] |[key(enter)]", {
                let mut m = ArgMap::default();
                m.positional.push("42".to_string());
                m.positional.push("jane doe".to_string());
                m
            }),
        ];

        for (tpl, args) in test_cases {
            let interp = interpolate(tpl, &args);
            let expected = finalize(&interp, None);

            let plan = ExecutionPlan::compile(tpl);
            let actual = plan.evaluate(&args, None, ExpansionOrigin::User);

            assert_eq!(actual, expected, "Mismatch for template: {}", tpl);
        }
    }
}
