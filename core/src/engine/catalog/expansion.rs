use std::sync::{Arc, RwLock};

use super::WindowResolver;
use crate::db::crud::TriggerAction;
use crate::engine::shell::{ScriptMetadata, compress, decompress};
use crate::engine::source::{AdaptiveSource, MemorySource, SnippetSource};
use crate::engine::variables::{
    ArgMap, ExpansionStep, FinalExpansion, finalize, interpolate, parse_tokens, tokenize,
};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreNormalizedTrigger {
    pub original: Arc<str>,
    pub normalized: Arc<str>,
}

pub struct ExpansionCatalog {
    source: Arc<dyn SnippetSource>,
    triggers: RwLock<Vec<PreNormalizedTrigger>>,
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
        let mut triggers: Vec<PreNormalizedTrigger> = actions
            .iter()
            .map(|(trigger, _)| PreNormalizedTrigger {
                original: Arc::from(trigger.as_str()),
                normalized: Arc::from(trigger.to_lowercase().as_str()),
            })
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
        let guard = match self.triggers.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let start_idx =
            guard.partition_point(|entry| entry.normalized.as_ref() < normalized_prefix.as_str());

        guard[start_idx..]
            .iter()
            .take_while(|entry| entry.normalized.starts_with(&normalized_prefix))
            .map(|entry| entry.original.as_ref().to_string())
            .collect()
    }

    fn get_raw_action_lazy(
        &self,
        keyword: &str,
        window: &WindowResolver,
        fetch_window: &mut Option<impl FnOnce() -> Option<ActiveWindowInfo>>,
    ) -> Option<TriggerAction> {
        if let Some(action) = self.source.get_action(keyword) {
            if !entry_has_app_filters(&action) {
                return Some(action);
            }
            let w = window.resolve(|| fetch_window.take().and_then(|f| f()));
            if is_app_allowed(&action, w) {
                return Some(action);
            }
        }

        let lower_keyword = keyword.to_lowercase();
        if lower_keyword != keyword
            && let Some(action) = self.source.get_action(&lower_keyword)
        {
            if !entry_has_app_filters(&action) {
                return Some(action);
            }
            let w = window.resolve(|| fetch_window.take().and_then(|f| f()));
            if is_app_allowed(&action, w) {
                return Some(action);
            }
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

    fn fetch_exact_match_lazy(
        &self,
        keyword: &str,
        window: &WindowResolver,
        fetch_window: &mut Option<impl FnOnce() -> Option<ActiveWindowInfo>>,
    ) -> Option<FinalExpansion> {
        let action = self.get_raw_action_lazy(keyword, window, fetch_window)?;
        self.expand_action(action, &ArgMap::default(), keyword)
    }

    fn fetch_hybrid_arguments_lazy(
        &self,
        keyword: &str,
        window: &WindowResolver,
        fetch_window: &mut Option<impl FnOnce() -> Option<ActiveWindowInfo>>,
    ) -> Option<FinalExpansion> {
        let tokens = tokenize(keyword, ':');
        if tokens.len() <= 1 {
            return None;
        }

        let base = tokens.first()?.trim();
        let action = self.get_raw_action_lazy(base, window, fetch_window)?;
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

    pub fn fetch_expansion_lazy(
        &self,
        keyword: &str,
        instant_expand: bool,
        window: &WindowResolver,
        mut fetch_window: Option<impl FnOnce() -> Option<ActiveWindowInfo>>,
    ) -> Option<FinalExpansion> {
        self.fetch_exact_match_lazy(keyword, window, &mut fetch_window)
            .or_else(|| self.fetch_hybrid_arguments_lazy(keyword, window, &mut fetch_window))
            .or_else(|| self.fetch_math_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_date_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_timezone_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_currency_words_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_nl_unit_conversion_fallback(keyword, instant_expand))
    }

    pub fn fetch_expansion(
        &self,
        keyword: &str,
        instant_expand: bool,
        active_window: Option<&str>,
    ) -> Option<FinalExpansion> {
        let window = WindowResolver::from_static(active_window);
        self.fetch_expansion_lazy(
            keyword,
            instant_expand,
            &window,
            None::<fn() -> Option<ActiveWindowInfo>>,
        )
    }

    pub fn fetch_expansion_info(
        &self,
        keyword: &str,
        instant_expand: bool,
        active_window: Option<ActiveWindowInfo>,
    ) -> Option<FinalExpansion> {
        let window = WindowResolver::from_info(active_window);
        self.fetch_expansion_lazy(
            keyword,
            instant_expand,
            &window,
            None::<fn() -> Option<ActiveWindowInfo>>,
        )
    }

    pub fn fetch_expansion_no_date_fallback_lazy(
        &self,
        keyword: &str,
        instant_expand: bool,
        window: &WindowResolver,
        mut fetch_window: Option<impl FnOnce() -> Option<ActiveWindowInfo>>,
    ) -> Option<FinalExpansion> {
        self.fetch_exact_match_lazy(keyword, window, &mut fetch_window)
            .or_else(|| self.fetch_hybrid_arguments_lazy(keyword, window, &mut fetch_window))
            .or_else(|| self.fetch_math_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_currency_words_fallback(keyword, instant_expand))
            .or_else(|| self.fetch_nl_unit_conversion_fallback(keyword, instant_expand))
    }

    pub fn fetch_expansion_no_date_fallback(
        &self,
        keyword: &str,
        instant_expand: bool,
        active_window: Option<&str>,
    ) -> Option<FinalExpansion> {
        let window = WindowResolver::from_static(active_window);
        self.fetch_expansion_no_date_fallback_lazy(
            keyword,
            instant_expand,
            &window,
            None::<fn() -> Option<ActiveWindowInfo>>,
        )
    }

    pub fn fetch_expansion_no_date_fallback_info(
        &self,
        keyword: &str,
        instant_expand: bool,
        active_window: Option<ActiveWindowInfo>,
    ) -> Option<FinalExpansion> {
        let window = WindowResolver::from_info(active_window);
        self.fetch_expansion_no_date_fallback_lazy(
            keyword,
            instant_expand,
            &window,
            None::<fn() -> Option<ActiveWindowInfo>>,
        )
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
fn sort_completion_triggers(triggers: &mut Vec<PreNormalizedTrigger>) {
    triggers.sort_by(|left, right| {
        left.normalized
            .cmp(&right.normalized)
            .then_with(|| left.original.cmp(&right.original))
    });
    triggers.dedup_by(|a, b| a.normalized == b.normalized && a.original == b.original);
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
pub(crate) fn is_app_allowed(
    action: &TriggerAction,
    active_window: Option<&ActiveWindowInfo>,
) -> bool {
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

    let Some(info) = active_window else {
        return false;
    };

    if has_only {
        let Some(allowed) = action.only_apps.as_ref() else {
            return false;
        };
        if !match_rules(allowed, info) {
            return false;
        }
    }

    if has_except {
        let Some(denied) = action.except_apps.as_ref() else {
            return false;
        };
        if match_rules(denied, info) {
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

    if args.positional.is_empty()
        && args.named.is_empty()
        && !action.output.contains('[')
        && !action.output.contains('|')
        && !action.output.contains('\\')
    {
        let output = if action.auto_case {
            apply_auto_case(&action.output, matched_keyword)
        } else {
            action.output
        };
        return Some(FinalExpansion {
            steps: if output.is_empty() {
                Vec::new()
            } else {
                vec![ExpansionStep::Text(output)]
            },
            is_calculation: false,
            ai_transformer_template: None,
        });
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
