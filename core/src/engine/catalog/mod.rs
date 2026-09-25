pub(crate) mod expansion;
pub(crate) mod hotkey;
pub(crate) mod regex;
pub(crate) mod window_resolver;

pub use expansion::{
    ActiveWindowInfo, ExpansionCatalog, expand_trigger_action, expand_trigger_action_with_args,
};
pub use hotkey::HotkeyCatalog;
pub use regex::RegexCatalog;
pub use window_resolver::WindowResolver;

pub(crate) use expansion::{entry_has_app_filters, is_app_allowed, is_excluded_phrase};

#[cfg(test)]
mod tests;
