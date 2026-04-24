use crate::db::crud::AutomationAction;
use crate::engine::shell::{ScriptMetadata, compress, decompress};
use crate::engine::source::{AdaptiveSource, MemorySource, SnippetSource};
use crate::engine::variables::{
    ArgMap, ExpansionStep, FinalExpansion, finalize, interpolate, parse_tokens, tokenize,
};

use std::sync::Arc;
use std::sync::RwLock;

pub struct ExpansionCatalog {
    source: Arc<dyn SnippetSource>,
}

#[derive(Default)]
pub struct HotkeyCatalog {
    actions: RwLock<std::collections::HashMap<String, AutomationAction>>,
}

impl HotkeyCatalog {
    pub fn new() -> Self {
        Self {
            actions: RwLock::new(std::collections::HashMap::new()),
        }
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, AutomationAction)>) {
        if let Ok(mut guard) = self.actions.write() {
            *guard = actions.into_iter().collect();
        }
    }

    pub fn get_action(&self, trigger: &str) -> Option<AutomationAction> {
        self.actions
            .read()
            .ok()
            .and_then(|guard| guard.get(trigger).cloned())
    }
}

impl ExpansionCatalog {
    pub fn new() -> Self {
        let memory = Arc::new(MemorySource::new());
        let adaptive = Arc::new(AdaptiveSource::new(memory));
        Self { source: adaptive }
    }

    pub fn with_source(source: Arc<dyn SnippetSource>) -> Self {
        Self { source }
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, AutomationAction)>) {
        self.source.load_actions(actions.into_iter().collect());
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

    fn interpolate_script(
        &self,
        action: AutomationAction,
        args: &ArgMap,
    ) -> Option<FinalExpansion> {
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
        })
    }

    fn expand_action(
        &self,
        action: AutomationAction,
        args: &ArgMap,
        matched_keyword: &str,
    ) -> Option<FinalExpansion> {
        if action.action_type == "script" {
            self.interpolate_script(action, args)
        } else {
            let interpolated = interpolate(&action.output, args);
            Some(finalize(&interpolated, Some(matched_keyword)))
        }
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
}
