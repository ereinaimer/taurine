use crate::db::crud::TriggerAction;
use crate::engine::shell::{ScriptMetadata, compress, decompress};
use crate::engine::source::{AdaptiveSource, MemorySource, SnippetSource};
use crate::engine::variables::{
    ArgMap, ExpansionStep, FinalExpansion, finalize, interpolate, parse_tokens, tokenize,
};
use crate::keys::{Hotkey, LogicalKey, hotkey_matches, parse_hotkey};

use std::sync::{Arc, OnceLock, RwLock};

use arc_swap::ArcSwap;

/// Lazily resolves the active window label.
/// The OS fetch happens at most once — only when a matching entry
/// with app filters is found.
pub(crate) struct WindowResolver {
    cached: OnceLock<Option<String>>,
}

impl WindowResolver {
    pub fn lazy() -> Self {
        Self {
            cached: OnceLock::new(),
        }
    }

    pub fn resolve(&self, fetcher: impl FnOnce() -> Option<String>) -> Option<&str> {
        self.cached.get_or_init(fetcher).as_deref()
    }

    #[allow(dead_code)]
    pub fn get_cached(&self) -> Option<&str> {
        self.cached.get().and_then(|o| o.as_deref())
    }
}

pub struct ExpansionCatalog {
    source: Arc<dyn SnippetSource>,
    triggers: RwLock<Vec<Arc<str>>>,
    history_triggers: RwLock<Vec<Arc<str>>>,
}

pub struct HotkeyCatalog {
    snapshot: ArcSwap<CatalogSnapshot>,
}

#[derive(Default)]
struct CatalogSnapshot {
    parsed_actions: std::collections::HashMap<LogicalKey, Vec<ParsedHotkeyAction>>,
}

#[derive(Clone)]
struct ParsedHotkeyAction {
    configured_trigger: String,
    hotkey: Hotkey,
    action: Arc<TriggerAction>,
}

impl Default for HotkeyCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyCatalog {
    pub fn new() -> Self {
        Self {
            snapshot: ArcSwap::new(Arc::new(CatalogSnapshot::default())),
        }
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, TriggerAction)>) {
        let mut snapshot = CatalogSnapshot::default();

        for (trigger, action) in actions {
            if let Ok(hotkey) = parse_hotkey(&trigger) {
                let entry = ParsedHotkeyAction {
                    configured_trigger: trigger.clone(),
                    hotkey,
                    action: Arc::new(action),
                };

                // Only bucket into parsed_actions so that multiple triggers
                // sharing the same hotkey but different app filters all survive.
                // The old canonical_string fast-path HashMap silently overwrote
                // earlier entries whenever two triggers parsed to the same hotkey.
                snapshot
                    .parsed_actions
                    .entry(hotkey.logical_key())
                    .or_default()
                    .push(entry);
            }
        }

        self.snapshot.store(Arc::new(snapshot));
    }

    pub fn has_entry_for(&self, key: LogicalKey) -> bool {
        self.snapshot.load().parsed_actions.contains_key(&key)
    }

    pub fn get_action(&self, trigger: &str) -> Option<TriggerAction> {
        let hotkey = parse_hotkey(trigger).ok()?;
        let base_key = hotkey.logical_key();
        let guard = self.snapshot.load();
        guard.parsed_actions.get(&base_key).and_then(|bucket| {
            bucket
                .iter()
                .find(|entry| entry.configured_trigger == trigger || entry.hotkey == hotkey)
                .map(|entry| entry.action.as_ref().clone())
        })
    }

    pub fn match_action(
        &self,
        pressed: Hotkey,
        active_window: Option<&str>,
    ) -> Option<(String, TriggerAction)> {
        let base_key = pressed.logical_key();
        let guard = self.snapshot.load();
        let bucket = guard.parsed_actions.get(&base_key)?;
        let pressed_canonical = pressed.canonical_string();

        // First pass: prefer an entry whose hotkey canonically matches the
        // pressed combo exactly (e.g. `ralt+m` wins over `alt+m` when the
        // right Alt key is pressed).
        if let Some(entry) = bucket.iter().find(|e| {
            e.hotkey.canonical_string() == pressed_canonical
                && is_app_allowed(&e.action, active_window)
        }) {
            return Some((
                entry.configured_trigger.clone(),
                entry.action.as_ref().clone(),
            ));
        }

        // Second pass: accept any entry whose hotkey overlaps the pressed combo
        // (handles generic modifiers like `alt+m` matching `lalt+m` presses).
        bucket
            .iter()
            .find(|e| hotkey_matches(e.hotkey, pressed) && is_app_allowed(&e.action, active_window))
            .map(|e| (e.configured_trigger.clone(), e.action.as_ref().clone()))
    }

    pub fn match_action_lazy(
        &self,
        pressed: Hotkey,
        fetch_window: impl FnOnce() -> Option<String>,
    ) -> Option<(String, TriggerAction)> {
        let base_key = pressed.logical_key();
        let guard = self.snapshot.load();
        let bucket = guard.parsed_actions.get(&base_key)?;
        let pressed_canonical = pressed.canonical_string();
        let window = WindowResolver::lazy();
        let mut fetch_window = Some(fetch_window);

        // Pass 1: exact canonical match — resolved iteratively
        for entry in bucket.iter() {
            if entry.hotkey.canonical_string() != pressed_canonical {
                continue;
            }
            if !entry_has_app_filters(&entry.action) {
                return Some((
                    entry.configured_trigger.clone(),
                    entry.action.as_ref().clone(),
                ));
            }
            let Some(w) = window.resolve(|| fetch_window.take().unwrap()()) else {
                continue;
            };
            if is_app_allowed(&entry.action, Some(w)) {
                return Some((
                    entry.configured_trigger.clone(),
                    entry.action.as_ref().clone(),
                ));
            }
        }

        // Pass 2: hotkey_matches fallback — resolved iteratively
        for entry in bucket.iter() {
            if !hotkey_matches(entry.hotkey, pressed) {
                continue;
            }
            if !entry_has_app_filters(&entry.action) {
                return Some((
                    entry.configured_trigger.clone(),
                    entry.action.as_ref().clone(),
                ));
            }
            let Some(w) = window.resolve(|| fetch_window.take().unwrap()()) else {
                continue;
            };
            if is_app_allowed(&entry.action, Some(w)) {
                return Some((
                    entry.configured_trigger.clone(),
                    entry.action.as_ref().clone(),
                ));
            }
        }

        None
    }
}

