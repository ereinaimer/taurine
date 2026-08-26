use std::sync::{Arc, RwLock};

use crate::db::crud::TriggerAction;
use crate::engine::shell::{ScriptMetadata, compress, decompress};
use crate::engine::source::{AdaptiveSource, MemorySource, SnippetSource};
use crate::engine::variables::{
    ArgMap, ExpansionStep, FinalExpansion, finalize, interpolate, parse_tokens, tokenize,
};
pub struct ExpansionCatalog {
    source: Arc<dyn SnippetSource>,
    triggers: RwLock<Vec<Arc<str>>>,
}
impl ExpansionCatalog {
    pub fn new() -> Self {
        let memory = Arc::new(MemorySource::new());
        let adaptive = Arc::new(AdaptiveSource::new(memory));
        Self {
            source: adaptive,
            triggers: RwLock::new(Vec::new()),
        }
    }

    pub fn with_source(source: Arc<dyn SnippetSource>) -> Self {
        Self {
            source,
            triggers: RwLock::new(Vec::new()),
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

    fn fetch_date_fallback(&self, keyword: &str, instant_expand: bool) -> Option<FinalExpansion> {
        if instant_expand {
            return None;
        }
        if !crate::settings::get_cached_inline_datetime_enabled() {
            return None;
        }
        let dialect = crate::settings::get_cached_inline_datetime_dialect();

        // Check for countdown queries first: "how many days until christmas?"
        if let Some(countdown) = crate::engine::dates::parse_countdown_query(keyword, &dialect) {
            let mut expansion = FinalExpansion::text(countdown);
            expansion.is_calculation = true;
            return Some(expansion);
        }

        // Check for date queries: "what is the date next friday?"
        if let Some(date_str) = crate::engine::dates::parse_date_query(keyword, &dialect) {
            let mut expansion = FinalExpansion::text(date_str);
            expansion.is_calculation = true;
            return Some(expansion);
        }

        // Require an explicit direction signal — bare quantities, bare times, and
        // absolute dates are intentionally excluded.
        // Exception: "now" is allowed in prefix-triggered mode (e.g. ">now") but
        // is excluded from triggerless mode via has_expansion_intent returning false.
        let is_prefix_now = keyword.trim() == "now";
        if !is_prefix_now && !crate::engine::dates::has_expansion_intent(keyword) {
            return None;
        }

        // Strip leading +/- so interim receives a clean phrase;
        // - prefix is already handled by preprocess_date_phrase (converts to "ago" suffix)
        let cleaned = keyword.trim_start_matches('+');
        if is_excluded_phrase(cleaned) {
            return None;
        }

        let (dt, is_date, is_time) = crate::engine::dates::parse_natural_date(cleaned, &dialect)?;
        let pattern = if is_date && is_time {
            crate::settings::get_cached_inline_datetime_datetime_format()
        } else if is_time {
            crate::settings::get_cached_inline_datetime_time_format()
        } else {
            crate::settings::get_cached_inline_datetime_date_format()
        };

        let formatted = crate::engine::dates::format_datetime(dt, &pattern);
        let mut expansion = FinalExpansion::text(formatted);
        expansion.is_calculation = true;
        Some(expansion)
    }

    fn fetch_currency_words_fallback(
        &self,
        keyword: &str,
        instant_expand: bool,
    ) -> Option<FinalExpansion> {
        if instant_expand {
            return None;
        }
        if !crate::settings::get_cached_inline_currency_to_words_enabled() {
            return None;
        }

        if !crate::engine::conversion::currency::has_currency_prefix(keyword) {
            return None;
        }

        let parsed_words = crate::engine::conversion::currency::convert_to_words(keyword)?;
        let mut expansion = FinalExpansion::text(parsed_words);
        expansion.is_calculation = true;
        Some(expansion)
    }

    fn fetch_nl_unit_conversion_fallback(
        &self,
        keyword: &str,
        instant_expand: bool,
    ) -> Option<FinalExpansion> {
        if instant_expand {
            return None;
        }
        let dummy_state = crate::engine::state::EngineState::new();
        let parsed_words = crate::engine::conversion::convert_natural(keyword, &dummy_state)?;
        let mut expansion = FinalExpansion::text(parsed_words);
        expansion.is_calculation = true;
        Some(expansion)
    }

    fn fetch_timezone_fallback(
        &self,
        keyword: &str,
        instant_expand: bool,
    ) -> Option<FinalExpansion> {
        if instant_expand {
            return None;
        }
        if !crate::settings::get_cached_inline_datetime_enabled() {
            return None;
        }
        let time_format = crate::settings::get_cached_inline_datetime_time_format();
        let dialect = crate::settings::get_cached_inline_datetime_dialect();
        let result =
            crate::engine::timezones::parse_timezone_expression(keyword, &time_format, &dialect)?;
        let mut expansion = FinalExpansion::text(result);
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
            .or_else(|| self.fetch_date_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_timezone_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_currency_words_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_nl_unit_conversion_fallback(keyword, instant_expand))
    }

    pub fn fetch_expansion_no_date_fallback(
        &self,
        keyword: &str,
        instant_expand: bool,
        active_window: Option<&str>,
    ) -> Option<FinalExpansion> {
        self.fetch_exact_match(keyword, active_window)
            .or_else(|| self.fetch_hybrid_arguments(keyword, active_window))
            .or_else(|| self.fetch_math_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_currency_words_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_nl_unit_conversion_fallback(keyword, instant_expand))
    }
}
pub(crate) fn is_excluded_phrase(phrase: &str) -> bool {
    let trimmed = phrase.trim().to_lowercase();
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let bare_words = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
        "mon",
        "tue",
        "wed",
        "thu",
        "fri",
        "sat",
        "sun",
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
        "jan",
        "feb",
        "mar",
        "apr",
        "jun",
        "jul",
        "aug",
        "sep",
        "oct",
        "nov",
        "dec",
    ];
    bare_words.contains(&trimmed.as_str())
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
fn split_app_filters(input: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&',') => {
                current.push(',');
                chars.next();
            }
            ',' => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    items.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        items.push(trimmed);
    }
    items
}
fn match_rules(filter_list: &str, info: &ActiveWindowInfo) -> bool {
    let rules: Vec<String> = split_app_filters(filter_list);
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
                ("exe".to_string(), rule.as_str())
            }
        } else {
            ("exe".to_string(), rule.as_str())
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
pub(crate) fn entry_has_app_filters(action: &TriggerAction) -> bool {
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
        let Some(allowed) = action.only_apps.as_ref() else {
            return false;
        };
        if !match_rules(allowed, &info) {
            return false;
        }
    }

    if has_except {
        let Some(denied) = action.except_apps.as_ref() else {
            return false;
        };
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
    if action.is_script() {
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

    let interpreter = action.interpreter?;
    let behavior = action.behavior?;

    let md = ScriptMetadata {
        interpreter,
        behavior,
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
