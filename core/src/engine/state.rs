use crate::engine::shell::{ScriptMetadata, compress, decompress};
use crate::engine::source::{AdaptiveSource, MemorySource, SnippetSource};
use crate::engine::variables::{
    ArgMap, ExpansionStep, FinalExpansion, finalize, interpolate, parse_tokens, tokenize,
};

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

    pub fn load_actions(
        &self,
        actions: impl IntoIterator<Item = (String, crate::db::crud::AutomationAction)>,
    ) {
        self.source.load_actions(actions.into_iter().collect());
    }

    fn get_raw_action(&self, keyword: &str) -> Option<crate::db::crud::AutomationAction> {
        self.source.get_action(keyword)
    }

    fn interpolate_script(
        &self,
        action: crate::db::crud::AutomationAction,
        args: &ArgMap,
    ) -> Option<FinalExpansion> {
        let compressed = action.script_binary?;

        // 1. Decompress
        let decompressed = decompress(&compressed).unwrap_or_default();

        // 2. Interpolate using the existing engine
        let interpolated = interpolate(&decompressed, args);

        // 3. Recompress for the downstream daemon executor.
        // If recompression fails, fallback to the original compressed binary to avoid panicking the hot-path.
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

    pub fn fetch_expansion(
        &self,
        keyword: &str,
    ) -> Option<crate::engine::variables::FinalExpansion> {
        // 1. Try exact match on `keyword` FIRST
        if let Some(action) = self.get_raw_action(keyword) {
            let args = ArgMap::default();
            if action.action_type == "script" {
                return self.interpolate_script(action, &args);
            } else {
                let interpolated = interpolate(&action.output, &args);
                return Some(finalize(&interpolated, Some(keyword)));
            }
        }

        // 2. Chained colon tokenization
        let tokens = tokenize(keyword, ':');
        if tokens.len() > 1 {
            let base = &tokens[0];
            if let Some(action) = self.get_raw_action(base) {
                let args = parse_tokens(&tokens[1..]);
                if action.action_type == "script" {
                    return self.interpolate_script(action, &args);
                } else {
                    let interpolated = interpolate(&action.output, &args);
                    return Some(finalize(&interpolated, Some(base)));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::AutomationAction;
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, compress, decompress};
    use crate::engine::source::MemorySource;
    use crate::engine::variables::ExpansionStep;
    use std::sync::Arc;

    #[test]
    fn test_script_interpolation_exact_match() {
        let memory = Arc::new(MemorySource::new());
        let state = EngineState::with_source('>', memory.clone());

        // Script with a positional argument inside it
        let script = "echo {0}";
        let compressed = compress(script).unwrap();

        let action = AutomationAction {
            output: String::new(),
            action_type: "script".to_string(),
            interpreter: Some(ScriptInterpreter::Bash),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: Some(compressed),
        };

        memory.load_actions(vec![("ip".to_string(), action)]);

        // Exact match "ip" should NOT provide arguments, so {0} remains as is
        let expansion = state.fetch_expansion("ip").unwrap();
        if let ExpansionStep::Script(md) = &expansion.steps[0] {
            let decompressed = decompress(&md.compressed_content).unwrap();
            assert_eq!(decompressed, "echo {0}");
        } else {
            panic!("Expected script expansion");
        }
    }

    #[test]
    fn test_script_interpolation_with_chained_args() {
        let memory = Arc::new(MemorySource::new());
        let state = EngineState::with_source('>', memory.clone());

        let script = "explorer {0}";
        let compressed = compress(script).unwrap();

        let action = AutomationAction {
            output: String::new(),
            action_type: "script".to_string(),
            interpreter: Some(ScriptInterpreter::PowerShell),
            behavior: Some(ScriptBehavior::Inline),
            script_binary: Some(compressed),
        };

        memory.load_actions(vec![("opendir".to_string(), action)]);

        // Test colon-delimited arguments with quotes to prevent splitting on drive colon: >opendir:"C:\Temp"
        let expansion = state.fetch_expansion("opendir:\"C:\\Temp\"").unwrap();
        if let ExpansionStep::Script(md) = &expansion.steps[0] {
            let decompressed = decompress(&md.compressed_content).unwrap();
            assert_eq!(decompressed, "explorer C:\\Temp");
        } else {
            panic!("Expected script expansion");
        }
    }

    #[test]
    fn test_script_interpolation_with_named_args() {
        let memory = Arc::new(MemorySource::new());
        let state = EngineState::with_source('>', memory.clone());

        let script = "curl https://{env}.example.com";
        let compressed = compress(script).unwrap();

        let action = AutomationAction {
            output: String::new(),
            action_type: "script".to_string(),
            interpreter: Some(ScriptInterpreter::Bash),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: Some(compressed),
        };

        memory.load_actions(vec![("api".to_string(), action)]);

        // Test named arguments: >api:env=prod
        let expansion = state.fetch_expansion("api:env=prod").unwrap();
        if let ExpansionStep::Script(md) = &expansion.steps[0] {
            let decompressed = decompress(&md.compressed_content).unwrap();
            assert_eq!(decompressed, "curl https://prod.example.com");
        } else {
            panic!("Expected script expansion");
        }
    }
}
