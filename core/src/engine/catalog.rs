use crate::db::crud::AutomationAction;
use crate::engine::shell::{ScriptMetadata, compress, decompress};
use crate::engine::source::{AdaptiveSource, MemorySource, SnippetSource};
use crate::engine::variables::{
    ArgMap, ExpansionStep, FinalExpansion, finalize, interpolate, parse_tokens, tokenize,
};
use crate::keys::{Hotkey, LogicalKey, hotkey_matches, parse_hotkey};

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;

pub struct ExpansionCatalog {
    source: Arc<dyn SnippetSource>,
    triggers: RwLock<Vec<String>>,
    history_triggers: RwLock<Vec<String>>,
}

#[derive(Default)]
pub struct HotkeyCatalog {
    snapshot: RwLock<CatalogSnapshot>,
}

#[derive(Default)]
struct CatalogSnapshot {
    actions: std::collections::HashMap<String, ParsedHotkeyAction>,
    parsed_actions: std::collections::HashMap<LogicalKey, Vec<ParsedHotkeyAction>>,
}

#[derive(Clone)]
struct ParsedHotkeyAction {
    configured_trigger: String,
    hotkey: Hotkey,
    action: AutomationAction,
}

impl HotkeyCatalog {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(CatalogSnapshot::default()),
        }
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, AutomationAction)>) {
        let mut snapshot = CatalogSnapshot::default();

        for (trigger, action) in actions {
            if let Ok(hotkey) = parse_hotkey(&trigger) {
                let entry = ParsedHotkeyAction {
                    configured_trigger: trigger.clone(),
                    hotkey,
                    action: action.clone(),
                };

                snapshot
                    .actions
                    .insert(hotkey.canonical_string(), entry.clone());
                snapshot.actions.insert(trigger.clone(), entry.clone());
                snapshot
                    .parsed_actions
                    .entry(hotkey.logical_key())
                    .or_default()
                    .push(entry);
            }
        }

        if let Ok(mut guard) = self.snapshot.write() {
            *guard = snapshot;
        }
    }

    pub fn get_action(&self, trigger: &str) -> Option<AutomationAction> {
        self.snapshot
            .read()
            .ok()
            .and_then(|guard| guard.actions.get(trigger).map(|entry| entry.action.clone()))
    }

    pub fn match_action(&self, pressed: Hotkey) -> Option<(String, AutomationAction)> {
        let exact_trigger = pressed.canonical_string();
        let base_key = pressed.logical_key();
        self.snapshot.read().ok().and_then(|guard| {
            if let Some(entry) = guard.actions.get(&exact_trigger) {
                return Some((entry.configured_trigger.clone(), entry.action.clone()));
            }

            guard.parsed_actions.get(&base_key).and_then(|bucket| {
                bucket
                    .iter()
                    .find(|entry| hotkey_matches(entry.hotkey, pressed))
                    .map(|entry| (entry.configured_trigger.clone(), entry.action.clone()))
            })
        })
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

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, AutomationAction)>) {
        let actions: Vec<_> = actions.into_iter().collect();
        let mut triggers: Vec<String> =
            actions.iter().map(|(trigger, _)| trigger.clone()).collect();
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
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn load_history_triggers(&self, triggers: impl IntoIterator<Item = String>) {
        let known_triggers = self
            .triggers
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let known_lookup: HashSet<String> = known_triggers.iter().cloned().collect();
        let mut seen = HashSet::new();
        let mut ordered_history = Vec::new();

        for trigger in triggers {
            if known_lookup.contains(&trigger) && seen.insert(trigger.clone()) {
                ordered_history.push(trigger);
            }
        }

        for trigger in known_triggers {
            if seen.insert(trigger.clone()) {
                ordered_history.push(trigger);
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
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn promote_history_trigger(&self, trigger: &str) {
        let known = self
            .triggers
            .read()
            .ok()
            .is_some_and(|guard| guard.iter().any(|candidate| candidate == trigger));

        if !known {
            return;
        }

        if let Ok(mut guard) = self.history_triggers.write() {
            if let Some(index) = guard.iter().position(|candidate| candidate == trigger) {
                let entry = guard.remove(index);
                guard.insert(0, entry);
            } else {
                guard.insert(0, trigger.to_string());
            }
        }
    }

    fn get_raw_action(&self, keyword: &str) -> Option<AutomationAction> {
        if let Some(action) = self.source.get_action(keyword) {
            return Some(action);
        }

        let lower_keyword = keyword.to_lowercase();
        if lower_keyword != keyword {
            return self.source.get_action(&lower_keyword);
        }

        None
    }

    fn expand_action(
        &self,
        action: AutomationAction,
        args: &ArgMap,
        matched_keyword: &str,
    ) -> Option<FinalExpansion> {
        expand_automation_action_with_args(action, args, matched_keyword)
    }

    fn fetch_exact_match(&self, keyword: &str) -> Option<FinalExpansion> {
        let action = self.get_raw_action(keyword)?;
        self.expand_action(action, &ArgMap::default(), keyword)
    }

    fn fetch_hybrid_arguments(&self, keyword: &str) -> Option<FinalExpansion> {
        let tokens = tokenize(keyword, ':');
        if tokens.len() <= 1 {
            return None;
        }

        let base = tokens.first()?.trim();
        let action = self.get_raw_action(base)?;
        let args = parse_tokens(&tokens[1..]);
        self.expand_action(action, &args, base)
    }

    fn fetch_math_fallback(&self, keyword: &str) -> Option<FinalExpansion> {
        let math_result = crate::engine::math::evaluate(keyword)?;
        let mut expansion = FinalExpansion::text(math_result);
        expansion.is_calculation = true;
        Some(expansion)
    }

    pub fn fetch_expansion(&self, keyword: &str) -> Option<FinalExpansion> {
        self.fetch_exact_match(keyword)
            .or_else(|| self.fetch_hybrid_arguments(keyword))
            .or_else(|| self.fetch_math_fallback(keyword))
    }
}

impl Default for ExpansionCatalog {
    fn default() -> Self {
        Self::new()
    }
}

fn sort_completion_triggers(triggers: &mut Vec<String>) {
    triggers.sort_by(|left, right| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
    });
    triggers.dedup();
}

pub(crate) fn expand_automation_action(
    action: AutomationAction,
    matched_keyword: &str,
) -> Option<FinalExpansion> {
    expand_automation_action_with_args(action, &ArgMap::default(), matched_keyword)
}

fn expand_automation_action_with_args(
    action: AutomationAction,
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

    Some(finalize(&interpolated, Some(matched_keyword)))
}

fn interpolate_script_action(action: AutomationAction, args: &ArgMap) -> Option<FinalExpansion> {
    let compressed = action.script_binary?;

    let decompressed = decompress(&compressed).unwrap_or_default();
    let interpolated = interpolate(&decompressed, args);
    let recompressed = compress(&interpolated).unwrap_or(compressed);

    let md = ScriptMetadata {
        interpreter: action.interpreter.unwrap(),
        behavior: action.behavior.unwrap(),
        compressed_content: recompressed,
    };

    Some(FinalExpansion {
        steps: vec![ExpansionStep::Script(md)],
        is_calculation: false,
        ai_transformer_template: None,
    })
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
            (
                "hi".to_string(),
                AutomationAction::text("base [0] ([mood])"),
            ),
            (
                "hi:erin".to_string(),
                AutomationAction::text("exact trigger wins"),
            ),
        ]);

        let expansion = catalog.fetch_expansion("hi:erin").unwrap();
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
            ("gm".to_string(), AutomationAction::text("lowercase")),
            ("GM".to_string(), AutomationAction::text("UPPERCASE")),
            (
                "only_low".to_string(),
                AutomationAction::text("only lowercase"),
            ),
        ]);

        assert_eq!(
            catalog.fetch_expansion("gm").unwrap().steps[0],
            ExpansionStep::Text("lowercase".to_string())
        );
        assert_eq!(
            catalog.fetch_expansion("GM").unwrap().steps[0],
            ExpansionStep::Text("UPPERCASE".to_string())
        );
        assert_eq!(
            catalog.fetch_expansion("Gm").unwrap().steps[0],
            ExpansionStep::Text("lowercase".to_string())
        );
        assert_eq!(
            catalog.fetch_expansion("ONLY_LOW").unwrap().steps[0],
            ExpansionStep::Text("only lowercase".to_string())
        );
        assert!(catalog.fetch_expansion("unknown").is_none());
        assert!(catalog.fetch_expansion("UNKNOWN").is_none());
    }

    #[test]
    fn script_interpolation_with_positional_args_matches_current_behavior() {
        let memory = Arc::new(MemorySource::new());
        let catalog = ExpansionCatalog::with_source(memory.clone());

        let script = "explorer [0]";
        let compressed = compress(script).unwrap();

        let action = AutomationAction {
            output: String::new(),
            action_type: "script".to_string(),
            interpreter: Some(ScriptInterpreter::PowerShell),
            behavior: Some(ScriptBehavior::Inline),
            script_binary: Some(compressed),
        };

        memory.load_actions(vec![("opendir".to_string(), action)]);

        let expansion = catalog.fetch_expansion("opendir:\"C:\\Temp\"").unwrap();
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

        let script = "curl https://[env].example.com";
        let compressed = compress(script).unwrap();

        let action = AutomationAction {
            output: String::new(),
            action_type: "script".to_string(),
            interpreter: Some(ScriptInterpreter::Bash),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: Some(compressed),
        };

        memory.load_actions(vec![("api".to_string(), action)]);

        let expansion = catalog.fetch_expansion("api:env=prod").unwrap();
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
            AutomationAction::text("exact snippet"),
        )]);

        let expansion = catalog.fetch_expansion("5+2").unwrap();
        assert_eq!(
            expansion.steps[0],
            ExpansionStep::Text("exact snippet".to_string())
        );
        assert!(!expansion.is_calculation);

        let fallback = catalog.fetch_expansion("7*6").unwrap();
        assert_eq!(fallback.steps[0], ExpansionStep::Text("42".to_string()));
        assert!(fallback.is_calculation);
    }

    #[test]
    fn matching_triggers_returns_sorted_prefix_matches() {
        let catalog = ExpansionCatalog::new();
        catalog.load_actions(vec![
            ("gpush".to_string(), AutomationAction::text("git push")),
            ("gs".to_string(), AutomationAction::text("git status")),
            ("gco".to_string(), AutomationAction::text("git checkout")),
            (
                "note".to_string(),
                AutomationAction::text("not a g trigger"),
            ),
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
            ("gm".to_string(), AutomationAction::text("good morning")),
            ("GitHub".to_string(), AutomationAction::text("github")),
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
            ("gs".to_string(), AutomationAction::text("git status")),
            ("email".to_string(), AutomationAction::text("team update")),
            ("uuid".to_string(), AutomationAction::text("1234")),
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
            ("gpush".to_string(), AutomationAction::text("git push")),
            ("gs".to_string(), AutomationAction::text("git status")),
            ("email".to_string(), AutomationAction::text("team update")),
            ("gco".to_string(), AutomationAction::text("git checkout")),
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
            ("gs".to_string(), AutomationAction::text("git status")),
            ("email".to_string(), AutomationAction::text("team update")),
            ("uuid".to_string(), AutomationAction::text("1234")),
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
            AutomationAction::text("git status"),
        )]);

        let action = hotkeys.get_action("ctrl+shift+g").unwrap();
        assert_eq!(action.output, "git status");

        let word_catalog = ExpansionCatalog::new();
        assert!(word_catalog.fetch_expansion("ctrl+shift+g").is_none());
    }

    #[test]
    fn hotkey_catalog_matches_generic_fallback_after_exact_side_miss() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![(
            "alt+m".to_string(),
            AutomationAction::text("generic alt"),
        )]);

        let (trigger, action) = hotkeys
            .match_action(parse_hotkey("ralt+m").unwrap())
            .unwrap();
        assert_eq!(trigger, "alt+m");
        assert_eq!(action.output, "generic alt");
    }

    #[test]
    fn hotkey_catalog_prefers_exact_side_specific_match() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![
            ("alt+m".to_string(), AutomationAction::text("generic alt")),
            ("ralt+m".to_string(), AutomationAction::text("right alt")),
        ]);

        let (trigger, action) = hotkeys
            .match_action(parse_hotkey("ralt+m").unwrap())
            .unwrap();
        assert_eq!(trigger, "ralt+m");
        assert_eq!(action.output, "right alt");
    }

    #[test]
    fn hotkey_catalog_exact_match_returns_configured_alias_not_canonical_trigger() {
        let hotkeys = HotkeyCatalog::new();
        hotkeys.load_actions(vec![(
            "altgr+m".to_string(),
            AutomationAction::text("configured alias"),
        )]);

        let (trigger, action) = hotkeys
            .match_action(parse_hotkey("ralt+m").unwrap())
            .unwrap();
        assert_eq!(trigger, "altgr+m");
        assert_eq!(action.output, "configured alias");
    }
}
