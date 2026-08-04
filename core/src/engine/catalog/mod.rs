pub(crate) mod expansion;
pub(crate) mod hotkey;
pub(crate) mod regex;

pub use expansion::{ActiveWindowInfo, ExpansionCatalog};
pub use hotkey::HotkeyCatalog;
pub use regex::RegexCatalog;

pub(crate) use expansion::{
    entry_has_app_filters, expand_trigger_action, expand_trigger_action_with_args, is_app_allowed,
    is_excluded_phrase,
};

#[cfg(test)]
mod tests;
