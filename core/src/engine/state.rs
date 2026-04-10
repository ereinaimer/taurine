use crate::engine::source::{AdaptiveSource, MemorySource, SnippetSource};
use std::sync::Arc;

pub struct EngineState {
    pub trigger_char: char,
    pub source: Arc<dyn SnippetSource>,
}

impl EngineState {
    pub fn new(trigger_char: char) -> Self {
        let memory = Arc::new(MemorySource::new());
        let adaptive = Arc::new(AdaptiveSource::new(memory));
        Self {
            trigger_char,
            source: adaptive,
        }
    }

    /// Creates an EngineState with a custom snippet source.
    pub fn with_source(trigger_char: char, source: Arc<dyn SnippetSource>) -> Self {
        Self {
            trigger_char,
            source,
        }
    }

    pub fn load_snippets(&self, snippets: impl IntoIterator<Item = (String, String)>) {
        self.source.load_snippets(snippets.into_iter().collect());
    }

    fn get_raw_expansion(&self, keyword: &str) -> Option<String> {
        self.source.get_snippet(keyword)
    }

    pub fn fetch_expansion(
        &self,
        keyword: &str,
    ) -> Option<crate::engine::variables::FinalExpansion> {
        // 1. Try exact match on `keyword` FIRST
        if let Some(template) = self.get_raw_expansion(keyword) {
            // Task 2.3: No-Argument Default Handling
            let args = crate::engine::variables::ArgMap::default();
            let interpolated =
                crate::engine::variables::interpolate(&template, &args, Some(keyword));
            return Some(crate::engine::variables::finalize(
                &interpolated,
                Some(keyword),
            ));
        }

        // 2. Task 2.1: Add hyphen-split fallback logic
        if let Some((base, raw_args)) = keyword.split_once('-')
            && let Some(template) = self.get_raw_expansion(base)
        {
            // Task 2.2: Hook up interpolation
            let args = crate::engine::variables::parse_args(raw_args);
            let interpolated = crate::engine::variables::interpolate(&template, &args, Some(base));
            return Some(crate::engine::variables::finalize(
                &interpolated,
                Some(base),
            ));
        }

        None
    }
}
