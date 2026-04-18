use crate::engine::shell::{ScriptMetadata, compress, decompress};
use crate::engine::source::{AdaptiveSource, MemorySource, SnippetSource};
use crate::engine::variables::{
    ArgMap, ExpansionStep, FinalExpansion, finalize, interpolate, parse_tokens, tokenize,
};

use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EngineMode {
    #[default]
    Normal,
    AiCapture,
}

pub struct EngineState {
    pub trigger_char: AtomicU32,
    pub source: Arc<dyn SnippetSource>,
    pub mode: RwLock<EngineMode>,
    pub ai_prompt_buffer: Mutex<String>,
}

impl EngineState {
    pub fn new(trigger_char: char) -> Self {
        let memory = Arc::new(MemorySource::new());
        let adaptive = Arc::new(AdaptiveSource::new(memory));
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            source: adaptive,
            mode: RwLock::new(EngineMode::Normal),
            ai_prompt_buffer: Mutex::new(String::new()),
        }
    }

    /// Creates an EngineState with a custom snippet source.
    pub fn with_source(trigger_char: char, source: Arc<dyn SnippetSource>) -> Self {
        Self {
            trigger_char: AtomicU32::new(trigger_char as u32),
            source,
            mode: RwLock::new(EngineMode::Normal),
            ai_prompt_buffer: Mutex::new(String::new()),
        }
    }

    pub fn engine_mode(&self) -> EngineMode {
        self.mode.read().map(|guard| *guard).unwrap_or_default()
    }

    pub fn set_engine_mode(&self, mode: EngineMode) {
        if let Ok(mut guard) = self.mode.write() {
            *guard = mode;
        }
    }

    pub fn append_ai_prompt_char(&self, c: char) {
        if let Ok(mut prompt) = self.ai_prompt_buffer.lock() {
            prompt.push(c);
        }
    }

    pub fn pop_ai_prompt_char(&self) {
        if let Ok(mut prompt) = self.ai_prompt_buffer.lock() {
            prompt.pop();
        }
    }

    pub fn pop_ai_prompt_word(&self) {
        if let Ok(mut prompt) = self.ai_prompt_buffer.lock() {
            pop_last_word_from_prompt(&mut prompt);
        }
    }

    pub fn clear_ai_prompt_buffer(&self) {
        if let Ok(mut prompt) = self.ai_prompt_buffer.lock() {
            prompt.clear();
        }
    }

    pub fn ai_prompt_buffer(&self) -> String {
        self.ai_prompt_buffer
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn is_ai_prompt_empty(&self) -> bool {
        self.ai_prompt_buffer
            .lock()
            .map(|guard| guard.is_empty())
            .unwrap_or(true)
    }

    pub fn load_actions(
        &self,
        actions: impl IntoIterator<Item = (String, crate::db::crud::AutomationAction)>,
    ) {
        self.source.load_actions(actions.into_iter().collect());
    }

    fn get_raw_action(&self, keyword: &str) -> Option<crate::db::crud::AutomationAction> {
        // 1. Try exact case-sensitive match first
        if let Some(action) = self.source.get_action(keyword) {
            return Some(action);
        }

        // 2. Optimization: Only attempt a second lookup if the keyword actually contains uppercase letters
        let lower_keyword = keyword.to_lowercase();
        if lower_keyword != keyword {
            return self.source.get_action(&lower_keyword);
        }

        None
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

    fn expand_action(
        &self,
        action: crate::db::crud::AutomationAction,
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

    pub fn fetch_expansion(
        &self,
        keyword: &str,
    ) -> Option<crate::engine::variables::FinalExpansion> {
        self.fetch_exact_match(keyword)
            .or_else(|| self.fetch_hybrid_arguments(keyword))
            .or_else(|| self.fetch_math_fallback(keyword))
    }
}

fn pop_last_word_from_prompt(prompt: &mut String) {
    if prompt.is_empty() {
        return;
    }

    let mut chars: Vec<char> = prompt.chars().collect();

    while let Some(last) = chars.last() {
        if last.is_whitespace() {
            chars.pop();
        } else {
            break;
        }
    }

    let Some(last) = chars.last().copied() else {
        prompt.clear();
        return;
    };

    let is_alphanumeric = last.is_alphanumeric();
    while let Some(current) = chars.last().copied() {
        if current.is_whitespace() || current.is_alphanumeric() != is_alphanumeric {
            break;
        }
        chars.pop();
    }

    *prompt = chars.into_iter().collect();
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
        let script = "echo [0]";
        let compressed = compress(script).unwrap();

        let action = AutomationAction {
            output: String::new(),
            action_type: "script".to_string(),
            interpreter: Some(ScriptInterpreter::Bash),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: Some(compressed),
        };

        memory.load_actions(vec![("ip".to_string(), action)]);

        // Exact match "ip" should NOT provide arguments, so [0] remains as is
        let expansion = state.fetch_expansion("ip").unwrap();
        if let ExpansionStep::Script(md) = &expansion.steps[0] {
            let decompressed = decompress(&md.compressed_content).unwrap();
            assert_eq!(decompressed, "echo [0]");
        } else {
            panic!("Expected script expansion");
        }
    }

    #[test]
    fn test_script_interpolation_with_chained_args() {
        let memory = Arc::new(MemorySource::new());
        let state = EngineState::with_source('>', memory.clone());

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

        // Test named arguments: >api:env=prod
        let expansion = state.fetch_expansion("api:env=prod").unwrap();
        if let ExpansionStep::Script(md) = &expansion.steps[0] {
            let decompressed = decompress(&md.compressed_content).unwrap();
            assert_eq!(decompressed, "curl https://prod.example.com");
        } else {
            panic!("Expected script expansion");
        }
    }

    #[test]
    fn test_exact_match_tier_beats_hybrid_argument_parsing() {
        let memory = Arc::new(MemorySource::new());
        let state = EngineState::with_source('>', memory.clone());

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

        let expansion = state.fetch_expansion("hi:erin").unwrap();
        assert_eq!(
            expansion.steps[0],
            ExpansionStep::Text("exact trigger wins".to_string())
        );
        assert!(!expansion.is_calculation);
    }

    #[test]
    fn test_hybrid_arguments_preserve_positional_and_named_tokens() {
        let memory = Arc::new(MemorySource::new());
        let state = EngineState::with_source('>', memory.clone());

        memory.load_actions(vec![(
            "hi".to_string(),
            AutomationAction::text("Hi [0], mood [mood]"),
        )]);

        let expansion = state.fetch_expansion("hi:erein:mood=sad").unwrap();
        assert_eq!(
            expansion.steps[0],
            ExpansionStep::Text("Hi erein, mood sad".to_string())
        );
        assert!(!expansion.is_calculation);
    }

    #[test]
    fn test_math_fallback_only_runs_after_snippet_tiers_miss() {
        let memory = Arc::new(MemorySource::new());
        let state = EngineState::with_source('>', memory.clone());

        memory.load_actions(vec![(
            "5+2".to_string(),
            AutomationAction::text("exact snippet"),
        )]);

        let expansion = state.fetch_expansion("5+2").unwrap();
        assert_eq!(
            expansion.steps[0],
            ExpansionStep::Text("exact snippet".to_string())
        );
        assert!(!expansion.is_calculation);

        let fallback = state.fetch_expansion("7*6").unwrap();
        assert_eq!(fallback.steps[0], ExpansionStep::Text("42".to_string()));
        assert!(fallback.is_calculation);
    }

    #[test]
    fn test_smart_match_fallback() {
        let memory = Arc::new(MemorySource::new());
        let state = EngineState::with_source('>', memory.clone());

        memory.load_actions(vec![
            ("gm".to_string(), AutomationAction::text("lowercase")),
            ("GM".to_string(), AutomationAction::text("UPPERCASE")),
            (
                "only_low".to_string(),
                AutomationAction::text("only lowercase"),
            ),
        ]);

        // 1. Exact match (lowercase)
        assert_eq!(
            state.fetch_expansion("gm").unwrap().steps[0],
            ExpansionStep::Text("lowercase".to_string())
        );

        // 2. Exact match (uppercase)
        assert_eq!(
            state.fetch_expansion("GM").unwrap().steps[0],
            ExpansionStep::Text("UPPERCASE".to_string())
        );

        // 3. Fallback match (typed Mixed, falls back to lowercase)
        assert_eq!(
            state.fetch_expansion("Gm").unwrap().steps[0],
            ExpansionStep::Text("lowercase".to_string())
        );

        // 4. Fallback match (typed Mixed, falls back to lowercase when only lowercase exists)
        assert_eq!(
            state.fetch_expansion("ONLY_LOW").unwrap().steps[0],
            ExpansionStep::Text("only lowercase".to_string())
        );

        // 5. No match
        assert!(state.fetch_expansion("unknown").is_none());
        assert!(state.fetch_expansion("UNKNOWN").is_none());
    }

    #[test]
    fn engine_state_defaults_to_normal_mode_with_empty_ai_prompt() {
        let state = EngineState::new('>');

        assert_eq!(state.engine_mode(), EngineMode::Normal);
        assert_eq!(state.ai_prompt_buffer(), "");
    }

    #[test]
    fn engine_state_ai_prompt_helpers_track_chars_and_words() {
        let state = EngineState::new('>');

        state.set_engine_mode(EngineMode::AiCapture);
        state.append_ai_prompt_char('h');
        state.append_ai_prompt_char('i');
        state.append_ai_prompt_char(' ');
        state.append_ai_prompt_char('世');
        state.append_ai_prompt_char('界');
        assert_eq!(state.ai_prompt_buffer(), "hi 世界");

        state.pop_ai_prompt_char();
        assert_eq!(state.ai_prompt_buffer(), "hi 世");

        state.pop_ai_prompt_word();
        assert_eq!(state.ai_prompt_buffer(), "hi ");

        state.clear_ai_prompt_buffer();
        assert_eq!(state.ai_prompt_buffer(), "");
        assert_eq!(state.engine_mode(), EngineMode::AiCapture);
        assert!(state.is_ai_prompt_empty());
    }
}
