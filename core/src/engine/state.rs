use crate::engine::source::{AdaptiveSource, MemorySource, SnippetSource};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

pub struct EngineState {
    pub trigger_char: AtomicU32,
    pub source: Arc<dyn SnippetSource>,
}

impl EngineState {
    pub fn new(trigger_char: char) -> Self {
        let memory = Arc::new(MemorySource::new());
        let adaptive = Arc::new(AdaptiveSource::new(memory));
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            source: adaptive,
        }
    }

    /// Creates an EngineState with a custom snippet source.
    pub fn with_source(trigger_char: char, source: Arc<dyn SnippetSource>) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            source,
        }
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, crate::db::crud::AutomationAction)>) {
        self.source.load_actions(actions.into_iter().collect());
    }

    fn get_raw_action(&self, keyword: &str) -> Option<crate::db::crud::AutomationAction> {
        self.source.get_action(keyword)
    }

    pub fn fetch_expansion(
        &self,
        keyword: &str,
    ) -> Option<crate::engine::variables::FinalExpansion> {
        // 1. Try exact match on `keyword` FIRST
        if let Some(action) = self.get_raw_action(keyword) {
            if action.action_type == "script" {
                let md = crate::engine::shell::ScriptMetadata {
                    interpreter: action.interpreter.unwrap(),
                    behavior: action.behavior.unwrap(),
                    compressed_content: action.script_binary.unwrap(),
                };
                return Some(crate::engine::variables::FinalExpansion {
                    steps: vec![crate::engine::variables::ExpansionStep::Script(md)],
                    is_calculation: false,
                });
            } else {
                // Task 2.3: No-Argument Default Handling for Text Expanders
                let args = crate::engine::variables::ArgMap::default();
                let interpolated = crate::engine::variables::interpolate(&action.output, &args);
                return Some(crate::engine::variables::finalize(
                    &interpolated,
                    Some(keyword),
                ));
            }
        }

        // 2. Chained colon tokenization
        let tokens = crate::engine::variables::tokenize(keyword, ':');
        if tokens.len() > 1 {
            let base = &tokens[0];
            if let Some(action) = self.get_raw_action(base) {
                // Scripts don't currently support chained colon arguments
                if action.action_type != "script" {
                    let args = crate::engine::variables::parse_tokens(&tokens[1..]);
                    let interpolated = crate::engine::variables::interpolate(&action.output, &args);
                    return Some(crate::engine::variables::finalize(
                        &interpolated,
                        Some(base),
                    ));
                }
            }
        }
        // 3. Fallback to inline math evaluation
        if let Some(math_result) = crate::engine::math::evaluate(keyword) {
            let mut fe = crate::engine::variables::FinalExpansion::text(math_result);
            fe.is_calculation = true;
            return Some(fe);
        }

        None
    }
}