pub struct RegexCatalog {
    snapshot: RwLock<RegexCatalogSnapshot>,
}

#[derive(Default)]
struct RegexCatalogSnapshot {
    entries: Vec<ParsedRegexAction>,
}

#[derive(Clone)]
struct ParsedRegexAction {
    pattern: String,
    regex: OnceLock<Result<regex::Regex, ()>>,
    action: TriggerAction,
}

impl RegexCatalog {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(RegexCatalogSnapshot::default()),
        }
    }

    pub fn is_empty(&self) -> bool {
        if let Ok(guard) = self.snapshot.read() {
            return guard.entries.is_empty();
        }
        true
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, TriggerAction)>) {
        let mut entries = Vec::new();
        for (pattern, action) in actions {
            entries.push(ParsedRegexAction {
                pattern,
                regex: OnceLock::new(),
                action,
            });
        }
        if let Ok(mut guard) = self.snapshot.write() {
            guard.entries = entries;
        }
    }

    pub fn match_action(
        &self,
        buffer_string: &str,
        active_window: Option<&str>,
    ) -> Option<(String, TriggerAction, Vec<String>)> {
        let guard = self.snapshot.read().ok()?;
        for entry in &guard.entries {
            let re = match entry
                .regex
                .get_or_init(|| regex::Regex::new(&entry.pattern).map_err(|_| ()))
            {
                Ok(re) => re,
                Err(_) => continue,
            };
            if is_app_allowed(&entry.action, active_window)
                && let Some(m) = re.find_iter(buffer_string).last()
                && m.end() == buffer_string.len()
                && !m.as_str().is_empty()
            {
                let matched_str = m.as_str();
                let mut captures_list = Vec::new();
                if let Some(caps) = re.captures(matched_str) {
                    for i in 1..caps.len() {
                        let val = caps
                            .get(i)
                            .map(|c| c.as_str().to_string())
                            .unwrap_or_default();
                        captures_list.push(val);
                    }
                }
                return Some((matched_str.to_string(), entry.action.clone(), captures_list));
            }
        }
        None
    }
}

impl Default for RegexCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpansionCatalog {
    pub fn new() -> Self {
        let memory = Arc::new(MemorySource::new());
        let adaptive = Arc::new(AdaptiveSource::new(memory));
        Self {
            source: adaptive,
            triggers: RwLock::new(Vec::new()),
            history_triggers: RwLock::new(Vec::new()),
        }
    }

    pub fn with_source(source: Arc<dyn SnippetSource>) -> Self {
        Self {
            source,
            triggers: RwLock::new(Vec::new()),
            history_triggers: RwLock::new(Vec::new()),
        }
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, TriggerAction)>) {
        let actions: Vec<_> = actions.into_iter().collect();
        let mut triggers: Vec<Arc<str>> = actions
            .iter()
            .map(|(trigger, _)| Arc::from(trigger.as_str()))
            .collect();
        sort_completion_triggers(&mut triggers);

        self.source.load_actions(actions);

        if let Ok(mut guard) = self.triggers.write() {
            *guard = triggers;
        }

        self.load_history_triggers(Vec::<String>::new());
    }

    pub fn matching_triggers(&self, prefix: &str) -> Vec<String> {
        if prefix.is_empty() {
            return Vec::new();
        }
        let normalized_prefix = prefix.to_lowercase();
        self.triggers
            .read()
            .map(|guard| {
                guard
                    .iter()
                    .filter(|trigger| trigger.to_lowercase().starts_with(&normalized_prefix))
                    .map(|arc| arc.as_ref().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn load_history_triggers(&self, triggers: impl IntoIterator<Item = String>) {
        let known = self
            .triggers
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();

        let mut seen = std::collections::HashSet::new();
        let mut ordered_history: Vec<Arc<str>> = triggers
            .into_iter()
            .filter(|t| known.iter().any(|k| k.as_ref() == t.as_str()) && seen.insert(t.clone()))
            .map(|t| Arc::from(t.as_str()))
            .collect();
        for k in known {
            if seen.insert(k.to_string()) {
                ordered_history.push(k);
            }
        }

        if let Ok(mut guard) = self.history_triggers.write() {
            *guard = ordered_history;
        }
    }

    pub fn matching_history_triggers(&self, prefix: &str) -> Vec<String> {
        let normalized_prefix = prefix.to_lowercase();
        self.history_triggers
            .read()
            .map(|guard| {
                guard
                    .iter()
                    .filter(|trigger| {
                        normalized_prefix.is_empty()
                            || trigger.to_lowercase().starts_with(&normalized_prefix)
                    })
                    .map(|arc| arc.as_ref().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn promote_history_trigger(&self, trigger: &str) {
        let known = self
            .triggers
            .read()
            .map(|guard| guard.iter().any(|k| k.as_ref() == trigger))
            .unwrap_or(false);

        if !known {
            return;
        }

        if let Ok(mut guard) = self.history_triggers.write() {
            if let Some(index) = guard.iter().position(|t| t.as_ref() == trigger) {
                let entry = guard.remove(index);
                guard.insert(0, entry);
            } else {
                guard.insert(0, Arc::from(trigger));
            }
        }
    }

    fn get_raw_action(&self, keyword: &str, active_window: Option<&str>) -> Option<TriggerAction> {
        if let Some(action) = self.source.get_action(keyword)
            && is_app_allowed(&action, active_window)
        {
            return Some(action);
        }

        let lower_keyword = keyword.to_lowercase();
        if lower_keyword != keyword
            && let Some(action) = self.source.get_action(&lower_keyword)
            && is_app_allowed(&action, active_window)
        {
            return Some(action);
        }

        None
    }

    fn expand_action(
        &self,
        action: TriggerAction,
        args: &ArgMap,
        matched_keyword: &str,
    ) -> Option<FinalExpansion> {
        expand_trigger_action_with_args(action, args, matched_keyword)
    }

    fn fetch_exact_match(
        &self,
        keyword: &str,
        active_window: Option<&str>,
    ) -> Option<FinalExpansion> {
        let action = self.get_raw_action(keyword, active_window)?;
        self.expand_action(action, &ArgMap::default(), keyword)
    }

    fn fetch_hybrid_arguments(
        &self,
        keyword: &str,
        active_window: Option<&str>,
    ) -> Option<FinalExpansion> {
        let tokens = tokenize(keyword, ':');
        if tokens.len() <= 1 {
            return None;
        }

        let base = tokens.first()?.trim();
        let action = self.get_raw_action(base, active_window)?;
        let args = parse_tokens(&tokens[1..]);
        self.expand_action(action, &args, base)
    }

    fn fetch_math_fallback(&self, keyword: &str, instant_expand: bool) -> Option<FinalExpansion> {
        if instant_expand {
            return None;
        }
        let math_result = crate::engine::math::evaluate(keyword)?;
        let mut expansion = FinalExpansion::text(math_result);
        expansion.is_calculation = true;
        Some(expansion)
    }

    pub fn fetch_expansion(
        &self,
        keyword: &str,
        instant_expand: bool,
        active_window: Option<&str>,
    ) -> Option<FinalExpansion> {
        self.fetch_exact_match(keyword, active_window)
            .or_else(|| self.fetch_hybrid_arguments(keyword, active_window))
            .or_else(|| self.fetch_math_fallback(keyword, instant_expand))
    }
}

impl Default for ExpansionCatalog {
    fn default() -> Self {
        Self::new()
    }
}

fn sort_completion_triggers(triggers: &mut Vec<Arc<str>>) {
    triggers.sort_by(|left, right| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
    });
    triggers.dedup();
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ActiveWindowInfo {
    pub title: Option<String>,
    pub class: Option<String>,
    pub exec_name: Option<String>,
    pub exec_path: Option<String>,
}

fn match_rules(filter_list: &str, info: &ActiveWindowInfo) -> bool {
    let rules: Vec<&str> = filter_list
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if rules.is_empty() {
        return false;
    }

    rules.iter().any(|rule| {
        let (prefix, value) = if let Some(pos) = rule.find(':') {
            let (p, v) = rule.split_at(pos);
            let p_lower = p.to_lowercase();
            if matches!(p_lower.as_str(), "exe" | "class" | "title") {
                (p_lower, &v[1..])
            } else {
                ("exe".to_string(), *rule)
            }
        } else {
            ("exe".to_string(), *rule)
        };

        let val_lower = value.to_lowercase();

        match prefix.as_str() {
            "exe" => {
                if let Some(path) = &info.exec_path
                    && (value.contains('/') || value.contains('\\'))
                {
                    path.replace('/', "\\").to_lowercase() == val_lower.replace('/', "\\")
                } else if let Some(name) = &info.exec_name {
                    let clean_name = name.to_lowercase();
                    let clean_name = clean_name.strip_suffix(".exe").unwrap_or(&clean_name);
                    let clean_val = val_lower.strip_suffix(".exe").unwrap_or(&val_lower);
                    clean_name == clean_val
                } else {
                    false
                }
            }
            "class" => {
                if let Some(class) = &info.class {
                    class.to_lowercase() == val_lower
                } else {
                    false
                }
            }
            "title" => {
                if let Some(title) = &info.title {
                    title.to_lowercase().contains(&val_lower)
                } else {
                    false
                }
            }
            _ => {
                if let Some(name) = &info.exec_name {
                    let clean_name = name.to_lowercase();
                    let clean_name = clean_name.strip_suffix(".exe").unwrap_or(&clean_name);
                    let clean_val = val_lower.strip_suffix(".exe").unwrap_or(&val_lower);
                    clean_name == clean_val
                } else {
                    false
                }
            }
        }
    })
}

fn entry_has_app_filters(action: &TriggerAction) -> bool {
    action
        .only_apps
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty())
        || action
            .except_apps
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
}

pub(crate) fn is_app_allowed(action: &TriggerAction, active_window: Option<&str>) -> bool {
    let has_only = action
        .only_apps
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty());
    let has_except = action
        .except_apps
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty());

    if !has_only && !has_except {
        return true;
    }

    let info = match active_window {
        Some(s) => {
            if s.starts_with('{') {
                serde_json::from_str::<ActiveWindowInfo>(s).unwrap_or_else(|_| ActiveWindowInfo {
                    exec_name: Some(s.to_string()),
                    ..Default::default()
                })
            } else {
                ActiveWindowInfo {
                    exec_name: Some(s.to_string()),
                    ..Default::default()
                }
            }
        }
        None => {
            return false;
        }
    };

    if has_only {
        let allowed = action.only_apps.as_ref().unwrap();
        if !match_rules(allowed, &info) {
            return false;
        }
    }

    if has_except {
        let denied = action.except_apps.as_ref().unwrap();
        if match_rules(denied, &info) {
            return false;
        }
    }

    true
}

pub(crate) fn expand_trigger_action(
    action: TriggerAction,
    matched_keyword: &str,
) -> Option<FinalExpansion> {
    expand_trigger_action_with_args(action, &ArgMap::default(), matched_keyword)
}

fn apply_auto_case(output: &str, typed_trigger: &str) -> String {
    let is_all_uppercase = typed_trigger
        .chars()
        .all(|c| !c.is_alphabetic() || c.is_uppercase());
    if is_all_uppercase {
        output.to_uppercase()
    } else {
        let starts_uppercase = typed_trigger
            .chars()
            .find(|c| c.is_alphabetic())
            .is_some_and(|c| c.is_uppercase());
        if starts_uppercase {
            let mut chars = output.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        } else {
            output.to_string()
        }
    }
}

pub(crate) fn expand_trigger_action_with_args(
    action: TriggerAction,
    args: &ArgMap,
    matched_keyword: &str,
) -> Option<FinalExpansion> {
    if action.action_type == "script" {
        return interpolate_script_action(action, args);
    }

    let interpolated = interpolate(&action.output, args);

    if crate::engine::variables::contains_ai_markers(&interpolated) {
        // Template contains | ai(...) transformer(s) — hand off to daemon for async resolution.
        return Some(FinalExpansion {
            steps: vec![],
            is_calculation: false,
            ai_transformer_template: Some(interpolated),
        });
    }

    let mut final_exp = finalize(&interpolated, Some(matched_keyword));
    if action.auto_case {
        for step in &mut final_exp.steps {
            if let ExpansionStep::Text(text) = step {
                *text = apply_auto_case(text, matched_keyword);
            }
        }
    }
    Some(final_exp)
}

fn interpolate_script_action(action: TriggerAction, args: &ArgMap) -> Option<FinalExpansion> {
    let compressed = action.script_binary?;

    let decompressed = decompress(&compressed).unwrap_or_default();
    let interpolated = interpolate(&decompressed, args);
    let recompressed = compress(&interpolated).unwrap_or(compressed);

    let md = ScriptMetadata {
        interpreter: action.interpreter.unwrap(),
        behavior: action.behavior.unwrap(),
        compressed_content: recompressed,
    };

    if !crate::settings::get_cached_scripts_enabled() {
        tracing::warn!(
            "Blocked execution of Script trigger because scripts are disabled globally."
        );
        Some(FinalExpansion {
            steps: vec![ExpansionStep::Text(
                "[Error: Script execution is disabled globally]".to_string(),
            )],
            is_calculation: false,
            ai_transformer_template: None,
        })
    } else {
        Some(FinalExpansion {
            steps: vec![ExpansionStep::Script(md)],
            is_calculation: false,
            ai_transformer_template: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, compress, decompress};
    use crate::engine::source::MemorySource;
    use std::sync::Arc;

    #[test]
    fn exact_match_precedence_beats_hybrid_argument_parsing() {
        let memory = Arc::new(MemorySource::new());
        let catalog = ExpansionCatalog::with_source(memory.clone());

        memory.load_actions(vec![
            ("hi".to_string(), TriggerAction::text("base [0] ([mood])")),
            (
                "hi:erin".to_string(),
                TriggerAction::text("exact trigger wins"),
            ),
        ]);

        let expansion = catalog.fetch_expansion("hi:erin", false, None).unwrap();
        assert_eq!(
            expansion.steps[0],
            ExpansionStep::Text("exact trigger wins".to_string())
        );
        assert!(!expansion.is_calculation);
    }

    #[test]
    fn raw_action_lookup_uses_case_fallback_after_exact_match_miss() {
        let memory = Arc::new(MemorySource::new());
        let catalog = ExpansionCatalog::with_source(memory.clone());

        memory.load_actions(vec![
            ("gm".to_string(), TriggerAction::text("lowercase")),
            ("GM".to_string(), TriggerAction::text("UPPERCASE")),
            (
                "only_low".to_string(),
                TriggerAction::text("only lowercase"),
            ),
        ]);

        assert_eq!(
            catalog.fetch_expansion("gm", false, None).unwrap().steps[0],
            ExpansionStep::Text("lowercase".to_string())
        );
        assert_eq!(
            catalog.fetch_expansion("GM", false, None).unwrap().steps[0],
            ExpansionStep::Text("UPPERCASE".to_string())
        );
        assert_eq!(
            catalog.fetch_expansion("Gm", false, None).unwrap().steps[0],
            ExpansionStep::Text("lowercase".to_string())
        );
        assert_eq!(
            catalog
                .fetch_expansion("ONLY_LOW", false, None)
                .unwrap()
                .steps[0],
            ExpansionStep::Text("only lowercase".to_string())
        );
        assert!(catalog.fetch_expansion("unknown", false, None).is_none());
        assert!(catalog.fetch_expansion("UNKNOWN", false, None).is_none());
    }

    #[test]
    fn script_interpolation_with_positional_args_matches_current_behavior() {
        let memory = Arc::new(MemorySource::new());
        let catalog = ExpansionCatalog::with_source(memory.clone());

        let script = "explorer [0=C:\\Temp]";
        let compressed = compress(script).unwrap();

        let action = TriggerAction {
            output: String::new(),
            action_type: "script".to_string(),
            only_apps: None,
            except_apps: None,
            auto_case: false,
            interpreter: Some(ScriptInterpreter::PowerShell),
            behavior: Some(ScriptBehavior::Inline),
            script_binary: Some(compressed),
        };

        memory.load_actions(vec![("opendir".to_string(), action)]);

        let expansion = catalog
            .fetch_expansion("opendir:\"C:\\Temp\"", false, None)
            .unwrap();
        if let ExpansionStep::Script(md) = &expansion.steps[0] {
            let decompressed = decompress(&md.compressed_content).unwrap();
            assert_eq!(decompressed, "explorer C:\\Temp");
        } else {
            panic!("Expected script expansion");
        }
    }

    #[test]
    fn script_interpolation_with_named_args_matches_current_behavior() {
        let memory = Arc::new(MemorySource::new());
        let catalog = ExpansionCatalog::with_source(memory.clone());

        let script = "curl https://[env=].example.com";
        let compressed = compress(script).unwrap();

        let action = TriggerAction {
            output: String::new(),
            action_type: "script".to_string(),
            only_apps: None,
            except_apps: None,
            auto_case: false,
            interpreter: Some(ScriptInterpreter::Bash),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: Some(compressed),
        };

        memory.load_actions(vec![("api".to_string(), action)]);

        let expansion = catalog
            .fetch_expansion("api:env=prod", false, None)
            .unwrap();
        if let ExpansionStep::Script(md) = &expansion.steps[0] {
            let decompressed = decompress(&md.compressed_content).unwrap();
            assert_eq!(decompressed, "curl https://prod.example.com");
        } else {
            panic!("Expected script expansion");
        }
    }

    #[test]
    fn math_fallback_only_runs_after_snippet_tiers_miss() {
        let memory = Arc::new(MemorySource::new());
        let catalog = ExpansionCatalog::with_source(memory.clone());

        memory.load_actions(vec![(
            "5+2".to_string(),
            TriggerAction::text("exact snippet"),
        )]);

        let expansion = catalog.fetch_expansion("5+2", false, None).unwrap();
        assert_eq!(
            expansion.steps[0],
            ExpansionStep::Text("exact snippet".to_string())
        );
        assert!(!expansion.is_calculation);

        let fallback = catalog.fetch_expansion("7*6", false, None).unwrap();
        assert_eq!(fallback.steps[0], ExpansionStep::Text("42".to_string()));
        assert!(fallback.is_calculation);
    }

    #[test]
    fn math_fallback_skipped_in_instant_expand() {
        let memory = Arc::new(MemorySource::new());
        let catalog = ExpansionCatalog::with_source(memory.clone());
        assert!(catalog.fetch_expansion("7*6", true, None).is_none());
    }

    #[test]
    fn matching_triggers_returns_sorted_prefix_matches() {
        let catalog = ExpansionCatalog::new();
        catalog.load_actions(vec![
            ("gpush".to_string(), TriggerAction::text("git push")),
            ("gs".to_string(), TriggerAction::text("git status")),
            ("gco".to_string(), TriggerAction::text("git checkout")),
            ("note".to_string(), TriggerAction::text("not a g trigger")),
        ]);

        assert_eq!(
            catalog.matching_triggers("g"),
            vec!["gco".to_string(), "gpush".to_string(), "gs".to_string()]
        );
    }

    #[test]
    fn matching_triggers_uses_case_insensitive_prefix_matching() {
        let catalog = ExpansionCatalog::new();
        catalog.load_actions(vec![
            ("gm".to_string(), TriggerAction::text("good morning")),
            ("GitHub".to_string(), TriggerAction::text("github")),
        ]);

        assert_eq!(
            catalog.matching_triggers("G"),
            vec!["GitHub".to_string(), "gm".to_string()]
        );
    }

    #[test]
    fn matching_history_triggers_preserves_loaded_recency_order() {
        let catalog = ExpansionCatalog::new();
        catalog.load_actions(vec![
            ("gs".to_string(), TriggerAction::text("git status")),
            ("email".to_string(), TriggerAction::text("team update")),
            ("uuid".to_string(), TriggerAction::text("1234")),
        ]);
        catalog.load_history_triggers(vec![
            "gs".to_string(),
            "email".to_string(),
            "uuid".to_string(),
        ]);

        assert_eq!(
            catalog.matching_history_triggers(""),
            vec!["gs".to_string(), "email".to_string(), "uuid".to_string()]
        );
    }

    #[test]
    fn matching_history_triggers_filters_by_prefix_without_reordering() {
        let catalog = ExpansionCatalog::new();
        catalog.load_actions(vec![
            ("gpush".to_string(), TriggerAction::text("git push")),
            ("gs".to_string(), TriggerAction::text("git status")),
            ("email".to_string(), TriggerAction::text("team update")),
            ("gco".to_string(), TriggerAction::text("git checkout")),
        ]);
        catalog.load_history_triggers(vec![
            "gs".to_string(),
            "email".to_string(),
            "gpush".to_string(),
            "gco".to_string(),
        ]);

        assert_eq!(
            catalog.matching_history_triggers("g"),
            vec!["gs".to_string(), "gpush".to_string(), "gco".to_string()]
        );
    }

    #[test]
    fn promote_history_trigger_moves_existing_word_trigger_to_front() {
        let catalog = ExpansionCatalog::new();
        catalog.load_actions(vec![
            ("gs".to_string(), TriggerAction::text("git status")),
            ("email".to_string(), TriggerAction::text("team update")),
            ("uuid".to_string(), TriggerAction::text("1234")),
        ]);
        catalog.load_history_triggers(vec![
            "gs".to_string(),
            "email".to_string(),
            "uuid".to_string(),
        ]);

        catalog.promote_history_trigger("uuid");

        assert_eq!(
            catalog.matching_history_triggers(""),
            vec!["uuid".to_string(), "gs".to_string(), "email".to_string()]
        );
    }

    #[test]
    fn hotkey_catalog_loads_actions_without_affecting_word_expansion_lookup() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![(
            "ctrl+shift+g".to_string(),
            TriggerAction::text("git status"),
        )]);

        let action = hotkeys.get_action("ctrl+shift+g").unwrap();
        assert_eq!(action.output, "git status");

        let word_catalog = ExpansionCatalog::new();
        assert!(
            word_catalog
                .fetch_expansion("ctrl+shift+g", false, None)
                .is_none()
        );
    }

    #[test]
    fn hotkey_catalog_matches_generic_fallback_after_exact_side_miss() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![(
            "alt+m".to_string(),
            TriggerAction::text("generic alt"),
        )]);

        let (trigger, action) = hotkeys
            .match_action(parse_hotkey("ralt+m").unwrap(), None)
            .unwrap();
        assert_eq!(trigger, "alt+m");
        assert_eq!(action.output, "generic alt");
    }

    #[test]
    fn hotkey_catalog_prefers_exact_side_specific_match() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![
            ("alt+m".to_string(), TriggerAction::text("generic alt")),
            ("ralt+m".to_string(), TriggerAction::text("right alt")),
        ]);

        let (trigger, action) = hotkeys
            .match_action(parse_hotkey("ralt+m").unwrap(), None)
            .unwrap();
        assert_eq!(trigger, "ralt+m");
        assert_eq!(action.output, "right alt");
    }

    #[test]
    fn hotkey_catalog_exact_match_returns_configured_alias_not_canonical_trigger() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![(
            "altgr+m".to_string(),
            TriggerAction::text("configured alias"),
        )]);

        let (trigger, action) = hotkeys
            .match_action(parse_hotkey("ralt+m").unwrap(), None)
            .unwrap();
        assert_eq!(trigger, "altgr+m");
        assert_eq!(action.output, "configured alias");
    }

    #[test]
    fn test_app_gating_prefix_rules() {
        let mut action = TriggerAction::text("dummy");

        // 1. exe: prefix (exact match, case-insensitive, strips .exe)
        action.only_apps = Some("exe:chrome,exe:firefox".to_string());

        let info_chrome = serde_json::to_string(&ActiveWindowInfo {
            exec_name: Some("Chrome.exe".to_string()),
            ..Default::default()
        })
        .unwrap();
        let info_firefox = serde_json::to_string(&ActiveWindowInfo {
            exec_name: Some("firefox".to_string()),
            ..Default::default()
        })
        .unwrap();
        let info_notepad = serde_json::to_string(&ActiveWindowInfo {
            exec_name: Some("notepad.exe".to_string()),
            ..Default::default()
        })
        .unwrap();

        assert!(is_app_allowed(&action, Some(&info_chrome)));
        assert!(is_app_allowed(&action, Some(&info_firefox)));
        assert!(!is_app_allowed(&action, Some(&info_notepad)));

        // 2. class: prefix (exact match, case-insensitive)
        action.only_apps = Some("class:CabinetWClass".to_string());
        let info_class_match = serde_json::to_string(&ActiveWindowInfo {
            class: Some("cabinetwclass".to_string()),
            ..Default::default()
        })
        .unwrap();
        let info_class_miss = serde_json::to_string(&ActiveWindowInfo {
            class: Some("Chrome_WidgetWin_1".to_string()),
            ..Default::default()
        })
        .unwrap();

        assert!(is_app_allowed(&action, Some(&info_class_match)));
        assert!(!is_app_allowed(&action, Some(&info_class_miss)));

        // 3. title: prefix (substring match, case-insensitive)
        action.only_apps = Some("title:Github,title:Google".to_string());
        let info_title_match = serde_json::to_string(&ActiveWindowInfo {
            title: Some("Taurine Pull Request - GitHub - Google Chrome".to_string()),
            ..Default::default()
        })
        .unwrap();
        let info_title_miss = serde_json::to_string(&ActiveWindowInfo {
            title: Some("Index of /docs".to_string()),
            ..Default::default()
        })
        .unwrap();

        assert!(is_app_allowed(&action, Some(&info_title_match)));
        assert!(!is_app_allowed(&action, Some(&info_title_miss)));

        // 4. Default no prefix (exe match)
        action.only_apps = Some("chrome".to_string());
        assert!(is_app_allowed(&action, Some(&info_chrome)));
        assert!(!is_app_allowed(&action, Some(&info_notepad)));

        // 5. Exclude filters
        action.only_apps = None;
        action.except_apps = Some("title:Gmail,exe:doom".to_string());

        let info_gmail = serde_json::to_string(&ActiveWindowInfo {
            title: Some("Inbox (1) - Gmail".to_string()),
            ..Default::default()
        })
        .unwrap();
        let info_doom = serde_json::to_string(&ActiveWindowInfo {
            exec_name: Some("doom.exe".to_string()),
            ..Default::default()
        })
        .unwrap();

        assert!(!is_app_allowed(&action, Some(&info_gmail)));
        assert!(!is_app_allowed(&action, Some(&info_doom)));
        assert!(is_app_allowed(&action, Some(&info_chrome)));

        // 6. Strict mode (None active window blocks if filters are active)
        assert!(!is_app_allowed(&action, None));

        // 7. Full path match (contains path separators)
        action.except_apps = Some("exe:/usr/bin/python3,exe:C:\\bin\\python.exe".to_string());
        let info_python_linux = serde_json::to_string(&ActiveWindowInfo {
            exec_path: Some("/usr/bin/python3".to_string()),
            ..Default::default()
        })
        .unwrap();
        let info_python_win = serde_json::to_string(&ActiveWindowInfo {
            exec_path: Some("c:\\bin\\python.exe".to_string()),
            ..Default::default()
        })
        .unwrap();
        let info_python_other = serde_json::to_string(&ActiveWindowInfo {
            exec_path: Some("/usr/local/bin/python3".to_string()),
            ..Default::default()
        })
        .unwrap();

        assert!(!is_app_allowed(&action, Some(&info_python_linux)));
        assert!(!is_app_allowed(&action, Some(&info_python_win)));
        assert!(is_app_allowed(&action, Some(&info_python_other)));

        // 8. Path without prefix containing colon (Windows path edge case)
        action.except_apps = Some("C:\\bin\\python.exe".to_string());
        assert!(!is_app_allowed(&action, Some(&info_python_win)));
        assert!(is_app_allowed(&action, Some(&info_python_other)));

        // 9. Slash normalization (forward vs backward slashes)
        action.except_apps = Some("exe:C:/bin/python.exe".to_string());
        assert!(!is_app_allowed(&action, Some(&info_python_win)));
    }

    #[test]
    fn entry_has_app_filters_returns_true_when_only_apps_set() {
        let action = TriggerAction {
            only_apps: Some("chrome".to_string()),
            ..TriggerAction::text("dummy")
        };
        assert!(entry_has_app_filters(&action));
    }

    #[test]
    fn entry_has_app_filters_returns_true_when_except_apps_set() {
        let action = TriggerAction {
            except_apps: Some("notepad".to_string()),
            ..TriggerAction::text("dummy")
        };
        assert!(entry_has_app_filters(&action));
    }

    #[test]
    fn entry_has_app_filters_returns_false_when_no_filters() {
        let action = TriggerAction::text("dummy");
        assert!(!entry_has_app_filters(&action));
    }

    #[test]
    fn match_action_lazy_matches_entry_without_filters_without_calling_fetcher() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![(
            "ctrl+shift+g".to_string(),
            TriggerAction::text("git status"),
        )]);

        let called = std::cell::Cell::new(false);
        let result = hotkeys.match_action_lazy(parse_hotkey("ctrl+shift+g").unwrap(), || {
            called.set(true);
            Some("chrome.exe".to_string())
        });
        assert!(
            !called.get(),
            "fetcher should not be called for filterless entry"
        );
        let (trigger, action) = result.unwrap();
        assert_eq!(trigger, "ctrl+shift+g");
        assert_eq!(action.output, "git status");
    }

    #[test]
    fn match_action_lazy_prefers_canonical_match_over_hotkey_matches_when_both_have_filters() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![
            (
                "alt+m".to_string(),
                TriggerAction {
                    output: "generic alt".to_string(),
                    only_apps: Some("chrome".to_string()),
                    ..TriggerAction::text("")
                },
            ),
            (
                "ralt+m".to_string(),
                TriggerAction {
                    output: "right alt".to_string(),
                    only_apps: Some("chrome".to_string()),
                    ..TriggerAction::text("")
                },
            ),
        ]);

        let (trigger, action) = hotkeys
            .match_action_lazy(parse_hotkey("ralt+m").unwrap(), || {
                Some("chrome.exe".to_string())
            })
            .unwrap();
        assert_eq!(trigger, "ralt+m");
        assert_eq!(action.output, "right alt");
    }

    #[test]
    fn match_action_lazy_matches_app_filtered_entry_in_correct_window() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![(
            "ctrl+shift+g".to_string(),
            TriggerAction {
                output: "only in chrome".to_string(),
                only_apps: Some("chrome".to_string()),
                ..TriggerAction::text("")
            },
        )]);

        let (trigger, action) = hotkeys
            .match_action_lazy(parse_hotkey("ctrl+shift+g").unwrap(), || {
                Some("chrome.exe".to_string())
            })
            .unwrap();
        assert_eq!(trigger, "ctrl+shift+g");
        assert_eq!(action.output, "only in chrome");
    }

    #[test]
    fn match_action_lazy_does_not_match_app_filtered_entry_in_wrong_window() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![(
            "ctrl+shift+g".to_string(),
            TriggerAction {
                output: "chrome only".to_string(),
                only_apps: Some("chrome".to_string()),
                ..TriggerAction::text("")
            },
        )]);

        let result = hotkeys.match_action_lazy(parse_hotkey("ctrl+shift+g").unwrap(), || {
            Some("notepad.exe".to_string())
        });
        assert!(result.is_none());
    }

    #[test]
    fn match_action_lazy_returns_none_on_empty_catalog() {
        let hotkeys = HotkeyCatalog::new();
        let result = hotkeys.match_action_lazy(parse_hotkey("ctrl+shift+g").unwrap(), || {
            Some("chrome.exe".to_string())
        });
        assert!(result.is_none());
    }

    #[test]
    fn hotkey_catalog_has_entry_for_returns_false_when_empty() {
        let hotkeys = HotkeyCatalog::new();
        assert!(!hotkeys.has_entry_for(LogicalKey::Letter('g')));
    }

    #[test]
    fn hotkey_catalog_has_entry_for_returns_true_when_entries_exist() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![(
            "ctrl+shift+g".to_string(),
            TriggerAction::text("git status"),
        )]);
        assert!(hotkeys.has_entry_for(LogicalKey::Letter('g')));
        assert!(!hotkeys.has_entry_for(LogicalKey::Letter('x')));
    }

    #[test]
    fn test_regex_catalog_compilation_and_match() {
        let catalog = RegexCatalog::new();
        catalog.load_actions(vec![
            (
                "issue-(\\d+)".to_string(),
                TriggerAction::text("https://github.com/issues/[0]"),
            ),
            (
                "invalid(pattern".to_string(),
                TriggerAction::text("skipped"),
            ),
        ]);
        let matched = catalog.match_action("my issue-102", None);
        assert!(matched.is_some());
        let (trigger, action, caps) = matched.unwrap();
        assert_eq!(trigger, "issue-102");
        assert_eq!(caps, vec!["102".to_string()]);

        use crate::engine::variables::{ArgMap, ExpansionStep};
        let arg_map = ArgMap {
            positional: caps,
            ..Default::default()
        };
        let expansion = expand_trigger_action_with_args(action, &arg_map, &trigger).unwrap();
        assert_eq!(expansion.steps.len(), 1);
        if let ExpansionStep::Text(ref text) = expansion.steps[0] {
            assert_eq!(text, "https://github.com/issues/102");
        } else {
            panic!("Expected text expansion step");
        }
    }
}
